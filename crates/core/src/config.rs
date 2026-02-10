use crate::DEFAULT_TEMPLATE;
use anyhow::{Context, Result};
use directories::ProjectDirs;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    pub template: String,
    pub exclude_strings: Vec<String>,
    #[serde(default = "default_true")]
    pub dedupe_same_maker: bool,
    #[serde(default)]
    pub backup_originals: bool,
    #[serde(default)]
    pub raw_parent_if_missing: bool,
}

fn default_true() -> bool {
    true
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            template: DEFAULT_TEMPLATE.to_string(),
            exclude_strings: Vec::new(),
            dedupe_same_maker: true,
            backup_originals: false,
            raw_parent_if_missing: false,
        }
    }
}

#[derive(Debug, Clone)]
pub struct AppPaths {
    pub config_dir: PathBuf,
    pub config_path: PathBuf,
    pub undo_path: PathBuf,
}

pub fn app_paths() -> Result<AppPaths> {
    let proj = ProjectDirs::from("com", "kelly", "fphoto-renamer")
        .context("OS標準設定ディレクトリを取得できませんでした")?;
    let config_dir = proj.config_dir().to_path_buf();
    Ok(AppPaths {
        config_path: config_dir.join("config.toml"),
        undo_path: config_dir.join("undo-last.json"),
        config_dir,
    })
}

pub fn load_config() -> Result<AppConfig> {
    let paths = app_paths()?;
    if !paths.config_path.exists() {
        return Ok(AppConfig::default());
    }

    let raw = fs::read_to_string(&paths.config_path).with_context(|| {
        format!(
            "設定ファイルを読めませんでした: {}",
            paths.config_path.display()
        )
    })?;

    let config = toml::from_str::<AppConfig>(&raw).context("設定ファイルのパースに失敗しました")?;
    Ok(config)
}

pub fn save_config(config: &AppConfig) -> Result<()> {
    let paths = app_paths()?;
    fs::create_dir_all(&paths.config_dir).with_context(|| {
        format!(
            "設定ディレクトリを作成できませんでした: {}",
            paths.config_dir.display()
        )
    })?;
    let body = toml::to_string_pretty(config).context("設定のシリアライズに失敗しました")?;
    write_file_atomically(&paths.config_path, &body, "設定ファイル")?;
    Ok(())
}

fn write_file_atomically(target_path: &Path, body: &str, label: &str) -> Result<()> {
    let file_name = target_path
        .file_name()
        .and_then(|v| v.to_str())
        .unwrap_or("config");
    let temp_path = target_path.with_file_name(format!(".{file_name}.{}.tmp", std::process::id()));

    fs::write(&temp_path, body).with_context(|| {
        format!(
            "{label}の一時ファイル書き込みに失敗しました: {}",
            temp_path.display()
        )
    })?;

    match fs::rename(&temp_path, target_path) {
        Ok(()) => Ok(()),
        Err(primary_rename_err) => {
            if target_path.exists() {
                fs::remove_file(target_path).with_context(|| {
                    format!(
                        "{label}の既存ファイル削除に失敗しました: {}",
                        target_path.display()
                    )
                })?;
                fs::rename(&temp_path, target_path).with_context(|| {
                    format!(
                        "{label}の置き換えに失敗しました: {} -> {}",
                        temp_path.display(),
                        target_path.display()
                    )
                })?;
                return Ok(());
            }

            let _ = fs::remove_file(&temp_path);
            Err(anyhow::Error::from(primary_rename_err).context(format!(
                "{label}の置き換えに失敗しました: {} -> {}",
                temp_path.display(),
                target_path.display()
            )))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::AppConfig;
    use crate::DEFAULT_TEMPLATE;

    #[test]
    fn default_config_has_expected_values() {
        let cfg = AppConfig::default();
        assert_eq!(cfg.template, DEFAULT_TEMPLATE);
        assert!(cfg.exclude_strings.is_empty());
        assert!(cfg.dedupe_same_maker);
        assert!(!cfg.backup_originals);
        assert!(!cfg.raw_parent_if_missing);
    }

    #[test]
    fn deserialize_legacy_config_defaults_new_flags_to_false() {
        let raw = r#"
date_format = "YYYYMMDDHHMMSS"
recursive_default = true
include_hidden_default = false
language = "ja"
template = "{orig_name}"
exclude_strings = ["-NR"]
"#;
        let cfg: AppConfig = toml::from_str(raw).expect("legacy config should deserialize");
        assert_eq!(cfg.template, "{orig_name}");
        assert_eq!(cfg.exclude_strings, vec!["-NR"]);
        assert!(cfg.dedupe_same_maker);
        assert!(!cfg.backup_originals);
        assert!(!cfg.raw_parent_if_missing);
    }
}
