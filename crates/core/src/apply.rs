use crate::config::app_paths;
use crate::planner::{RenameCandidate, RenamePlan};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, Serialize, Deserialize)]
struct UndoLog {
    operations: Vec<RenameOperation>,
    #[serde(default)]
    backup_originals: bool,
    #[serde(default)]
    jpg_root: Option<PathBuf>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct RenameOperation {
    from: PathBuf,
    to: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApplyResult {
    pub applied: usize,
    pub unchanged: usize,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Default)]
pub struct ApplyOptions {
    pub backup_originals: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UndoResult {
    pub restored: usize,
}

pub fn apply_plan(plan: &RenamePlan) -> Result<ApplyResult> {
    apply_plan_with_options(plan, &ApplyOptions::default())
}

pub fn apply_plan_with_options(plan: &RenamePlan, options: &ApplyOptions) -> Result<ApplyResult> {
    let candidates: Vec<&RenameCandidate> = plan.candidates.iter().filter(|c| c.changed).collect();
    if candidates.is_empty() {
        return Ok(ApplyResult {
            applied: 0,
            unchanged: plan.candidates.len(),
        });
    }

    if options.backup_originals {
        backup_original_files(plan, &candidates)?;
    }

    let mut first_phase = HashMap::<PathBuf, PathBuf>::new();

    for (index, candidate) in candidates.iter().enumerate() {
        let temp_path = temp_path_for(&candidate.original_path, index);
        fs::rename(&candidate.original_path, &temp_path).with_context(|| {
            format!(
                "一時リネームに失敗しました: {} -> {}",
                candidate.original_path.display(),
                temp_path.display()
            )
        })?;
        first_phase.insert(candidate.original_path.clone(), temp_path);
    }

    let mut operations = Vec::with_capacity(candidates.len());
    for candidate in candidates {
        let temp = first_phase
            .get(&candidate.original_path)
            .context("一時ファイル情報が見つかりません")?;

        fs::rename(temp, &candidate.target_path).with_context(|| {
            format!(
                "最終リネームに失敗しました: {} -> {}",
                temp.display(),
                candidate.target_path.display()
            )
        })?;

        operations.push(RenameOperation {
            from: candidate.original_path.clone(),
            to: candidate.target_path.clone(),
        });
    }

    persist_undo(&operations, plan, options)?;

    Ok(ApplyResult {
        applied: operations.len(),
        unchanged: plan.candidates.len().saturating_sub(operations.len()),
    })
}

fn backup_original_files(plan: &RenamePlan, candidates: &[&RenameCandidate]) -> Result<()> {
    let backup_root = plan.jpg_root.join("backup");
    fs::create_dir_all(&backup_root).with_context(|| {
        format!(
            "バックアップフォルダを作成できませんでした: {}",
            backup_root.display()
        )
    })?;

    for candidate in candidates {
        let backup_path =
            resolve_backup_path(&backup_root, &plan.jpg_root, &candidate.original_path);
        if let Some(parent) = backup_path.parent() {
            fs::create_dir_all(parent).with_context(|| {
                format!(
                    "バックアップ用フォルダを作成できませんでした: {}",
                    parent.display()
                )
            })?;
        }
        fs::copy(&candidate.original_path, &backup_path).with_context(|| {
            format!(
                "バックアップに失敗しました: {} -> {}",
                candidate.original_path.display(),
                backup_path.display()
            )
        })?;
    }

    Ok(())
}

fn resolve_backup_path(backup_root: &Path, jpg_root: &Path, original_path: &Path) -> PathBuf {
    if let Ok(relative) = original_path.strip_prefix(jpg_root) {
        if !relative.as_os_str().is_empty() {
            let candidate = backup_root.join(relative);
            return unique_backup_path(candidate);
        }
    }

    let file_name = original_path
        .file_name()
        .map(|v| v.to_os_string())
        .unwrap_or_else(|| OsString::from("file"));
    unique_backup_path(backup_root.join(file_name))
}

fn unique_backup_path(candidate: PathBuf) -> PathBuf {
    if !candidate.exists() {
        return candidate;
    }

    let parent = candidate.parent().unwrap_or_else(|| Path::new("."));
    let stem = candidate
        .file_stem()
        .map(|v| v.to_string_lossy().to_string())
        .unwrap_or_else(|| "file".to_string());
    let ext = candidate
        .extension()
        .map(|v| v.to_string_lossy().to_string())
        .unwrap_or_default();

    let mut n = 1usize;
    loop {
        let mut name = format!("{}_{:03}", stem, n);
        if !ext.is_empty() {
            name.push('.');
            name.push_str(&ext);
        }
        let next = parent.join(name);
        if !next.exists() {
            return next;
        }
        n += 1;
    }
}

pub fn undo_last() -> Result<UndoResult> {
    let paths = app_paths()?;
    if !paths.undo_path.exists() {
        anyhow::bail!("取り消し可能な履歴がありません");
    }

    let raw = fs::read_to_string(&paths.undo_path).with_context(|| {
        format!(
            "取り消しログを読めませんでした: {}",
            paths.undo_path.display()
        )
    })?;
    let log = serde_json::from_str::<UndoLog>(&raw).context("取り消しログが壊れています")?;

    for op in log.operations.iter().rev() {
        if !op.to.exists() {
            continue;
        }
        fs::rename(&op.to, &op.from).with_context(|| {
            format!(
                "取り消しに失敗しました: {} -> {}",
                op.to.display(),
                op.from.display()
            )
        })?;
    }

    cleanup_backup_if_needed(&log)?;

    fs::remove_file(&paths.undo_path).with_context(|| {
        format!(
            "取り消しログ削除に失敗しました: {}",
            paths.undo_path.display()
        )
    })?;

    Ok(UndoResult {
        restored: log.operations.len(),
    })
}

fn persist_undo(
    operations: &[RenameOperation],
    plan: &RenamePlan,
    options: &ApplyOptions,
) -> Result<()> {
    let paths = app_paths()?;
    fs::create_dir_all(&paths.config_dir).with_context(|| {
        format!(
            "設定ディレクトリ作成に失敗しました: {}",
            paths.config_dir.display()
        )
    })?;

    let log = UndoLog {
        operations: operations.to_vec(),
        backup_originals: options.backup_originals,
        jpg_root: Some(plan.jpg_root.clone()),
    };
    let body =
        serde_json::to_string_pretty(&log).context("取り消しログのシリアライズに失敗しました")?;
    fs::write(&paths.undo_path, body).with_context(|| {
        format!(
            "取り消しログ書き込みに失敗しました: {}",
            paths.undo_path.display()
        )
    })?;
    Ok(())
}

fn cleanup_backup_if_needed(log: &UndoLog) -> Result<()> {
    if !log.backup_originals {
        return Ok(());
    }

    let Some(jpg_root) = log.jpg_root.as_ref() else {
        return Ok(());
    };

    let backup_root = jpg_root.join("backup");
    if !backup_root.exists() {
        return Ok(());
    }

    if backup_root.is_dir() {
        fs::remove_dir_all(&backup_root).with_context(|| {
            format!(
                "バックアップフォルダ削除に失敗しました: {}",
                backup_root.display()
            )
        })?;
    } else {
        fs::remove_file(&backup_root).with_context(|| {
            format!(
                "バックアップファイル削除に失敗しました: {}",
                backup_root.display()
            )
        })?;
    }

    Ok(())
}

fn temp_path_for(original_path: &Path, index: usize) -> PathBuf {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let parent = original_path.parent().unwrap_or_else(|| Path::new("."));
    let file_name = original_path
        .file_name()
        .map(|v| v.to_string_lossy().to_string())
        .unwrap_or_else(|| "file".to_string());
    parent.join(format!(".fphoto_tmp_{}_{}_{}", now, index, file_name))
}

#[cfg(test)]
mod tests {
    use super::{
        apply_plan_with_options, cleanup_backup_if_needed, resolve_backup_path, unique_backup_path,
        ApplyOptions, UndoLog,
    };
    use crate::metadata::{MetadataSource, PhotoMetadata};
    use crate::planner::{RenameCandidate, RenamePlan, RenameStats};
    use chrono::Local;
    use std::fs;
    use std::path::PathBuf;
    use tempfile::tempdir;

