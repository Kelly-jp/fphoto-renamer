use crate::exif_reader::read_exif_metadata;
use crate::matcher::{build_raw_match_index, find_matching_raw, find_matching_xmp, RawMatchIndex};
use crate::metadata::{MetadataSource, PartialMetadata, PhotoMetadata};
use crate::sanitize::{
    apply_exclusions, cleanup_filename, normalize_spaces_to_underscore, sanitize_filename,
    truncate_filename_if_needed,
};
use crate::template::{parse_template, render_template_with_options, TemplatePart};
use crate::xmp_reader::read_xmp_metadata;
use crate::DEFAULT_TEMPLATE;
use anyhow::{Context, Result};
use chrono::{DateTime, Local};
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

#[derive(Debug, Clone)]
pub struct PlanOptions {
    pub jpg_input: PathBuf,
    pub raw_input: Option<PathBuf>,
    pub raw_from_jpg_parent_when_missing: bool,
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
            raw_from_jpg_parent_when_missing: false,
            recursive: false,
            include_hidden: false,
            template: DEFAULT_TEMPLATE.to_string(),
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
    #[serde(default = "default_source_label")]
    pub source_label: String,
    pub metadata: PhotoMetadata,
    pub rendered_base: String,
    pub changed: bool,
}

fn default_source_label() -> String {
    "jpg".to_string()
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
    pub jpg_root: PathBuf,
    pub template: String,
    pub exclusions: Vec<String>,
    pub candidates: Vec<RenameCandidate>,
    pub stats: RenameStats,
}

#[derive(Debug)]
struct PreparedCandidate {
    original_path: PathBuf,
    metadata: PhotoMetadata,
    source_label: String,
    rendered_base: String,
    extension: String,
}

#[derive(Debug)]
struct ResolvedMetadata {
    metadata: PhotoMetadata,
    source_label: String,
}

struct PrepareContext<'a> {
    jpg_root: &'a Path,
    raw_root: Option<&'a Path>,
    raw_match_index: Option<&'a RawMatchIndex>,
    recursive: bool,
    parts: &'a [TemplatePart],
    dedupe_same_maker: bool,
    exclusions: &'a [String],
    max_filename_len: usize,
}

pub fn generate_plan(options: &PlanOptions) -> Result<RenamePlan> {
    if !options.jpg_input.exists() {
        anyhow::bail!("JPGフォルダが存在しません: {}", options.jpg_input.display());
    }
    if !options.jpg_input.is_dir() {
        anyhow::bail!("JPGフォルダではありません: {}", options.jpg_input.display());
    }
    if let Some(raw_input) = options.raw_input.as_ref() {
        if !raw_input.exists() {
            anyhow::bail!("RAWフォルダが存在しません: {}", raw_input.display());
        }
        if !raw_input.is_dir() {
            anyhow::bail!("RAWフォルダではありません: {}", raw_input.display());
        }
    }

    let parts = parse_template(&options.template)?;
    let effective_raw_input = resolve_effective_raw_input(options);
    let raw_match_index = effective_raw_input
        .as_deref()
        .map(|raw_root| build_raw_match_index(&options.jpg_input, raw_root, options.recursive));
    let mut stats = RenameStats::default();
    let jpg_files = collect_jpg_files(
        &options.jpg_input,
        options.recursive,
        options.include_hidden,
        &mut stats,
    )?;

    let prepare_context = PrepareContext {
        jpg_root: &options.jpg_input,
        raw_root: effective_raw_input.as_deref(),
        raw_match_index: raw_match_index.as_ref(),
        recursive: options.recursive,
        parts: &parts,
        dedupe_same_maker: options.dedupe_same_maker,
        exclusions: &options.exclusions,
        max_filename_len: options.max_filename_len,
    };
    let prepared_results: Vec<Result<PreparedCandidate>> = jpg_files
        .par_iter()
        .map(|jpg_path| prepare_candidate(&prepare_context, jpg_path))
        .collect();

    let mut prepared = Vec::with_capacity(prepared_results.len());
    for result in prepared_results {
        prepared.push(result?);
    }

    let mut candidates = Vec::with_capacity(prepared.len());
    let mut planned_paths = HashSet::<PathBuf>::new();
    for prepared in prepared {
        let target = resolve_collision(
            &prepared.original_path,
            &prepared.rendered_base,
            &prepared.extension,
            &mut planned_paths,
            options.max_filename_len,
        )?;

        let changed = target != prepared.original_path;
        if !changed {
            stats.unchanged += 1;
        }

        stats.planned += 1;
        candidates.push(RenameCandidate {
            original_path: prepared.original_path,
            target_path: target,
            metadata_source: prepared.metadata.source,
            source_label: prepared.source_label,
            metadata: prepared.metadata,
            rendered_base: prepared.rendered_base,
            changed,
        });
    }

    Ok(RenamePlan {
        jpg_root: options.jpg_input.clone(),
        template: options.template.clone(),
        exclusions: options.exclusions.clone(),
        candidates,
        stats,
    })
}

