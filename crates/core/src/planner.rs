use crate::exif_reader::read_exif_metadata;
use crate::matcher::{find_matching_raw, xmp_for_raw};
use crate::metadata::{MetadataSource, PartialMetadata, PhotoMetadata};
use crate::sanitize::{
    apply_exclusions, cleanup_filename, sanitize_filename, truncate_filename_if_needed,
};
use crate::template::{parse_template, render_template_with_options};
use crate::xmp_reader::read_xmp_metadata;
use anyhow::{Context, Result};
use chrono::{DateTime, Local};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

#[derive(Debug, Clone)]
pub struct PlanOptions {
    pub jpg_input: PathBuf,
    pub raw_input: Option<PathBuf>,
    pub recursive: bool,
    pub include_hidden: bool,
    pub template: String,
    pub dedupe_same_maker: bool,
    pub exclusions: Vec<String>,
    pub max_filename_len: usize,
}

impl Default for PlanOptions {
    fn default() -> Self {
        Self {
            jpg_input: PathBuf::new(),
            raw_input: None,
            recursive: false,
            include_hidden: false,
            template: "{year}{month}{day}{hour}{minute}{second}_{camera_make}_{camera_model}_{lens_make}_{lens_model}_{film_sim}_{orig_name}".to_string(),
            dedupe_same_maker: true,
            exclusions: Vec::new(),
            max_filename_len: 240,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RenameCandidate {
    pub original_path: PathBuf,
    pub target_path: PathBuf,
    pub metadata_source: MetadataSource,
    pub metadata: PhotoMetadata,
    pub rendered_base: String,
    pub changed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RenameStats {
    pub scanned_files: usize,
    pub jpg_files: usize,
    pub skipped_non_jpg: usize,
    pub skipped_hidden: usize,
    pub planned: usize,
    pub unchanged: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RenamePlan {
    pub template: String,
    pub exclusions: Vec<String>,
    pub candidates: Vec<RenameCandidate>,
    pub stats: RenameStats,
}

pub fn generate_plan(options: &PlanOptions) -> Result<RenamePlan> {
    if !options.jpg_input.exists() {
        anyhow::bail!("JPGフォルダが存在しません: {}", options.jpg_input.display());
    }

    let parts = parse_template(&options.template)?;
    let mut stats = RenameStats::default();
    let jpg_files = collect_jpg_files(
        &options.jpg_input,
        options.recursive,
        options.include_hidden,
        &mut stats,
    )?;

    let mut candidates = Vec::with_capacity(jpg_files.len());
    let mut planned_paths = HashSet::<PathBuf>::new();

    for jpg_path in jpg_files {
        let metadata = resolve_metadata(
            &options.jpg_input,
            options.raw_input.as_deref(),
            &jpg_path,
            options.recursive,
        )?;

        let rendered = render_template_with_options(&parts, &metadata, options.dedupe_same_maker);
        let excluded = apply_exclusions(rendered, &options.exclusions);
        let cleaned = cleanup_filename(&excluded);
        let sanitized = sanitize_filename(&cleaned);

        let extension = jpg_path
            .extension()
            .map(|v| format!(".{}", v.to_string_lossy()))
            .unwrap_or_default();

        let truncated =
            truncate_filename_if_needed(&sanitized, &extension, options.max_filename_len);
        let target = resolve_collision(
            &jpg_path,
            &truncated,
            &extension,
            &mut planned_paths,
            options.max_filename_len,
        )?;

        let changed = target != jpg_path;
        if !changed {
            stats.unchanged += 1;
        }

        stats.planned += 1;
        candidates.push(RenameCandidate {
            original_path: jpg_path,
            target_path: target,
            metadata_source: metadata.source,
            metadata,
            rendered_base: truncated,
            changed,
        });
    }

    Ok(RenamePlan {
        template: options.template.clone(),
        exclusions: options.exclusions.clone(),
        candidates,
        stats,
    })
}

pub fn render_preview_sample(
    template: &str,
    dedupe_same_maker: bool,
    exclusions: &[String],
    metadata: &PhotoMetadata,
    extension_with_dot: &str,
    max_filename_len: usize,
) -> Result<String> {
    let parts = parse_template(template)?;
    let rendered = render_template_with_options(&parts, metadata, dedupe_same_maker);
    let excluded = apply_exclusions(rendered, exclusions);
    let cleaned = cleanup_filename(&excluded);
    let sanitized = sanitize_filename(&cleaned);
    let truncated = truncate_filename_if_needed(&sanitized, extension_with_dot, max_filename_len);
    Ok(format!("{}{}", truncated, extension_with_dot))
}

fn collect_jpg_files(
    root: &Path,
    recursive: bool,
    include_hidden: bool,
    stats: &mut RenameStats,
) -> Result<Vec<PathBuf>> {
    let mut out = Vec::new();

    if recursive {
        for entry in WalkDir::new(root).sort_by_file_name() {
            let entry =
                entry.with_context(|| format!("フォルダ走査に失敗しました: {}", root.display()))?;
            let path = entry.path();
            if path.is_dir() {
                continue;
            }
            stats.scanned_files += 1;

            if is_hidden(path) && !include_hidden {
                stats.skipped_hidden += 1;
                continue;
            }

            if is_jpg(path) {
                stats.jpg_files += 1;
                out.push(path.to_path_buf());
            } else {
                stats.skipped_non_jpg += 1;
            }
        }
    } else {
        for entry in fs::read_dir(root)
            .with_context(|| format!("フォルダを読めませんでした: {}", root.display()))?
        {
            let entry =
                entry.with_context(|| format!("エントリ読み取り失敗: {}", root.display()))?;
            let path = entry.path();
            if path.is_dir() {
                continue;
            }
            stats.scanned_files += 1;
            if is_hidden(&path) && !include_hidden {
                stats.skipped_hidden += 1;
                continue;
            }
            if is_jpg(&path) {
                stats.jpg_files += 1;
                out.push(path);
            } else {
                stats.skipped_non_jpg += 1;
            }
        }
        out.sort();
    }

    Ok(out)
}

fn resolve_metadata(
    jpg_root: &Path,
    raw_root: Option<&Path>,
    jpg_path: &Path,
    recursive: bool,
) -> Result<PhotoMetadata> {
    let fallback_date = file_modified_to_local(jpg_path).unwrap_or_else(Local::now);
    let original_name = jpg_path
        .file_stem()
        .map(|v| v.to_string_lossy().to_string())
        .unwrap_or_else(|| "untitled".to_string());

    if let Some(raw_root) = raw_root {
        if let Some(raw_path) = find_matching_raw(jpg_root, raw_root, jpg_path, recursive) {
            let raw_exif = read_exif_metadata(&raw_path).ok();

            for xmp in xmp_for_raw(&raw_path) {
                if !xmp.exists() {
                    continue;
                }

                match read_xmp_metadata(&xmp) {
                    Ok(mut xmp_meta) => {
                        let mut source = MetadataSource::Xmp;
                        if let Some(raw) = raw_exif.as_ref() {
                            let before = xmp_meta.clone();
                            xmp_meta.merge_missing_from(raw);
                            if metadata_changed(&before, &xmp_meta) {
                                source = MetadataSource::XmpAndRawExif;
                            }
                        }

                        return Ok(to_photo_metadata(
                            xmp_meta,
                            source,
                            fallback_date,
                            original_name,
                            jpg_path,
                        ));
                    }
                    Err(_) => {
                        if let Some(raw) = raw_exif.as_ref() {
                            return Ok(to_photo_metadata(
                                raw.clone(),
                                MetadataSource::RawExif,
                                fallback_date,
                                original_name,
                                jpg_path,
                            ));
                        }
                    }
                }
            }

            if let Some(raw) = raw_exif {
                return Ok(to_photo_metadata(
                    raw,
                    MetadataSource::RawExif,
                    fallback_date,
                    original_name,
                    jpg_path,
                ));
            }
        }
    }

    let jpg_meta = read_exif_metadata(jpg_path).unwrap_or_default();
    Ok(to_photo_metadata(
        jpg_meta,
        MetadataSource::JpgExif,
        fallback_date,
        original_name,
        jpg_path,
    ))
}

fn to_photo_metadata(
    partial: PartialMetadata,
    source: MetadataSource,
    fallback_date: DateTime<Local>,
    original_name: String,
    jpg_path: &Path,
) -> PhotoMetadata {
    let source = if partial.date.is_none() {
        MetadataSource::FallbackFileModified
    } else {
        source
    };

    PhotoMetadata {
        source,
        date: partial.date.unwrap_or(fallback_date),
        camera_make: partial.camera_make,
        camera_model: partial.camera_model,
        lens_make: partial.lens_make,
        lens_model: partial.lens_model,
        film_sim: partial.film_sim,
        original_name,
        jpg_path: jpg_path.to_path_buf(),
    }
}

fn metadata_changed(a: &PartialMetadata, b: &PartialMetadata) -> bool {
    a.date != b.date
        || a.camera_make != b.camera_make
        || a.camera_model != b.camera_model
        || a.lens_make != b.lens_make
        || a.lens_model != b.lens_model
        || a.film_sim != b.film_sim
}

fn resolve_collision(
    original_path: &Path,
    base: &str,
    extension: &str,
    planned_paths: &mut HashSet<PathBuf>,
    max_len: usize,
) -> Result<PathBuf> {
    let parent = original_path
        .parent()
        .context("親ディレクトリを取得できませんでした")?;

    let mut candidate = parent.join(format!("{}{}", base, extension));
    if is_available(&candidate, original_path, planned_paths) {
        planned_paths.insert(candidate.clone());
        return Ok(candidate);
    }

    let mut n = 1usize;
    loop {
        let suffix = format!("_{:03}", n);
        let base = truncate_filename_if_needed(&(base.to_string() + &suffix), extension, max_len);
        candidate = parent.join(format!("{}{}", base, extension));
        if is_available(&candidate, original_path, planned_paths) {
            planned_paths.insert(candidate.clone());
            return Ok(candidate);
        }
        n += 1;
    }
}

fn is_available(candidate: &Path, original_path: &Path, planned_paths: &HashSet<PathBuf>) -> bool {
    if planned_paths.contains(candidate) {
        return false;
    }
    if candidate == original_path {
        return true;
    }
    !candidate.exists()
}

fn is_jpg(path: &Path) -> bool {
    path.extension()
        .map(|ext| {
            let ext = ext.to_string_lossy();
            ext.eq_ignore_ascii_case("jpg") || ext.eq_ignore_ascii_case("jpeg")
        })
        .unwrap_or(false)
}

fn is_hidden(path: &Path) -> bool {
    path.file_name()
        .map(|name| name.to_string_lossy().starts_with('.'))
        .unwrap_or(false)
}

fn file_modified_to_local(path: &Path) -> Option<DateTime<Local>> {
    let time = fs::metadata(path).ok()?.modified().ok()?;
    Some(DateTime::from(time))
}
