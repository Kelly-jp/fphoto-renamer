use anyhow::Result;
use clap::ArgAction;
use clap::{Args, Parser, Subcommand, ValueEnum};
use fphoto_renamer_core::{
    app_paths, apply_plan_with_options, generate_plan, load_config, parse_template, undo_last,
    ApplyOptions, PlanOptions, DEFAULT_TEMPLATE,
};
use std::path::PathBuf;

const EXIFTOOL_PATH_ENV: &str = "FPHOTO_EXIFTOOL_PATH";

#[derive(Debug, Parser)]
#[command(name = "fphoto-renamer-cli")]
#[command(about = "JPG写真のファイル名をテンプレートで一括リネームします")]
#[command(version)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Debug, Subcommand)]
enum Commands {
    Rename(RenameArgs),
    Undo,
    Config(ConfigArgs),
}

#[derive(Debug, Args)]
struct ConfigArgs {
    #[command(subcommand)]
    action: ConfigAction,
}

#[derive(Debug, Subcommand)]
enum ConfigAction {
    Show,
}

#[derive(Debug, Args)]
struct RenameArgs {
    #[arg(long)]
    jpg_input: String,
    #[arg(long)]
    raw_input: Option<String>,
    #[arg(long, default_value_t = false)]
    raw_parent_if_missing: bool,
    #[arg(long, default_value_t = false)]
    apply: bool,
    #[arg(
        long,
        default_value = DEFAULT_TEMPLATE
    )]
    template: String,
    #[arg(long, allow_hyphen_values = true)]
    exclude: Vec<String>,
    #[arg(long = "dedupe-same-maker", default_value_t = true, action = ArgAction::Set)]
    dedupe_same_maker: bool,
    #[arg(long, default_value_t = false)]
    backup_originals: bool,
    #[arg(long, value_enum, default_value_t = OutputFormat::Table)]
    output: OutputFormat,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum OutputFormat {
    Table,
    Json,
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Rename(args) => cmd_rename(args),
        Commands::Undo => cmd_undo(),
        Commands::Config(config) => match config.action {
            ConfigAction::Show => cmd_config_show(),
        },
    }
}

fn cmd_rename(args: RenameArgs) -> Result<()> {
    configure_exiftool_path();
    parse_template(&args.template)?;

    let options = PlanOptions {
        jpg_input: args.jpg_input.into(),
        raw_input: args.raw_input.map(Into::into),
        raw_from_jpg_parent_when_missing: args.raw_parent_if_missing,
        recursive: false,
        include_hidden: false,
        template: args.template,
        dedupe_same_maker: args.dedupe_same_maker,
        exclusions: args.exclude,
        max_filename_len: 240,
    };

    let plan = generate_plan(&options)?;

    match args.output {
        OutputFormat::Json => {
            println!("{}", serde_json::to_string_pretty(&plan)?);
        }
        OutputFormat::Table => {
            print_table(&plan);
        }
    }

    if args.apply {
        let result = apply_plan_with_options(
            &plan,
            &ApplyOptions {
                backup_originals: args.backup_originals,
            },
        )?;
        eprintln!(
            "適用完了: {}件 (変更なし {}件)",
            result.applied, result.unchanged
        );
    } else {
        eprintln!("dry-run: リネームは未実行です。実行する場合は --apply を指定してください。");
    }

    Ok(())
}

fn configure_exiftool_path() {
    if std::env::var_os(EXIFTOOL_PATH_ENV).is_some() {
        return;
    }

    for candidate in exiftool_path_candidates() {
        if candidate.is_file() {
            std::env::set_var(EXIFTOOL_PATH_ENV, candidate);
            return;
        }
    }
}

fn exiftool_path_candidates() -> Vec<PathBuf> {
    let mut candidates = Vec::new();

    #[cfg(target_os = "windows")]
    let binary_name = "exiftool.exe";
    #[cfg(not(target_os = "windows"))]
    let binary_name = "exiftool";

    if let Some(path) = find_in_path(binary_name) {
        candidates.push(path);
    }

    if let Ok(exe_path) = std::env::current_exe() {
        if let Some(exe_dir) = exe_path.parent() {
            #[cfg(target_os = "windows")]
            {
                candidates.push(exe_dir.join("resources/bin/windows/exiftool.exe"));
                candidates.push(exe_dir.join("exiftool.exe"));
            }
            #[cfg(target_os = "macos")]
            {
                candidates.push(exe_dir.join("resources/bin/macos/exiftool"));
                candidates.push(exe_dir.join("exiftool"));
            }
            #[cfg(target_os = "linux")]
            {
                candidates.push(exe_dir.join("resources/bin/linux/exiftool"));
                candidates.push(exe_dir.join("exiftool"));
            }
        }
    }

    #[cfg(target_os = "windows")]
    {
        candidates.push(
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../gui/src-tauri/resources/bin/windows/exiftool.exe"),
        );
    }
    #[cfg(target_os = "macos")]
    {
        candidates.push(
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../gui/src-tauri/resources/bin/macos/exiftool"),
        );
        candidates.push(PathBuf::from("/opt/homebrew/bin/exiftool"));
        candidates.push(PathBuf::from("/usr/local/bin/exiftool"));
        candidates.push(PathBuf::from("/usr/bin/exiftool"));
    }
    #[cfg(target_os = "linux")]
    {
        candidates.push(
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../gui/src-tauri/resources/bin/linux/exiftool"),
        );
        candidates.push(PathBuf::from("/usr/local/bin/exiftool"));
        candidates.push(PathBuf::from("/usr/bin/exiftool"));
    }

    candidates
}

