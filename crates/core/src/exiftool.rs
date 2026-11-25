use anyhow::{anyhow, bail, Context, Result};
use serde_json::Value;
use std::ffi::OsStr;
use std::io::{BufRead, BufReader, Write};
use std::path::{self, Path};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::{Mutex, OnceLock};

const EXIFTOOL_PATH_ENV: &str = "FPHOTO_EXIFTOOL_PATH";
static EXIFTOOL: OnceLock<Result<Mutex<Option<ExifTool>>>> = OnceLock::new();

pub(crate) fn read_json(path: &Path, tags: &[&str]) -> Result<Value> {
    let path = path::absolute(path)?;
    let filename = path
        .to_str()
        .filter(|name| !name.contains(['\r', '\n']))
        .context("ExifTool の引数ファイルで扱えないファイル名です")?;
    let instance = EXIFTOOL
        .get_or_init(|| ExifTool::start().map(|process| Mutex::new(Some(process))))
        .as_ref()
        .map_err(|err| anyhow!("{err:#}"))?;
    let mut process = instance
        .lock()
        .map_err(|_| anyhow!("ExifTool のロック取得に失敗しました"))?;
    let exiftool = match process.as_mut() {
        Some(exiftool) => exiftool,
        None => process.insert(ExifTool::start()?),
    };
    let response = match exiftool.request(filename, tags) {
        Ok(response) => response,
        Err(err) => {
            // An interrupted command leaves the stream out of sync.
            *process = None;
            return Err(err);
        }
    };
    let response = response.trim_end();
    let (json, status) = response.rsplit_once('\n').unwrap_or(("", response));
    if status.trim_end() != "0" {
        bail!("ExifTool の終了コード {status}: {}", path.display());
    }
    let [metadata] = serde_json::from_str::<[Value; 1]>(json)
        .context("ExifTool の JSON 応答を解析できませんでした")?;
    if !metadata.is_object() {
        bail!("ExifTool の JSON 応答にメタデータがありません");
    }
    Ok(metadata)
}

struct ExifTool {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
}

impl ExifTool {
    fn start() -> Result<Self> {
        if let Some(executable) = std::env::var_os(EXIFTOOL_PATH_ENV).filter(|p| !p.is_empty()) {
            if let Ok(process) = Self::spawn(&executable) {
                return Ok(process);
            }
        }
        Self::spawn(OsStr::new("exiftool")).context("ExifTool を起動できません")
    }

    fn spawn(executable: &OsStr) -> std::io::Result<Self> {
        let mut command = Command::new(executable);
        command
            .args(["-charset", "filename=UTF8", "-stay_open", "True", "-@", "-"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            command.creation_flags(CREATE_NO_WINDOW);
        }

        let mut child = command.spawn()?;
        let stdin = child.stdin.take().expect("ExifTool stdin is piped");
        let stdout = child.stdout.take().expect("ExifTool stdout is piped");
        Ok(Self {
            child,
            stdin,
            stdout: BufReader::new(stdout),
        })
    }

    fn request(&mut self, filename: &str, tags: &[&str]) -> Result<String> {
        // -echo3 writes the command status to stdout immediately before {ready}.
        writeln!(self.stdin, "-json\n-echo3\n${{status}}")?;
        for tag in tags {
            writeln!(self.stdin, "{tag}")?;
        }
        writeln!(self.stdin, "{filename}\n-execute")?;
        self.stdin.flush()?;

        let mut response = String::new();
        loop {
            let start = response.len();
            if self.stdout.read_line(&mut response)? == 0 {
                bail!("ExifTool が応答の途中で終了しました");
            }
            if response[start..].trim_end() == "{ready}" {
                response.truncate(start);
                return Ok(response);
            }
        }
    }
}

impl Drop for ExifTool {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