fn prepare_candidate(context: &PrepareContext<'_>, jpg_path: &Path) -> Result<PreparedCandidate> {
    let resolved = resolve_metadata(
        context.jpg_root,
        context.raw_root,
        context.raw_match_index,
        jpg_path,
        context.recursive,
    )?;
    let rendered =
        render_template_with_options(context.parts, &resolved.metadata, context.dedupe_same_maker);
    let excluded = apply_exclusions(rendered, context.exclusions);
    let normalized_spaces = normalize_spaces_to_underscore(&excluded);
    let cleaned = cleanup_filename(&normalized_spaces);
    let sanitized = sanitize_filename(&cleaned);

    let extension = jpg_path
        .extension()
        .map(|v| format!(".{}", v.to_string_lossy()))
        .unwrap_or_default();
    let rendered_base =
        truncate_filename_if_needed(&sanitized, &extension, context.max_filename_len);

    Ok(PreparedCandidate {
        original_path: jpg_path.to_path_buf(),
        metadata: resolved.metadata,
        source_label: resolved.source_label,
        rendered_base,
        extension,
    })
}

fn resolve_effective_raw_input(options: &PlanOptions) -> Option<PathBuf> {
    if let Some(raw_input) = options.raw_input.as_ref() {
        return Some(raw_input.clone());
    }

    if !options.raw_from_jpg_parent_when_missing {
        return None;
    }

    options.jpg_input.parent().map(PathBuf::from)
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
    let normalized_spaces = normalize_spaces_to_underscore(&excluded);
    let cleaned = cleanup_filename(&normalized_spaces);
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
        let mut walker = WalkDir::new(root).sort_by_file_name().into_iter();
        while let Some(entry) = walker.next() {
            let entry =
                entry.with_context(|| format!("フォルダ走査に失敗しました: {}", root.display()))?;
            let path = entry.path();
            if path.is_dir() {
                if entry.depth() > 0 && !include_hidden && is_hidden(path) {
                    stats.skipped_hidden += 1;
                    walker.skip_current_dir();
                }
                continue;
            }
            if is_hidden(path) && !include_hidden {
                stats.skipped_hidden += 1;
                continue;
            }
            stats.scanned_files += 1;

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
            if is_hidden(&path) && !include_hidden {
                stats.skipped_hidden += 1;
                continue;
            }
            stats.scanned_files += 1;
            if is_jpg(&path) {
                stats.jpg_files += 1;
                out.push(path);
            } else {
                stats.skipped_non_jpg += 1;
            }
        }
    }

    out.sort();

    Ok(out)
}