    fn sample_metadata(jpg_path: PathBuf) -> PhotoMetadata {
        PhotoMetadata {
            source: MetadataSource::JpgExif,
            date: Local::now(),
            camera_make: Some("FUJIFILM".to_string()),
            camera_model: Some("X-T5".to_string()),
            lens_make: Some("FUJIFILM".to_string()),
            lens_model: Some("XF16-55".to_string()),
            film_sim: Some("CLASSIC CHROME".to_string()),
            original_name: "IMG_0001".to_string(),
            jpg_path,
        }
    }

    #[test]
    fn apply_plan_returns_unchanged_when_no_candidates_changed() {
        let temp = tempdir().expect("tempdir");
        let jpg_root = temp.path().join("jpg");
        fs::create_dir_all(&jpg_root).expect("create jpg root");

        let original = jpg_root.join("IMG_0001.JPG");
        let target = jpg_root.join("IMG_0001.JPG");
        let plan = RenamePlan {
            jpg_root: jpg_root.clone(),
            template: "{orig_name}".to_string(),
            exclusions: Vec::new(),
            candidates: vec![RenameCandidate {
                original_path: original.clone(),
                target_path: target,
                metadata_source: MetadataSource::JpgExif,
                metadata: sample_metadata(original),
                rendered_base: "IMG_0001".to_string(),
                changed: false,
            }],
            stats: RenameStats::default(),
        };

        let result = apply_plan_with_options(&plan, &ApplyOptions::default())
            .expect("unchanged plan should be accepted");
        assert_eq!(result.applied, 0);
        assert_eq!(result.unchanged, 1);
    }