fn find_in_path(binary_name: &str) -> Option<PathBuf> {
    let path_var = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path_var) {
        let candidate = dir.join(binary_name);
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

fn cmd_undo() -> Result<()> {
    let result = undo_last()?;
    println!("取り消し完了: {}件", result.restored);
    Ok(())
}

fn cmd_config_show() -> Result<()> {
    let config = load_config()?;
    let paths = app_paths()?;
    println!("設定ファイル: {}", paths.config_path.display());
    println!("{}", toml::to_string_pretty(&config)?);
    Ok(())
}

fn print_table(plan: &fphoto_renamer_core::RenamePlan) {
    println!("元ファイル -> 新ファイル (source)");
    for candidate in &plan.candidates {
        println!(
            "{} -> {} ({})",
            candidate.original_path.display(),
            candidate.target_path.display(),
            candidate.source_label
        );
    }

    println!(
        "\n集計: scanned={} jpg={} non_jpg_skip={} unchanged={}",
        plan.stats.scanned_files,
        plan.stats.jpg_files,
        plan.stats.skipped_non_jpg,
        plan.stats.unchanged
    );
}

#[cfg(test)]
mod tests {
    use super::{Cli, Commands, OutputFormat};
    use clap::error::ErrorKind;
    use clap::Parser;
    use fphoto_renamer_core::DEFAULT_TEMPLATE;

    #[test]
    fn parse_rename_defaults() {
        let cli = Cli::try_parse_from(["fphoto-renamer-cli", "rename", "--jpg-input", "/tmp/jpg"])
            .expect("parse should succeed");

        match cli.command {
            Commands::Rename(args) => {
                assert_eq!(args.jpg_input, "/tmp/jpg");
                assert_eq!(args.raw_input, None);
                assert!(!args.raw_parent_if_missing);
                assert!(!args.apply);
                assert_eq!(args.template, DEFAULT_TEMPLATE);
                assert!(args.exclude.is_empty());
                assert!(args.dedupe_same_maker);
                assert!(!args.backup_originals);
                assert!(matches!(args.output, OutputFormat::Table));
            }
            _ => panic!("rename command expected"),
        }
    }

    #[test]
    fn parse_rename_with_explicit_values() {
        let cli = Cli::try_parse_from([
            "fphoto-renamer-cli",
            "rename",
            "--jpg-input",
            "/tmp/jpg",
            "--raw-input",
            "/tmp/raw",
            "--raw-parent-if-missing",
            "--apply",
            "--template",
            "{orig_name}",
            "--exclude",
            "-NR",
            "--exclude",
            "-DxO",
            "--dedupe-same-maker=false",
            "--backup-originals",
            "--output",
            "json",
        ])
        .expect("parse should succeed");

        match cli.command {
            Commands::Rename(args) => {
                assert_eq!(args.raw_input.as_deref(), Some("/tmp/raw"));
                assert!(args.raw_parent_if_missing);
                assert!(args.apply);
                assert_eq!(args.template, "{orig_name}");
                assert_eq!(args.exclude, vec!["-NR".to_string(), "-DxO".to_string()]);
                assert!(!args.dedupe_same_maker);
                assert!(args.backup_originals);
                assert!(matches!(args.output, OutputFormat::Json));
            }
            _ => panic!("rename command expected"),
        }
    }

    #[test]
    fn parse_invalid_output_value_fails() {
        let err = Cli::try_parse_from([
            "fphoto-renamer-cli",
            "rename",
            "--jpg-input",
            "/tmp/jpg",
            "--output",
            "yaml",
        ])
        .expect_err("invalid output should fail");
        let rendered = err.to_string();
        assert!(
            rendered.contains("invalid value"),
            "unexpected parse error: {rendered}"
        );
    }

    #[test]
    fn parse_version_option_displays_version() {
        let err = Cli::try_parse_from(["fphoto-renamer-cli", "--version"])
            .expect_err("--version should display version and exit");
        assert_eq!(err.kind(), ErrorKind::DisplayVersion);

        let rendered = err.to_string();
        assert!(
            rendered.contains(env!("CARGO_PKG_VERSION")),
            "version is not included in output: {rendered}"
        );
    }
}
