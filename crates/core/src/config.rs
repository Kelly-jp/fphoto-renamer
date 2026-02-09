use crate::DEFAULT_TEMPLATE;
use anyhow::{Context, Result};
use directories::ProjectDirs;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    pub template: String,
    pub exclude_strings: Vec<String>,
    #[serde(default)]
    pub backup_originals: bool,
    #[serde(default)]
    pub raw_parent_if_missing: bool,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            template: DEFAULT_TEMPLATE.to_string(),
            exclude_strings: Vec::new(),
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
    fs::write(&paths.config_path, body).with_context(|| {
        format!(
            "設定ファイルを書き込めませんでした: {}",
            paths.config_path.display()
        )
    })?;
    Ok(())
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
        assert!(!cfg.backup_originals);
        assert!(!cfg.raw_parent_if_missing);
    }
}
