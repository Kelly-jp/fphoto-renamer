use crate::config::{app_paths, AppPaths};
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
    #[serde(default)]
    jpg_roots: Vec<PathBuf>,
    #[serde(default)]
    backup_paths: Vec<PathBuf>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct RenameOperation {
    from: PathBuf,
    to: PathBuf,
}

#[derive(Debug, Clone)]
struct ValidatedUndoLog {
    operations: Vec<RenameOperation>,
    jpg_roots: Vec<PathBuf>,
    backup_paths: Vec<PathBuf>,
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
    let paths = app_paths()?;
    apply_plan_with_options_with_paths(plan, options, &paths)
}

fn apply_plan_with_options_with_paths(
    plan: &RenamePlan,
    options: &ApplyOptions,
    paths: &AppPaths,
) -> Result<ApplyResult> {
    let candidates: Vec<&RenameCandidate> = plan.candidates.iter().filter(|c| c.changed).collect();
    if candidates.is_empty() {
        return Ok(ApplyResult {
            applied: 0,
            unchanged: plan.candidates.len(),
        });
    }

    validate_apply_candidates(plan, &candidates)?;

    let backup_paths = if options.backup_originals {
        backup_original_files(plan, &candidates)?
    } else {
        Vec::new()
    };

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

    if let Err(persist_err) = persist_undo(&operations, plan, options, &backup_paths, paths) {
        let rollback_result = rollback_after_undo_persist_failure(&operations);
        let backup_cleanup_result =
            cleanup_created_backups_after_persist_failure(plan, &backup_paths);
        return Err(compose_persist_failure_error(
            persist_err,
            rollback_result,
            backup_cleanup_result,
        ));
    }

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

fn plan_jpg_roots(plan: &RenamePlan) -> Vec<PathBuf> {
    if plan.jpg_roots.is_empty() {
        return vec![plan.jpg_root.clone()];
    }
    plan.jpg_roots.clone()
}

fn canonicalize_jpg_roots(raw_roots: &[PathBuf]) -> Result<Vec<PathBuf>> {
    if raw_roots.is_empty() {
        bail!("JPGルートが指定されていません");
    }

    let mut seen = HashSet::<PathBuf>::new();
    let mut out = Vec::<PathBuf>::new();
    for root in raw_roots {
        let canonical = fs::canonicalize(root)
            .with_context(|| format!("JPGルートを解決できませんでした: {}", root.display()))?;
        if !canonical.is_dir() {
            bail!("JPGルートがフォルダではありません: {}", canonical.display());
        }
        if seen.insert(canonical.clone()) {
            out.push(canonical);
        }
    }
    Ok(out)
}

fn path_within_any_root(path: &Path, roots: &[PathBuf]) -> bool {
    roots.iter().any(|root| path.starts_with(root))
}

fn pick_most_specific_root<'a>(path: &Path, roots: &'a [PathBuf]) -> Option<&'a PathBuf> {
    roots
        .iter()
        .filter(|root| path.starts_with(root))
        .max_by_key(|root| root.components().count())
}

