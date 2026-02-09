use crate::config::app_paths;
use crate::planner::{RenameCandidate, RenamePlan};
use anyhow::{bail, Context, Result};
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
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

    validate_apply_candidates(plan, &candidates)?;

    if options.backup_originals {
        backup_original_files(plan, &candidates)?;
    }

    let mut staged = Vec::<StagedRename>::with_capacity(candidates.len());
    for (index, candidate) in candidates.iter().enumerate() {
        let entry = StagedRename {
            original_path: candidate.original_path.clone(),
            target_path: candidate.target_path.clone(),
            temp_path: temp_path_for(&candidate.original_path, index),
        };
        if let Err(err) = fs::rename(&entry.original_path, &entry.temp_path) {
            let stage_err = anyhow::Error::from(err).context(format!(
                "一時リネームに失敗しました: {} -> {}",
                entry.original_path.display(),
                entry.temp_path.display()
            ));
            if let Err(rollback_err) = rollback_staged_to_original_paths(&staged) {
                return Err(stage_err.context(format!(
                    "一時リネーム失敗後のロールバックにも失敗しました: {rollback_err}"
                )));
            }
            return Err(stage_err);
        }
        staged.push(entry);
    }

    let mut operations = Vec::with_capacity(candidates.len());
    for (finalized, entry) in staged.iter().enumerate() {
        if let Err(err) = fs::rename(&entry.temp_path, &entry.target_path) {
            let apply_err = anyhow::Error::from(err).context(format!(
                "最終リネームに失敗しました: {} -> {}",
                entry.temp_path.display(),
                entry.target_path.display()
            ));
            if let Err(rollback_err) = rollback_after_final_rename_failure(&staged, finalized) {
                return Err(apply_err.context(format!(
                    "最終リネーム失敗後のロールバックにも失敗しました: {rollback_err}"
                )));
            }
            return Err(apply_err);
        }

        operations.push(RenameOperation {
            from: entry.original_path.clone(),
            to: entry.target_path.clone(),
        });
    }

    persist_undo(&operations, plan, options)?;

    Ok(ApplyResult {
        applied: operations.len(),
        unchanged: plan.candidates.len().saturating_sub(operations.len()),
    })
}

#[derive(Debug, Clone)]
struct StagedRename {
    original_path: PathBuf,
    target_path: PathBuf,
    temp_path: PathBuf,
}

fn validate_apply_candidates(plan: &RenamePlan, candidates: &[&RenameCandidate]) -> Result<()> {
    let jpg_root = fs::canonicalize(&plan.jpg_root).with_context(|| {
        format!(
            "JPGルートを解決できませんでした: {}",
            plan.jpg_root.display()
        )
    })?;
    let mut seen_original_paths = HashSet::<PathBuf>::new();
    let mut seen_target_paths = HashSet::<PathBuf>::new();

    for candidate in candidates {
        let original_canonical = fs::canonicalize(&candidate.original_path).with_context(|| {
            format!(
                "元ファイルを解決できませんでした: {}",
                candidate.original_path.display()
            )
        })?;
        if !original_canonical.starts_with(&jpg_root) {
            bail!(
                "JPGフォルダ外の元ファイルは適用できません: {}",
                candidate.original_path.display()
            );
        }
        if !seen_original_paths.insert(original_canonical) {
            bail!(
                "重複した元ファイルが含まれています: {}",
                candidate.original_path.display()
            );
        }

        let target_parent = candidate.target_path.parent().with_context(|| {
            format!(
                "リネーム先に親ディレクトリがありません: {}",
                candidate.target_path.display()
            )
        })?;
        let target_name = candidate.target_path.file_name().with_context(|| {
            format!(
                "リネーム先ファイル名が不正です: {}",
                candidate.target_path.display()
            )
        })?;
        let target_parent_canonical = fs::canonicalize(target_parent).with_context(|| {
            format!(
                "リネーム先親ディレクトリを解決できませんでした: {}",
                target_parent.display()
            )
        })?;
        if !target_parent_canonical.starts_with(&jpg_root) {
            bail!(
                "JPGフォルダ外のリネーム先は適用できません: {}",
                candidate.target_path.display()
            );
        }
        let normalized_target = target_parent_canonical.join(target_name);
        if !seen_target_paths.insert(normalized_target) {
            bail!(
                "重複したリネーム先が含まれています: {}",
                candidate.target_path.display()
            );
        }
    }

    Ok(())
}