fn resolve_metadata(
    jpg_root: &Path,
    raw_root: Option<&Path>,
    raw_match_index: Option<&RawMatchIndex>,
    jpg_path: &Path,
    recursive: bool,
) -> Result<ResolvedMetadata> {
    let fallback_date = file_modified_to_local(jpg_path).unwrap_or_else(Local::now);
    let original_name = jpg_path
        .file_stem()
        .map(|v| v.to_string_lossy().to_string())
        .unwrap_or_else(|| "untitled".to_string());
    let mut jpg_exif_meta_cache: Option<PartialMetadata> = None;
    let mut jpg_exif_loaded = false;

    let mut load_jpg_exif_meta = || {
        if !jpg_exif_loaded {
            jpg_exif_meta_cache = read_exif_metadata(jpg_path).ok();
            jpg_exif_loaded = true;
        }
    };

    if let Some(raw_root) = raw_root {
        let (xmp_path, raw_path) = if let Some(index) = raw_match_index {
            (index.find_xmp(jpg_path), index.find_raw(jpg_path))
        } else {
            (
                find_matching_xmp(jpg_root, raw_root, jpg_path, recursive),
                find_matching_raw(jpg_root, raw_root, jpg_path, recursive),
            )
        };
        let mut raw_exif_cache: Option<PartialMetadata> = None;
        let mut raw_exif_loaded = false;
        let mut load_raw_exif_meta = || -> Option<PartialMetadata> {
            if !raw_exif_loaded {
                raw_exif_cache = raw_path
                    .as_ref()
                    .and_then(|path| read_exif_metadata(path).ok());
                raw_exif_loaded = true;
            }
            raw_exif_cache.clone()
        };

        if let Some(xmp_path) = xmp_path {
            match read_xmp_metadata(&xmp_path) {
                Ok(mut xmp_meta) => {
                    let mut source = MetadataSource::Xmp;
                    if metadata_has_missing_fields(&xmp_meta) {
                        if let Some(raw) = load_raw_exif_meta().as_ref() {
                            let before = xmp_meta.clone();
                            xmp_meta.merge_missing_from(raw);
                            if metadata_changed(&before, &xmp_meta) {
                                source = MetadataSource::XmpAndRawExif;
                            }
                        }
                    }

                    let merged = if metadata_has_missing_fields(&xmp_meta) {
                        load_jpg_exif_meta();
                        merge_with_jpg_fallback(xmp_meta, jpg_exif_meta_cache.as_ref())
                    } else {
                        xmp_meta
                    };
                    let metadata =
                        to_photo_metadata(merged, source, fallback_date, original_name, jpg_path);
                    return Ok(ResolvedMetadata {
                        source_label: metadata_source_label(metadata.source, raw_path.as_deref()),
                        metadata,
                    });
                }
                Err(_) => {
                    if let Some(raw) = load_raw_exif_meta() {
                        let merged = if metadata_has_missing_fields(&raw) {
                            load_jpg_exif_meta();
                            merge_with_jpg_fallback(raw, jpg_exif_meta_cache.as_ref())
                        } else {
                            raw
                        };
                        let metadata = to_photo_metadata(
                            merged,
                            MetadataSource::RawExif,
                            fallback_date,
                            original_name,
                            jpg_path,
                        );
                        return Ok(ResolvedMetadata {
                            source_label: metadata_source_label(
                                metadata.source,
                                raw_path.as_deref(),
                            ),
                            metadata,
                        });
                    }
                }
            }
        }

        if let Some(raw) = load_raw_exif_meta() {
            let merged = if metadata_has_missing_fields(&raw) {
                load_jpg_exif_meta();
                merge_with_jpg_fallback(raw, jpg_exif_meta_cache.as_ref())
            } else {
                raw
            };
            let metadata = to_photo_metadata(
                merged,
                MetadataSource::RawExif,
                fallback_date,
                original_name,
                jpg_path,
            );
            return Ok(ResolvedMetadata {
                source_label: metadata_source_label(metadata.source, raw_path.as_deref()),
                metadata,
            });
        }
    }

    load_jpg_exif_meta();
    let jpg_meta = jpg_exif_meta_cache.unwrap_or_default();
    let metadata = to_photo_metadata(
        jpg_meta,
        MetadataSource::JpgExif,
        fallback_date,
        original_name,
        jpg_path,
    );
    Ok(ResolvedMetadata {
        source_label: metadata_source_label(metadata.source, None),
        metadata,
    })
}

fn metadata_source_label(source: MetadataSource, raw_path: Option<&Path>) -> String {
    match source {
        MetadataSource::Xmp | MetadataSource::XmpAndRawExif => "xmp".to_string(),
        MetadataSource::RawExif => raw_path
            .and_then(|path| path.extension().and_then(|v| v.to_str()))
            .map(|ext| ext.trim().to_ascii_lowercase())
            .filter(|ext| !ext.is_empty())
            .unwrap_or_else(|| "raw".to_string()),
        MetadataSource::JpgExif | MetadataSource::FallbackFileModified => "jpg".to_string(),
    }
}