fn validate_apply_candidates(plan: &RenamePlan, candidates: &[&RenameCandidate]) -> Result<()> {
    let jpg_roots = canonicalize_jpg_roots(&plan_jpg_roots(plan))?;
    let mut seen_original_paths = HashSet::<PathBuf>::new();
    let mut seen_target_paths = HashSet::<PathBuf>::new();

    for candidate in candidates {
        let original_canonical = fs::canonicalize(&candidate.original_path).with_context(|| {
            format!(
                "元ファイルを解決できませんでした: {}",
                candidate.original_path.display()
            )
        })?;
        if !path_within_any_root(&original_canonical, &jpg_roots) {
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
        if !path_within_any_root(&target_parent_canonical, &jpg_roots) {
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

fn rollback_after_undo_persist_failure(operations: &[RenameOperation]) -> Result<()> {
    for operation in operations.iter().rev() {
        if !operation.to.exists() {
            continue;
        }
        fs::rename(&operation.to, &operation.from).with_context(|| {
            format!(
                "取り消しログ保存失敗後のロールバックに失敗しました: {} -> {}",
                operation.to.display(),
                operation.from.display()
            )
        })?;
    }
    Ok(())
}

fn cleanup_created_backups_after_persist_failure(
    plan: &RenamePlan,
    backup_paths: &[PathBuf],
) -> Result<()> {
    if backup_paths.is_empty() {
        return Ok(());
    }

    let validated = ValidatedUndoLog {
        operations: Vec::new(),
        jpg_roots: plan_jpg_roots(plan),
        backup_paths: backup_paths.to_vec(),
    };
    cleanup_backup_if_needed(&validated)
}

fn compose_persist_failure_error(
    persist_err: anyhow::Error,
    rollback_result: Result<()>,
    backup_cleanup_result: Result<()>,
) -> anyhow::Error {
    match (rollback_result, backup_cleanup_result) {
        (Ok(()), Ok(())) => persist_err
            .context("取り消しログの保存に失敗したため、適用した変更をロールバックしました"),
        (Ok(()), Err(backup_cleanup_err)) => persist_err.context(format!(
            "取り消しログの保存に失敗したため、適用した変更をロールバックしましたがバックアップ掃除に失敗しました: {backup_cleanup_err}"
        )),
        (Err(rollback_err), Ok(())) => persist_err.context(format!(
            "取り消しログの保存に失敗し、適用済み変更のロールバックにも失敗しました: {rollback_err}"
        )),
        (Err(rollback_err), Err(backup_cleanup_err)) => persist_err.context(format!(
            "取り消しログの保存に失敗し、適用済み変更のロールバックにも失敗しました: {rollback_err}; バックアップ掃除にも失敗しました: {backup_cleanup_err}"
        )),
    }
}

fn backup_original_files(
    plan: &RenamePlan,
    candidates: &[&RenameCandidate],
) -> Result<Vec<PathBuf>> {
    let jpg_roots = canonicalize_jpg_roots(&plan_jpg_roots(plan))?;
    let mut backup_roots = Vec::<(PathBuf, PathBuf)>::new();
    for jpg_root in &jpg_roots {
        let backup_root = jpg_root.join("backup");
        fs::create_dir_all(&backup_root).with_context(|| {
            format!(
                "バックアップフォルダを作成できませんでした: {}",
                backup_root.display()
            )
        })?;
        let backup_root_canonical = fs::canonicalize(&backup_root).with_context(|| {
            format!(
                "バックアップフォルダを解決できませんでした: {}",
                backup_root.display()
            )
        })?;
        if !backup_root_canonical.starts_with(jpg_root) {
            bail!(
                "バックアップフォルダがJPGフォルダ外を指しています: {}",
                backup_root.display()
            );
        }
        backup_roots.push((jpg_root.clone(), backup_root_canonical));
    }

    let mut reserved_paths = HashSet::<PathBuf>::new();
    let mut backup_jobs = Vec::<(PathBuf, PathBuf)>::with_capacity(candidates.len());
    for candidate in candidates {
        let original_canonical = fs::canonicalize(&candidate.original_path).with_context(|| {
            format!(
                "元ファイルを解決できませんでした: {}",
                candidate.original_path.display()
            )
        })?;
        let Some(root) = backup_roots
            .iter()
            .filter(|(jpg_root, _)| original_canonical.starts_with(jpg_root))
            .max_by_key(|(jpg_root, _)| jpg_root.components().count())
        else {
            bail!(
                "バックアップ対象がJPGルート外です: {}",
                candidate.original_path.display()
            );
        };
        let backup_path = resolve_backup_path_with_reserved(
            &root.1,
            &root.0,
            &original_canonical,
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

    Ok(backup_jobs
        .into_iter()
        .map(|(_, backup_path)| backup_path)
        .collect())
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
    let validated = validate_undo_log(&log)?;

    let restored = restore_operations(&validated.operations)?;

    cleanup_backup_if_needed(&validated)?;

    fs::remove_file(&paths.undo_path).with_context(|| {
        format!(
            "取り消しログ削除に失敗しました: {}",
            paths.undo_path.display()
        )
    })?;

    Ok(UndoResult { restored })
}

fn validate_undo_log(log: &UndoLog) -> Result<ValidatedUndoLog> {
    let raw_jpg_roots = if !log.jpg_roots.is_empty() {
        log.jpg_roots.clone()
    } else if let Some(jpg_root) = log.jpg_root.as_ref() {
        vec![jpg_root.clone()]
    } else {
        bail!("取り消しログにJPGルートが記録されていません");
    };
    let jpg_roots = canonicalize_jpg_roots(&raw_jpg_roots)?;

    let mut seen_from = HashSet::<PathBuf>::new();
    let mut seen_to = HashSet::<PathBuf>::new();
    let mut operations = Vec::<RenameOperation>::with_capacity(log.operations.len());
    for operation in &log.operations {
        let normalized_from =
            normalize_path_within_roots(&operation.from, &jpg_roots, "取り消し元パス")?;
        let normalized_to =
            normalize_path_within_roots(&operation.to, &jpg_roots, "取り消し先パス")?;

        if !seen_from.insert(normalized_from.clone()) {
            bail!(
                "取り消しログに重複した取り消し元パスがあります: {}",
                normalized_from.display()
            );
        }
        if !seen_to.insert(normalized_to.clone()) {
            bail!(
                "取り消しログに重複した取り消し先パスがあります: {}",
                normalized_to.display()
            );
        }

        operations.push(RenameOperation {
            from: normalized_from,
            to: normalized_to,
        });
    }

    if !log.backup_originals {
        return Ok(ValidatedUndoLog {
            operations,
            jpg_roots,
            backup_paths: Vec::new(),
        });
    }

    let backup_roots: Vec<PathBuf> = jpg_roots.iter().map(|root| root.join("backup")).collect();

    let mut backup_paths = Vec::<PathBuf>::new();
    for backup_path in &log.backup_paths {
        if !backup_path.exists() {
            continue;
        }
        if backup_path.is_dir() {
            bail!(
                "取り消しログのバックアップパスがディレクトリです: {}",
                backup_path.display()
            );
        }
        backup_paths.push(normalize_path_within_roots(
            backup_path,
            &backup_roots,
            "バックアップパス",
        )?);
    }

    Ok(ValidatedUndoLog {
        operations,
        jpg_roots,
        backup_paths,
    })
}

fn normalize_path_within_roots(path: &Path, roots: &[PathBuf], label: &str) -> Result<PathBuf> {
    let parent = path
        .parent()
        .with_context(|| format!("{label}に親ディレクトリがありません: {}", path.display()))?;
    let file_name = path
        .file_name()
        .with_context(|| format!("{label}のファイル名が不正です: {}", path.display()))?;
    let canonical_parent = fs::canonicalize(parent).with_context(|| {
        format!(
            "{label}の親ディレクトリを解決できませんでした: {}",
            parent.display()
        )
    })?;
    let Some(root) = pick_most_specific_root(&canonical_parent, roots) else {
        bail!("{label}が許可範囲外です: {}", path.display());
    };
    if !canonical_parent.starts_with(root) {
        bail!("{label}が許可範囲外です: {}", path.display());
    };
    Ok(canonical_parent.join(file_name))
}

fn restore_operations(operations: &[RenameOperation]) -> Result<usize> {
    let mut restored = 0usize;
    for op in operations.iter().rev() {
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
    backup_paths: &[PathBuf],
    paths: &AppPaths,
) -> Result<()> {
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
        jpg_roots: plan_jpg_roots(plan),
        backup_paths: backup_paths.to_vec(),
    };
    let body =
        serde_json::to_string_pretty(&log).context("取り消しログのシリアライズに失敗しました")?;
    write_file_atomically(&paths.undo_path, &body, "取り消しログ")?;
    Ok(())
}

fn write_file_atomically(target_path: &Path, body: &str, label: &str) -> Result<()> {
    let file_name = target_path
        .file_name()
        .and_then(|v| v.to_str())
        .unwrap_or("state");
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

fn cleanup_backup_if_needed(log: &ValidatedUndoLog) -> Result<()> {
    if log.backup_paths.is_empty() {
        return Ok(());
    }

    let backup_roots: Vec<PathBuf> = log
        .jpg_roots
        .iter()
        .map(|root| root.join("backup"))
        .collect();

    for backup_path in &log.backup_paths {
        if !backup_path.exists() {
            continue;
        }
        if backup_path.is_dir() {
            bail!(
                "取り消しログのバックアップパスがディレクトリです: {}",
                backup_path.display()
            );
        }
        fs::remove_file(backup_path).with_context(|| {
            format!(
                "バックアップファイル削除に失敗しました: {}",
                backup_path.display()
            )
        })?;
        if let Some(parent) = backup_path.parent() {
            if let Some(backup_root) = pick_most_specific_root(parent, &backup_roots) {
                remove_empty_dirs_until(parent, backup_root)?;
            }
        }
    }

    for backup_root in backup_roots {
        if backup_root.exists() && backup_root.is_dir() && directory_is_empty(&backup_root)? {
            fs::remove_dir(&backup_root).with_context(|| {
                format!(
                    "バックアップフォルダ削除に失敗しました: {}",
                    backup_root.display()
                )
            })?;
        }
    }

    Ok(())
}

fn directory_is_empty(path: &Path) -> Result<bool> {
    let mut entries = fs::read_dir(path)
        .with_context(|| format!("ディレクトリを読めませんでした: {}", path.display()))?;
    Ok(entries.next().is_none())
}

fn remove_empty_dirs_until(start: &Path, stop: &Path) -> Result<()> {
    let mut current = Some(start.to_path_buf());
    while let Some(dir) = current {
        if dir == stop || !dir.starts_with(stop) {
            break;
        }
        if !dir.exists() || !dir.is_dir() || !directory_is_empty(&dir)? {
            break;
        }
        fs::remove_dir(&dir)
            .with_context(|| format!("空ディレクトリ削除に失敗しました: {}", dir.display()))?;
        current = dir.parent().map(PathBuf::from);
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
    #[cfg(unix)]
    use super::backup_original_files;
    use super::{
        apply_plan_with_options, apply_plan_with_options_with_paths, cleanup_backup_if_needed,
        resolve_backup_path, resolve_backup_path_with_reserved, restore_operations,
        unique_backup_path, validate_undo_log, ApplyOptions, UndoLog,
    };
    use crate::config::AppPaths;
    use crate::metadata::{MetadataSource, PhotoMetadata};
    use crate::planner::{RenameCandidate, RenamePlan, RenameStats};
    use chrono::Local;
    use std::collections::HashSet;
    use std::fs;
    #[cfg(unix)]
    use std::os::unix::fs as unix_fs;
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
            jpg_roots: vec![jpg_root.clone()],
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
    fn apply_plan_with_multiple_jpg_roots_succeeds() {
        let temp = tempdir().expect("tempdir");
        let root_a = temp.path().join("a");
        let root_b = temp.path().join("b");
        fs::create_dir_all(&root_a).expect("create root a");
        fs::create_dir_all(&root_b).expect("create root b");

        let original_a = root_a.join("IMG_A.JPG");
        let original_b = root_b.join("IMG_B.JPG");
        let target_a = root_a.join("IMG_A_NEW.JPG");
        let target_b = root_b.join("IMG_B_NEW.JPG");
        fs::write(&original_a, b"A").expect("write A");
        fs::write(&original_b, b"B").expect("write B");

        let plan = RenamePlan {
            jpg_root: temp.path().to_path_buf(),
            jpg_roots: vec![root_a.clone(), root_b.clone()],
            template: "{orig_name}".to_string(),
            exclusions: Vec::new(),
            candidates: vec![
                RenameCandidate {
                    original_path: original_a.clone(),
                    target_path: target_a.clone(),
                    metadata_source: MetadataSource::JpgExif,
                    source_label: "jpg".to_string(),
                    metadata: sample_metadata(original_a.clone()),
                    rendered_base: "IMG_A_NEW".to_string(),
                    changed: true,
                },
                RenameCandidate {
                    original_path: original_b.clone(),
                    target_path: target_b.clone(),
                    metadata_source: MetadataSource::JpgExif,
                    source_label: "jpg".to_string(),
                    metadata: sample_metadata(original_b.clone()),
                    rendered_base: "IMG_B_NEW".to_string(),
                    changed: true,
                },
            ],
            stats: RenameStats::default(),
        };

        let paths = AppPaths {
            config_dir: temp.path().join("config"),
            config_path: temp.path().join("config/config.toml"),
            undo_path: temp.path().join("config/undo-last.json"),
        };
        let result = apply_plan_with_options_with_paths(&plan, &ApplyOptions::default(), &paths)
            .expect("apply should succeed for multi roots");

        assert_eq!(result.applied, 2);
        assert!(target_a.exists());
        assert!(target_b.exists());
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
        let backup_file = backup_root.join("file.txt");
        fs::write(&backup_file, b"x").expect("create backup file");

        let log = UndoLog {
            operations: Vec::new(),
            backup_originals: true,
            jpg_root: Some(jpg_root.clone()),
            jpg_roots: Vec::new(),
            backup_paths: vec![backup_file],
        };
        let validated = validate_undo_log(&log).expect("undo log should be valid");
        cleanup_backup_if_needed(&validated).expect("cleanup should succeed");
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
            jpg_roots: Vec::new(),
            backup_paths: Vec::new(),
        };
        let validated = validate_undo_log(&log).expect("undo log should be valid");
        cleanup_backup_if_needed(&validated).expect("cleanup should succeed");
        assert!(backup_root.exists());
    }

    #[test]
    fn cleanup_backup_if_needed_keeps_untracked_backup_files() {
        let temp = tempdir().expect("tempdir");
        let jpg_root = temp.path().join("jpg");
        let backup_root = jpg_root.join("backup");
        fs::create_dir_all(&backup_root).expect("create backup root");
        let tracked = backup_root.join("tracked.txt");
        let keep = backup_root.join("keep.txt");
        fs::write(&tracked, b"x").expect("create tracked file");
        fs::write(&keep, b"x").expect("create keep file");

        let log = UndoLog {
            operations: Vec::new(),
            backup_originals: true,
            jpg_root: Some(jpg_root),
            jpg_roots: Vec::new(),
            backup_paths: vec![tracked.clone()],
        };
        let validated = validate_undo_log(&log).expect("undo log should be valid");
        cleanup_backup_if_needed(&validated).expect("cleanup should succeed");

        assert!(!tracked.exists());
        assert!(keep.exists());
        assert!(backup_root.exists());
    }

    #[test]
    fn cleanup_backup_if_needed_skips_when_backup_paths_missing_in_legacy_log() {
        let temp = tempdir().expect("tempdir");
        let jpg_root = temp.path().join("jpg");
        let backup_root = jpg_root.join("backup");
        fs::create_dir_all(&backup_root).expect("create backup root");
        let keep = backup_root.join("keep.txt");
        fs::write(&keep, b"x").expect("create keep file");

        let log = UndoLog {
            operations: Vec::new(),
            backup_originals: true,
            jpg_root: Some(jpg_root),
            jpg_roots: Vec::new(),
            backup_paths: Vec::new(),
        };
        let validated = validate_undo_log(&log).expect("undo log should be valid");
        cleanup_backup_if_needed(&validated).expect("cleanup should succeed");

        assert!(keep.exists());
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

    #[cfg(unix)]
    #[test]
    fn backup_original_files_rejects_backup_symlink_outside_jpg_root() {
        let temp = tempdir().expect("tempdir");
        let jpg_root = temp.path().join("jpg");
        let outside_root = temp.path().join("outside");
        fs::create_dir_all(&jpg_root).expect("create jpg root");
        fs::create_dir_all(&outside_root).expect("create outside root");

        let original = jpg_root.join("IMG_0001.JPG");
        fs::write(&original, b"x").expect("write original");
        let backup_link = jpg_root.join("backup");
        unix_fs::symlink(&outside_root, &backup_link).expect("create backup symlink");

        let candidate = RenameCandidate {
            original_path: original.clone(),
            target_path: jpg_root.join("IMG_0001_NEW.JPG"),
            metadata_source: MetadataSource::JpgExif,
            source_label: "jpg".to_string(),
            metadata: sample_metadata(original),
            rendered_base: "IMG_0001_NEW".to_string(),
            changed: true,
        };
        let plan = RenamePlan {
            jpg_root: jpg_root.clone(),
            jpg_roots: vec![jpg_root.clone()],
            template: "{orig_name}".to_string(),
            exclusions: Vec::new(),
            candidates: vec![candidate.clone()],
            stats: RenameStats::default(),
        };

        let err = backup_original_files(&plan, &[&candidate]).expect_err("symlink root must fail");
        assert!(err
            .to_string()
            .contains("バックアップフォルダがJPGフォルダ外を指しています"));
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
            jpg_roots: vec![jpg_root.clone()],
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
    fn apply_plan_rolls_back_when_undo_log_persist_fails() {
        let temp = tempdir().expect("tempdir");
        let jpg_root = temp.path().join("jpg");
        fs::create_dir_all(&jpg_root).expect("create jpg root");

        let original = jpg_root.join("IMG_0001.JPG");
        let renamed = jpg_root.join("RENAMED_0001.JPG");
        fs::write(&original, b"x").expect("write original");

        let plan = RenamePlan {
            jpg_root: jpg_root.clone(),
            jpg_roots: vec![jpg_root.clone()],
            template: "{orig_name}".to_string(),
            exclusions: Vec::new(),
            candidates: vec![RenameCandidate {
                original_path: original.clone(),
                target_path: renamed.clone(),
                metadata_source: MetadataSource::JpgExif,
                source_label: "jpg".to_string(),
                metadata: sample_metadata(original.clone()),
                rendered_base: "RENAMED_0001".to_string(),
                changed: true,
            }],
            stats: RenameStats::default(),
        };

        let blocked_config_dir = temp.path().join("blocked-config");
        fs::write(&blocked_config_dir, b"not-a-directory").expect("create blocked config path");
        let blocked_paths = AppPaths {
            config_dir: blocked_config_dir.clone(),
            config_path: blocked_config_dir.join("config.toml"),
            undo_path: blocked_config_dir.join("undo-last.json"),
        };

        let err = apply_plan_with_options_with_paths(
            &plan,
            &ApplyOptions {
                backup_originals: true,
            },
            &blocked_paths,
        )
        .expect_err("persist should fail");

        assert!(
            err.to_string().contains("取り消しログ"),
            "error should include undo persistence context: {err}"
        );
        assert!(original.exists(), "original should be restored");
        assert!(!renamed.exists(), "renamed file should be rolled back");
        assert!(
            !jpg_root.join("backup").exists(),
            "backup directory should be cleaned after rollback"
        );
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
            jpg_roots: vec![jpg_root.clone()],
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
            jpg_roots: vec![jpg_root.clone()],
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
            jpg_roots: Vec::new(),
            backup_paths: Vec::new(),
        };

        let restored = restore_operations(&log.operations).expect("restore should succeed");
        assert_eq!(restored, 1);
        assert!(from_a.exists());
        assert!(!to_a.exists());
        assert!(!from_b.exists());
    }

    #[test]
    fn validate_undo_log_rejects_operation_outside_jpg_root() {
        let temp = tempdir().expect("tempdir");
        let jpg_root = temp.path().join("jpg");
        let outside_root = temp.path().join("outside");
        fs::create_dir_all(&jpg_root).expect("create jpg root");
        fs::create_dir_all(&outside_root).expect("create outside root");

        let inside_from = jpg_root.join("IMG_0001.JPG");
        let outside_to = outside_root.join("RENAMED_0001.JPG");

        let log = UndoLog {
            operations: vec![super::RenameOperation {
                from: inside_from,
                to: outside_to,
            }],
            backup_originals: false,
            jpg_root: Some(jpg_root),
            jpg_roots: Vec::new(),
            backup_paths: Vec::new(),
        };

        let err = validate_undo_log(&log).expect_err("outside path must be rejected");
        assert!(err.to_string().contains("許可範囲外"));
    }
}