fn rollback_staged_to_original_paths(staged: &[StagedRename]) -> Result<()> {
    for entry in staged.iter().rev() {
        if !entry.temp_path.exists() {
            continue;
        }
        fs::rename(&entry.temp_path, &entry.original_path).with_context(|| {
            format!(
                "ロールバックに失敗しました: {} -> {}",
                entry.temp_path.display(),
                entry.original_path.display()
            )
        })?;
    }
    Ok(())
}

fn rollback_after_final_rename_failure(staged: &[StagedRename], finalized: usize) -> Result<()> {
    for entry in staged[..finalized].iter().rev() {
        if !entry.target_path.exists() {
            continue;
        }
        fs::rename(&entry.target_path, &entry.temp_path).with_context(|| {
            format!(
                "ロールバック(退避)に失敗しました: {} -> {}",
                entry.target_path.display(),
                entry.temp_path.display()
            )
        })?;
    }
    rollback_staged_to_original_paths(staged)
}

fn backup_original_files(plan: &RenamePlan, candidates: &[&RenameCandidate]) -> Result<()> {
    let backup_root = plan.jpg_root.join("backup");
    fs::create_dir_all(&backup_root).with_context(|| {
        format!(
            "バックアップフォルダを作成できませんでした: {}",
            backup_root.display()
        )
    })?;

    let mut reserved_paths = HashSet::<PathBuf>::new();
    let mut backup_jobs = Vec::<(PathBuf, PathBuf)>::with_capacity(candidates.len());
    for candidate in candidates {
        let backup_path = resolve_backup_path_with_reserved(
            &backup_root,
            &plan.jpg_root,
            &candidate.original_path,
            &mut reserved_paths,
        );
        backup_jobs.push((candidate.original_path.clone(), backup_path));
    }

    backup_jobs
        .par_iter()
        .try_for_each(|(original_path, backup_path)| -> Result<()> {
            if let Some(parent) = backup_path.parent() {
                fs::create_dir_all(parent).with_context(|| {
                    format!(
                        "バックアップ用フォルダを作成できませんでした: {}",
                        parent.display()
                    )
                })?;
            }
            fs::copy(original_path, backup_path).with_context(|| {
                format!(
                    "バックアップに失敗しました: {} -> {}",
                    original_path.display(),
                    backup_path.display()
                )
            })?;
            Ok(())
        })?;

    Ok(())
}

#[cfg(test)]
fn resolve_backup_path(backup_root: &Path, jpg_root: &Path, original_path: &Path) -> PathBuf {
    let mut reserved_paths = HashSet::<PathBuf>::new();
    resolve_backup_path_with_reserved(backup_root, jpg_root, original_path, &mut reserved_paths)
}

fn resolve_backup_path_with_reserved(
    backup_root: &Path,
    jpg_root: &Path,
    original_path: &Path,
    reserved_paths: &mut HashSet<PathBuf>,
) -> PathBuf {
    if let Ok(relative) = original_path.strip_prefix(jpg_root) {
        if !relative.as_os_str().is_empty() {
            let candidate = backup_root.join(relative);
            return unique_backup_path_with_reserved(candidate, reserved_paths);
        }
    }

    let file_name = original_path
        .file_name()
        .map(|v| v.to_os_string())
        .unwrap_or_else(|| OsString::from("file"));
    unique_backup_path_with_reserved(backup_root.join(file_name), reserved_paths)
}

#[cfg(test)]
fn unique_backup_path(candidate: PathBuf) -> PathBuf {
    let mut reserved_paths = HashSet::<PathBuf>::new();
    unique_backup_path_with_reserved(candidate, &mut reserved_paths)
}