    #[test]
    fn unique_backup_path_adds_incremental_suffix() {
        let temp = tempdir().expect("tempdir");
        let candidate = temp.path().join("IMG_0001.JPG");
        fs::write(&candidate, b"x").expect("create first");
        fs::write(temp.path().join("IMG_0001_001.JPG"), b"x").expect("create second");

        let resolved = unique_backup_path(candidate);
        assert_eq!(
            resolved.file_name().and_then(|v| v.to_str()),
            Some("IMG_0001_002.JPG")
        );
    }

    #[test]
    fn resolve_backup_path_keeps_relative_tree_under_backup_root() {
        let temp = tempdir().expect("tempdir");
        let jpg_root = temp.path().join("jpg");
        let backup_root = jpg_root.join("backup");
        let original = jpg_root.join("nested").join("IMG_0001.JPG");

        let backup_path = resolve_backup_path(&backup_root, &jpg_root, &original);
        assert_eq!(backup_path, backup_root.join("nested").join("IMG_0001.JPG"));
    }

    #[test]
    fn resolve_backup_path_falls_back_to_filename_for_outside_root() {
        let temp = tempdir().expect("tempdir");
        let jpg_root = temp.path().join("jpg");
        let backup_root = jpg_root.join("backup");
        let original = temp.path().join("other").join("IMG_9999.JPG");

        let backup_path = resolve_backup_path(&backup_root, &jpg_root, &original);
        assert_eq!(backup_path, backup_root.join("IMG_9999.JPG"));
    }

    #[test]
    fn cleanup_backup_if_needed_removes_backup_directory() {
        let temp = tempdir().expect("tempdir");
        let jpg_root = temp.path().join("jpg");
        let backup_root = jpg_root.join("backup");
        fs::create_dir_all(&backup_root).expect("create backup root");
        fs::write(backup_root.join("file.txt"), b"x").expect("create backup file");

        let log = UndoLog {
            operations: Vec::new(),
            backup_originals: true,
            jpg_root: Some(jpg_root.clone()),
        };
        cleanup_backup_if_needed(&log).expect("cleanup should succeed");
        assert!(!backup_root.exists());
    }

    #[test]
    fn cleanup_backup_if_needed_keeps_backup_when_disabled() {
        let temp = tempdir().expect("tempdir");
        let jpg_root = temp.path().join("jpg");
        let backup_root = jpg_root.join("backup");
        fs::create_dir_all(&backup_root).expect("create backup root");

        let log = UndoLog {
            operations: Vec::new(),
            backup_originals: false,
            jpg_root: Some(jpg_root),
        };
        cleanup_backup_if_needed(&log).expect("cleanup should succeed");
        assert!(backup_root.exists());
    }
}