fn metadata_has_missing_fields(meta: &PartialMetadata) -> bool {
    meta.date.is_none()
        || meta.camera_make.is_none()
        || meta.camera_model.is_none()
        || meta.lens_make.is_none()
        || meta.lens_model.is_none()
        || meta.film_sim.is_none()
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

fn merge_with_jpg_fallback(
    mut base: PartialMetadata,
    jpg_exif_meta: Option<&PartialMetadata>,
) -> PartialMetadata {
    if let Some(jpg_meta) = jpg_exif_meta {
        base.merge_missing_from(jpg_meta);
    }
    base
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

#[cfg(test)]
mod tests {
    use super::{generate_plan, merge_with_jpg_fallback, metadata_source_label, PlanOptions};
    use crate::metadata::{MetadataSource, PartialMetadata};
    use std::fs;
    use std::path::PathBuf;
    use tempfile::tempdir;

    #[test]
    fn merge_with_jpg_fallback_fills_missing_fields() {
        let base = PartialMetadata {
            camera_make: None,
            camera_model: None,
            lens_make: None,
            lens_model: None,
            ..Default::default()
        };
        let jpg = PartialMetadata {
            camera_make: Some("FUJIFILM".to_string()),
            camera_model: Some("X-H2".to_string()),
            lens_make: Some("FUJIFILM".to_string()),
            lens_model: Some("XF35mm F1.4 R".to_string()),
            ..Default::default()
        };

        let merged = merge_with_jpg_fallback(base, Some(&jpg));
        assert_eq!(merged.camera_make.as_deref(), Some("FUJIFILM"));
        assert_eq!(merged.camera_model.as_deref(), Some("X-H2"));
        assert_eq!(merged.lens_make.as_deref(), Some("FUJIFILM"));
        assert_eq!(merged.lens_model.as_deref(), Some("XF35mm F1.4 R"));
    }

    #[test]
    fn merge_with_jpg_fallback_keeps_existing_values() {
        let base = PartialMetadata {
            camera_make: Some("SONY".to_string()),
            camera_model: Some("A7C".to_string()),
            lens_make: Some("SIGMA".to_string()),
            lens_model: Some("35mm F2 DG DN".to_string()),
            ..Default::default()
        };
        let jpg = PartialMetadata {
            camera_make: Some("FUJIFILM".to_string()),
            camera_model: Some("X-H2".to_string()),
            lens_make: Some("FUJIFILM".to_string()),
            lens_model: Some("XF35mm F1.4 R".to_string()),
            ..Default::default()
        };

        let merged = merge_with_jpg_fallback(base, Some(&jpg));
        assert_eq!(merged.camera_make.as_deref(), Some("SONY"));
        assert_eq!(merged.camera_model.as_deref(), Some("A7C"));
        assert_eq!(merged.lens_make.as_deref(), Some("SIGMA"));
        assert_eq!(merged.lens_model.as_deref(), Some("35mm F2 DG DN"));
    }

    #[test]
    fn generate_plan_uses_xmp_when_only_xmp_exists_in_raw_folder() {
        let temp = tempdir().expect("tempdir");
        let jpg_root = temp.path().join("jpg");
        let raw_root = temp.path().join("raw");
        fs::create_dir_all(&jpg_root).expect("jpg root");
        fs::create_dir_all(&raw_root).expect("raw root");

        let jpg_path = jpg_root.join("DSC00001.JPG");
        fs::write(&jpg_path, b"not-a-real-jpg").expect("jpg file");

        let xmp = raw_root.join("DSC00001.xmp");
        fs::write(
            &xmp,
            r#"<x:xmpmeta><rdf:RDF><rdf:Description><exif:DateTimeOriginal>2026:02:08 10:20:30</exif:DateTimeOriginal><exif:Make>FUJIFILM</exif:Make></rdf:Description></rdf:RDF></x:xmpmeta>"#,
        )
        .expect("xmp file");

        let plan = generate_plan(&PlanOptions {
            jpg_input: jpg_root,
            raw_input: Some(raw_root),
            raw_from_jpg_parent_when_missing: false,
            recursive: false,
            include_hidden: false,
            template: "{camera_maker}_{orig_name}".to_string(),
            dedupe_same_maker: true,
            exclusions: Vec::new(),
            max_filename_len: 240,
        })
        .expect("plan generation should succeed");

        assert_eq!(plan.candidates.len(), 1);
        let c = &plan.candidates[0];
        assert_eq!(c.metadata_source, MetadataSource::Xmp);
        assert_eq!(c.source_label, "xmp");
        assert_eq!(c.metadata.camera_make.as_deref(), Some("FUJIFILM"));
    }

    #[test]
    fn generate_plan_fails_when_explicit_raw_folder_is_missing() {
        let temp = tempdir().expect("tempdir");
        let jpg_root = temp.path().join("jpg");
        fs::create_dir_all(&jpg_root).expect("jpg root");

        let jpg_path = jpg_root.join("DSC00099.JPG");
        fs::write(&jpg_path, b"not-a-real-jpg").expect("jpg file");

        let missing_raw_root = temp.path().join("missing-raw");
        let result = generate_plan(&PlanOptions {
            jpg_input: jpg_root,
            raw_input: Some(missing_raw_root.clone()),
            raw_from_jpg_parent_when_missing: false,
            recursive: false,
            include_hidden: false,
            template: "{orig_name}".to_string(),
            dedupe_same_maker: true,
            exclusions: Vec::new(),
            max_filename_len: 240,
        });

        let err = result.expect_err("plan generation should fail");
        assert!(err.to_string().contains(&format!(
            "RAWフォルダが存在しません: {}",
            missing_raw_root.display()
        )));
    }

    #[test]
    fn generate_plan_fails_when_jpg_input_is_not_directory() {
        let temp = tempdir().expect("tempdir");
        let jpg_file = temp.path().join("not-dir.JPG");
        fs::write(&jpg_file, b"not-a-directory").expect("jpg file");

        let result = generate_plan(&PlanOptions {
            jpg_input: jpg_file.clone(),
            raw_input: None,
            raw_from_jpg_parent_when_missing: false,
            recursive: false,
            include_hidden: false,
            template: "{orig_name}".to_string(),
            dedupe_same_maker: true,
            exclusions: Vec::new(),
            max_filename_len: 240,
        });

        let err = result.expect_err("plan generation should fail");
        assert!(err.to_string().contains(&format!(
            "JPGフォルダではありません: {}",
            jpg_file.display()
        )));
    }

    #[test]
    fn generate_plan_fails_when_explicit_raw_path_is_not_directory() {
        let temp = tempdir().expect("tempdir");
        let jpg_root = temp.path().join("jpg");
        fs::create_dir_all(&jpg_root).expect("jpg root");

        let jpg_path = jpg_root.join("DSC00098.JPG");
        fs::write(&jpg_path, b"not-a-real-jpg").expect("jpg file");

        let raw_file = temp.path().join("raw-file.txt");
        fs::write(&raw_file, b"not-a-folder").expect("raw file");

        let result = generate_plan(&PlanOptions {
            jpg_input: jpg_root,
            raw_input: Some(raw_file.clone()),
            raw_from_jpg_parent_when_missing: false,
            recursive: false,
            include_hidden: false,
            template: "{orig_name}".to_string(),
            dedupe_same_maker: true,
            exclusions: Vec::new(),
            max_filename_len: 240,
        });

        let err = result.expect_err("plan generation should fail");
        assert!(err.to_string().contains(&format!(
            "RAWフォルダではありません: {}",
            raw_file.display()
        )));
    }

    #[test]
    fn generate_plan_falls_back_to_jpg_when_raw_file_is_missing() {
        let temp = tempdir().expect("tempdir");
        let jpg_root = temp.path().join("jpg");
        let raw_root = temp.path().join("raw");
        fs::create_dir_all(&jpg_root).expect("jpg root");
        fs::create_dir_all(&raw_root).expect("raw root");

        let jpg_path = jpg_root.join("DSC00100.JPG");
        fs::write(&jpg_path, b"not-a-real-jpg").expect("jpg file");

        let plan = generate_plan(&PlanOptions {
            jpg_input: jpg_root,
            raw_input: Some(raw_root),
            raw_from_jpg_parent_when_missing: false,
            recursive: false,
            include_hidden: false,
            template: "{orig_name}".to_string(),
            dedupe_same_maker: true,
            exclusions: Vec::new(),
            max_filename_len: 240,
        })
        .expect("plan generation should succeed");

        assert_eq!(plan.candidates.len(), 1);
        assert_eq!(plan.candidates[0].source_label, "jpg");
    }

    #[test]
    fn generate_plan_uses_jpg_parent_as_raw_when_enabled() {
        let temp = tempdir().expect("tempdir");
        let parent_root = temp.path().join("session");
        let jpg_root = parent_root.join("jpg");
        fs::create_dir_all(&jpg_root).expect("jpg root");

        let jpg_path = jpg_root.join("DSC00010.JPG");
        fs::write(&jpg_path, b"not-a-real-jpg").expect("jpg file");

        let xmp = parent_root.join("DSC00010.xmp");
        fs::write(
            &xmp,
            r#"<x:xmpmeta><rdf:RDF><rdf:Description><exif:DateTimeOriginal>2026:02:08 10:20:30</exif:DateTimeOriginal><exif:Make>FUJIFILM</exif:Make></rdf:Description></rdf:RDF></x:xmpmeta>"#,
        )
        .expect("xmp file");

        let plan = generate_plan(&PlanOptions {
            jpg_input: jpg_root,
            raw_input: None,
            raw_from_jpg_parent_when_missing: true,
            recursive: false,
            include_hidden: false,
            template: "{camera_maker}_{orig_name}".to_string(),
            dedupe_same_maker: true,
            exclusions: Vec::new(),
            max_filename_len: 240,
        })
        .expect("plan generation should succeed");

        assert_eq!(plan.candidates.len(), 1);
        let c = &plan.candidates[0];
        assert_eq!(c.metadata_source, MetadataSource::Xmp);
        assert_eq!(c.source_label, "xmp");
        assert_eq!(c.metadata.camera_make.as_deref(), Some("FUJIFILM"));
    }

    #[test]
    fn generate_plan_non_recursive_returns_stable_sorted_order() {
        let temp = tempdir().expect("tempdir");
        let jpg_root = temp.path().join("jpg");
        fs::create_dir_all(&jpg_root).expect("jpg root");
        fs::write(jpg_root.join("B.JPG"), b"b").expect("write b");
        fs::write(jpg_root.join("A.JPG"), b"a").expect("write a");

        let plan = generate_plan(&PlanOptions {
            jpg_input: jpg_root,
            raw_input: None,
            raw_from_jpg_parent_when_missing: false,
            recursive: false,
            include_hidden: false,
            template: "{orig_name}".to_string(),
            dedupe_same_maker: true,
            exclusions: Vec::new(),
            max_filename_len: 240,
        })
        .expect("plan generation should succeed");

        assert_eq!(plan.candidates.len(), 2);
        assert_eq!(
            plan.candidates[0]
                .original_path
                .file_name()
                .and_then(|v| v.to_str()),
            Some("A.JPG")
        );
        assert_eq!(
            plan.candidates[1]
                .original_path
                .file_name()
                .and_then(|v| v.to_str()),
            Some("B.JPG")
        );
    }

    #[test]
    fn generate_plan_recursive_skips_hidden_directories_when_disabled() {
        let temp = tempdir().expect("tempdir");
        let jpg_root = temp.path().join("jpg");
        let hidden_dir = jpg_root.join(".hidden");
        fs::create_dir_all(&hidden_dir).expect("hidden dir");
        fs::write(jpg_root.join("VISIBLE.JPG"), b"visible").expect("visible jpg");
        fs::write(hidden_dir.join("INSIDE.JPG"), b"hidden jpg").expect("hidden jpg");

        let plan = generate_plan(&PlanOptions {
            jpg_input: jpg_root,
            raw_input: None,
            raw_from_jpg_parent_when_missing: false,
            recursive: true,
            include_hidden: false,
            template: "{orig_name}".to_string(),
            dedupe_same_maker: true,
            exclusions: Vec::new(),
            max_filename_len: 240,
        })
        .expect("plan generation should succeed");

        assert_eq!(plan.candidates.len(), 1);
        assert_eq!(
            plan.candidates[0]
                .original_path
                .file_name()
                .and_then(|v| v.to_str()),
            Some("VISIBLE.JPG")
        );
        assert_eq!(plan.stats.jpg_files, 1);
        assert_eq!(plan.stats.skipped_hidden, 1);
    }

    #[test]
    fn metadata_source_label_uses_raw_extension_for_raw_exif() {
        let raw_path = PathBuf::from("/tmp/session/DSC00001.RAF");
        let label = metadata_source_label(MetadataSource::RawExif, Some(&raw_path));
        assert_eq!(label, "raf");
    }

    #[test]
    fn metadata_source_label_returns_xmp_for_combined_source() {
        let raw_path = PathBuf::from("/tmp/session/DSC00001.DNG");
        let label = metadata_source_label(MetadataSource::XmpAndRawExif, Some(&raw_path));
        assert_eq!(label, "xmp");
    }

    #[test]
    fn metadata_source_label_returns_jpg_for_fallback_source() {
        let label = metadata_source_label(MetadataSource::FallbackFileModified, None);
        assert_eq!(label, "jpg");
    }
}