fn unique_backup_path_with_reserved(
    candidate: PathBuf,
    reserved_paths: &mut HashSet<PathBuf>,
) -> PathBuf {
    if !candidate.exists() && !reserved_paths.contains(&candidate) {
        reserved_paths.insert(candidate.clone());
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
        if !next.exists() && !reserved_paths.contains(&next) {
            reserved_paths.insert(next.clone());
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

    let restored = restore_operations(&log)?;

    cleanup_backup_if_needed(&log)?;

    fs::remove_file(&paths.undo_path).with_context(|| {
        format!(
            "取り消しログ削除に失敗しました: {}",
            paths.undo_path.display()
        )
    })?;

    Ok(UndoResult { restored })
}

fn restore_operations(log: &UndoLog) -> Result<usize> {
    let mut restored = 0usize;
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
        restored += 1;
    }
    Ok(restored)
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
        apply_plan_with_options, cleanup_backup_if_needed, resolve_backup_path,
        resolve_backup_path_with_reserved, restore_operations, unique_backup_path, ApplyOptions,
        UndoLog,
    };
    use crate::metadata::{MetadataSource, PhotoMetadata};
    use crate::planner::{RenameCandidate, RenamePlan, RenameStats};
    use chrono::Local;
    use std::collections::HashSet;
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
                source_label: "jpg".to_string(),
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

    #[test]
    fn resolve_backup_path_with_reserved_avoids_in_batch_collisions() {
        let temp = tempdir().expect("tempdir");
        let jpg_root = temp.path().join("jpg");
        let backup_root = jpg_root.join("backup");
        fs::create_dir_all(&backup_root).expect("create backup root");

        let original_a = temp.path().join("a").join("IMG_0001.JPG");
        let original_b = temp.path().join("b").join("IMG_0001.JPG");

        let mut reserved = HashSet::<PathBuf>::new();
        let backup_a =
            resolve_backup_path_with_reserved(&backup_root, &jpg_root, &original_a, &mut reserved);
        let backup_b =
            resolve_backup_path_with_reserved(&backup_root, &jpg_root, &original_b, &mut reserved);

        assert_eq!(backup_a, backup_root.join("IMG_0001.JPG"));
        assert_eq!(backup_b, backup_root.join("IMG_0001_001.JPG"));
    }

    #[test]
    fn apply_plan_rolls_back_when_final_rename_fails_midway() {
        let temp = tempdir().expect("tempdir");
        let jpg_root = temp.path().join("jpg");
        fs::create_dir_all(&jpg_root).expect("create jpg root");

        let original_a = jpg_root.join("IMG_A.JPG");
        let original_b = jpg_root.join("IMG_B.JPG");
        fs::write(&original_a, b"A").expect("write A");
        fs::write(&original_b, b"B").expect("write B");

        let blocked_dir = jpg_root.join("blocked");
        fs::create_dir_all(&blocked_dir).expect("create blocked dir");
        fs::write(blocked_dir.join("keep.txt"), b"x").expect("write keep");

        let renamed_a = jpg_root.join("RENAMED_A.JPG");
        let plan = RenamePlan {
            jpg_root: jpg_root.clone(),
            template: "{orig_name}".to_string(),
            exclusions: Vec::new(),
            candidates: vec![
                RenameCandidate {
                    original_path: original_a.clone(),
                    target_path: renamed_a.clone(),
                    metadata_source: MetadataSource::JpgExif,
                    source_label: "jpg".to_string(),
                    metadata: sample_metadata(original_a.clone()),
                    rendered_base: "RENAMED_A".to_string(),
                    changed: true,
                },
                RenameCandidate {
                    original_path: original_b.clone(),
                    target_path: blocked_dir.clone(),
                    metadata_source: MetadataSource::JpgExif,
                    source_label: "jpg".to_string(),
                    metadata: sample_metadata(original_b.clone()),
                    rendered_base: "blocked".to_string(),
                    changed: true,
                },
            ],
            stats: RenameStats::default(),
        };

        let err = apply_plan_with_options(&plan, &ApplyOptions::default())
            .expect_err("second phase should fail");
        assert!(err.to_string().contains("最終リネームに失敗しました"));

        assert!(original_a.exists(), "original A should be restored");
        assert!(original_b.exists(), "original B should be restored");
        assert!(!renamed_a.exists(), "renamed A should be rolled back");

        let has_temp = fs::read_dir(&jpg_root)
            .expect("read jpg root")
            .flatten()
            .any(|entry| {
                entry
                    .file_name()
                    .to_string_lossy()
                    .starts_with(".fphoto_tmp_")
            });
        assert!(!has_temp, "temporary files should not remain");
    }

    #[test]
    fn apply_plan_rejects_target_outside_jpg_root() {
        let temp = tempdir().expect("tempdir");
        let jpg_root = temp.path().join("jpg");
        let outside_root = temp.path().join("outside");
        fs::create_dir_all(&jpg_root).expect("create jpg root");
        fs::create_dir_all(&outside_root).expect("create outside root");

        let original = jpg_root.join("IMG_0001.JPG");
        fs::write(&original, b"x").expect("write original");
        let outside_target = outside_root.join("RENAMED.JPG");
        let plan = RenamePlan {
            jpg_root: jpg_root.clone(),
            template: "{orig_name}".to_string(),
            exclusions: Vec::new(),
            candidates: vec![RenameCandidate {
                original_path: original.clone(),
                target_path: outside_target,
                metadata_source: MetadataSource::JpgExif,
                source_label: "jpg".to_string(),
                metadata: sample_metadata(original.clone()),
                rendered_base: "RENAMED".to_string(),
                changed: true,
            }],
            stats: RenameStats::default(),
        };

        let err = apply_plan_with_options(&plan, &ApplyOptions::default())
            .expect_err("outside target should be rejected");
        assert!(err
            .to_string()
            .contains("JPGフォルダ外のリネーム先は適用できません"));
        assert!(original.exists(), "original file should stay untouched");
    }

    #[test]
    fn apply_plan_rejects_duplicate_targets() {
        let temp = tempdir().expect("tempdir");
        let jpg_root = temp.path().join("jpg");
        fs::create_dir_all(&jpg_root).expect("create jpg root");

        let original_a = jpg_root.join("IMG_A.JPG");
        let original_b = jpg_root.join("IMG_B.JPG");
        fs::write(&original_a, b"A").expect("write A");
        fs::write(&original_b, b"B").expect("write B");

        let duplicate_target = jpg_root.join("SAME.JPG");
        let plan = RenamePlan {
            jpg_root: jpg_root.clone(),
            template: "{orig_name}".to_string(),
            exclusions: Vec::new(),
            candidates: vec![
                RenameCandidate {
                    original_path: original_a.clone(),
                    target_path: duplicate_target.clone(),
                    metadata_source: MetadataSource::JpgExif,
                    source_label: "jpg".to_string(),
                    metadata: sample_metadata(original_a.clone()),
                    rendered_base: "SAME".to_string(),
                    changed: true,
                },
                RenameCandidate {
                    original_path: original_b.clone(),
                    target_path: duplicate_target,
                    metadata_source: MetadataSource::JpgExif,
                    source_label: "jpg".to_string(),
                    metadata: sample_metadata(original_b.clone()),
                    rendered_base: "SAME".to_string(),
                    changed: true,
                },
            ],
            stats: RenameStats::default(),
        };

        let err = apply_plan_with_options(&plan, &ApplyOptions::default())
            .expect_err("duplicate targets should be rejected");
        assert!(err
            .to_string()
            .contains("重複したリネーム先が含まれています"));
        assert!(original_a.exists());
        assert!(original_b.exists());
    }

    #[test]
    fn restore_operations_counts_only_existing_targets() {
        let temp = tempdir().expect("tempdir");
        let from_a = temp.path().join("A.JPG");
        let to_a = temp.path().join("RENAMED_A.JPG");
        let from_b = temp.path().join("B.JPG");
        let to_b = temp.path().join("RENAMED_B.JPG");
        fs::write(&to_a, b"A").expect("write renamed A");

        let log = UndoLog {
            operations: vec![
                super::RenameOperation {
                    from: from_a.clone(),
                    to: to_a.clone(),
                },
                super::RenameOperation {
                    from: from_b.clone(),
                    to: to_b,
                },
            ],
            backup_originals: false,
            jpg_root: None,
        };

        let restored = restore_operations(&log).expect("restore should succeed");
        assert_eq!(restored, 1);
        assert!(from_a.exists());
        assert!(!to_a.exists());
        assert!(!from_b.exists());
    }
}
