use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

const RAW_EXT_PRIORITY: &[&str] = &["dng", "raf"];
const XMP_EXT_PRIORITY: &[&str] = &["xmp"];

#[derive(Debug, Clone)]
pub struct RawMatchIndex {
    recursive: bool,
    jpg_root: PathBuf,
    files_by_rel_dir: HashMap<PathBuf, HashMap<String, Vec<PathBuf>>>,
}

pub fn build_raw_match_index(jpg_root: &Path, raw_root: &Path, recursive: bool) -> RawMatchIndex {
    let mut files_by_rel_dir = HashMap::<PathBuf, HashMap<String, Vec<PathBuf>>>::new();

    if recursive {
        for entry in WalkDir::new(raw_root).sort_by_file_name() {
            let Ok(entry) = entry else {
                continue;
            };
            if !entry.file_type().is_file() {
                continue;
            }
            insert_index_path(&mut files_by_rel_dir, raw_root, entry.path(), true);
        }
    } else if let Ok(entries) = fs::read_dir(raw_root) {
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_file() {
                continue;
            }
            insert_index_path(&mut files_by_rel_dir, raw_root, &path, false);
        }
    }

    for stem_map in files_by_rel_dir.values_mut() {
        for candidates in stem_map.values_mut() {
            candidates.sort();
        }
    }

    RawMatchIndex {
        recursive,
        jpg_root: jpg_root.to_path_buf(),
        files_by_rel_dir,
    }
}

impl RawMatchIndex {
    pub fn find_raw(&self, jpg_path: &Path) -> Option<PathBuf> {
        self.find_matching_by_priority(jpg_path, RAW_EXT_PRIORITY)
    }

    pub fn find_xmp(&self, jpg_path: &Path) -> Option<PathBuf> {
        self.find_matching_by_priority(jpg_path, XMP_EXT_PRIORITY)
    }

    fn find_matching_by_priority(&self, jpg_path: &Path, extensions: &[&str]) -> Option<PathBuf> {
        let rel_dir = self.resolve_search_rel_dir(jpg_path);
        let stem_original = jpg_path.file_stem()?.to_string_lossy().to_string();
        let stem_key = stem_original.to_ascii_lowercase();
        let candidates = self.files_by_rel_dir.get(&rel_dir)?.get(&stem_key)?;

        for ext in extensions {
            if let Some(path) = pick_candidate_with_case_variants(candidates, &stem_original, ext) {
                return Some(path);
            }
        }

        None
    }

    fn resolve_search_rel_dir(&self, jpg_path: &Path) -> PathBuf {
        if !self.recursive {
            return PathBuf::new();
        }

        jpg_path
            .strip_prefix(&self.jpg_root)
            .ok()
            .and_then(|rel| rel.parent().map(PathBuf::from))
            .unwrap_or_default()
    }
}

pub fn find_matching_raw(
    jpg_root: &Path,
    raw_root: &Path,
    jpg_path: &Path,
    recursive: bool,
) -> Option<PathBuf> {
    find_matching_by_priority(jpg_root, raw_root, jpg_path, recursive, RAW_EXT_PRIORITY)
}

pub fn find_matching_xmp(
    jpg_root: &Path,
    raw_root: &Path,
    jpg_path: &Path,
    recursive: bool,
) -> Option<PathBuf> {
    find_matching_by_priority(jpg_root, raw_root, jpg_path, recursive, XMP_EXT_PRIORITY)
}

fn find_matching_by_priority(
    jpg_root: &Path,
    raw_root: &Path,
    jpg_path: &Path,
    recursive: bool,
    extensions: &[&str],
) -> Option<PathBuf> {
    let search_dir = resolve_search_dir(jpg_root, raw_root, jpg_path, recursive);
    let stem = jpg_path.file_stem()?.to_string_lossy().to_string();

    for ext in extensions {
        if let Some(path) = find_candidate_with_case_variants(&search_dir, &stem, ext) {
            return Some(path);
        }
    }

    None
}

fn resolve_search_dir(
    jpg_root: &Path,
    raw_root: &Path,
    jpg_path: &Path,
    recursive: bool,
) -> PathBuf {
    if !recursive {
        return raw_root.to_path_buf();
    }

    let rel_dir = jpg_path
        .strip_prefix(jpg_root)
        .ok()
        .and_then(|rel| rel.parent().map(PathBuf::from));

    if let Some(dir) = rel_dir {
        raw_root.join(dir)
    } else {
        raw_root.to_path_buf()
    }
}

fn find_candidate_with_case_variants(search_dir: &Path, stem: &str, ext: &str) -> Option<PathBuf> {
    let lower = search_dir.join(format!("{}.{}", stem, ext));
    if lower.exists() {
        return Some(lower);
    }

    let upper = search_dir.join(format!("{}.{}", stem, ext.to_ascii_uppercase()));
    if upper.exists() {
        return Some(upper);
    }

    let expected = format!("{}.{}", stem, ext);
    let expected_lower = expected.to_ascii_lowercase();
    let entries = fs::read_dir(search_dir).ok()?;
    for entry in entries.flatten() {
        let name = entry.file_name();
        if !name.to_string_lossy().eq_ignore_ascii_case(&expected_lower) {
            continue;
        }
        let path = entry.path();
        if path.is_file() {
            return Some(path);
        }
    }

    None
}

fn insert_index_path(
    files_by_rel_dir: &mut HashMap<PathBuf, HashMap<String, Vec<PathBuf>>>,
    raw_root: &Path,
    path: &Path,
    recursive: bool,
) {
    let ext = path
        .extension()
        .and_then(|v| v.to_str())
        .unwrap_or_default();
    if !is_index_target_extension(ext) {
        return;
    }

    let stem = path
        .file_stem()
        .and_then(|v| v.to_str())
        .unwrap_or_default();
    if stem.is_empty() {
        return;
    }

    let rel_dir = if recursive {
        path.parent()
            .and_then(|parent| parent.strip_prefix(raw_root).ok())
            .map(PathBuf::from)
            .unwrap_or_default()
    } else {
        PathBuf::new()
    };

    let stem_key = stem.to_ascii_lowercase();
    let stem_map = files_by_rel_dir.entry(rel_dir).or_default();
    stem_map
        .entry(stem_key)
        .or_default()
        .push(path.to_path_buf());
}

fn pick_candidate_with_case_variants(
    candidates: &[PathBuf],
    stem_original: &str,
    ext: &str,
) -> Option<PathBuf> {
    let exact_lower = format!("{}.{}", stem_original, ext);
    if let Some(path) = candidates
        .iter()
        .find(|candidate| file_name_equals(candidate, &exact_lower))
    {
        return Some(path.clone());
    }

    let exact_upper = format!("{}.{}", stem_original, ext.to_ascii_uppercase());
    if let Some(path) = candidates
        .iter()
        .find(|candidate| file_name_equals(candidate, &exact_upper))
    {
        return Some(path.clone());
    }

    let expected_ci = exact_lower.to_ascii_lowercase();
    if let Some(path) = candidates.iter().find(|candidate| {
        candidate
            .file_name()
            .and_then(|v| v.to_str())
            .map(|name| name.to_ascii_lowercase() == expected_ci)
            .unwrap_or(false)
    }) {
        return Some(path.clone());
    }

    candidates
        .iter()
        .find(|candidate| {
            candidate
                .extension()
                .and_then(|v| v.to_str())
                .map(|candidate_ext| candidate_ext.eq_ignore_ascii_case(ext))
                .unwrap_or(false)
        })
        .cloned()
}

fn file_name_equals(path: &Path, expected: &str) -> bool {
    path.file_name()
        .and_then(|v| v.to_str())
        .map(|name| name == expected)
        .unwrap_or(false)
}

fn is_index_target_extension(ext: &str) -> bool {
    ext.eq_ignore_ascii_case("dng")
        || ext.eq_ignore_ascii_case("raf")
        || ext.eq_ignore_ascii_case("xmp")
}

#[cfg(test)]
mod tests {
    use super::{build_raw_match_index, find_matching_raw, find_matching_xmp};
    use std::fs::{self, File};
    use std::path::Path;
    use tempfile::tempdir;

    fn touch(path: &Path) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("parent dirs must be creatable");
        }
        File::create(path).expect("file must be creatable");
    }

    #[test]
    fn finds_xmp_even_when_raw_missing() {
        let temp = tempdir().expect("tempdir");
        let jpg_root = temp.path().join("jpg");
        let raw_root = temp.path().join("raw");
        fs::create_dir_all(&jpg_root).expect("jpg root");
        fs::create_dir_all(&raw_root).expect("raw root");

        let jpg = jpg_root.join("DSC00001.JPG");
        let xmp = raw_root.join("DSC00001.xmp");
        touch(&xmp);

        let found_xmp = find_matching_xmp(&jpg_root, &raw_root, &jpg, false);
        let found_raw = find_matching_raw(&jpg_root, &raw_root, &jpg, false);
        assert_eq!(found_xmp.as_deref(), Some(xmp.as_path()));
        assert!(found_raw.is_none());

        let index = build_raw_match_index(&jpg_root, &raw_root, false);
        assert_eq!(index.find_xmp(&jpg).as_deref(), Some(xmp.as_path()));
        assert!(index.find_raw(&jpg).is_none());
    }

    #[test]
    fn prefers_dng_over_raf_when_both_exist() {
        let temp = tempdir().expect("tempdir");
        let jpg_root = temp.path().join("jpg");
        let raw_root = temp.path().join("raw");
        fs::create_dir_all(&jpg_root).expect("jpg root");
        fs::create_dir_all(&raw_root).expect("raw root");

        let jpg = jpg_root.join("DSC00002.JPG");
        let dng = raw_root.join("DSC00002.dng");
        let raf = raw_root.join("DSC00002.raf");
        touch(&dng);
        touch(&raf);

        let found = find_matching_raw(&jpg_root, &raw_root, &jpg, false);
        assert_eq!(found.as_deref(), Some(dng.as_path()));

        let index = build_raw_match_index(&jpg_root, &raw_root, false);
        assert_eq!(index.find_raw(&jpg).as_deref(), Some(dng.as_path()));
    }

    #[test]
    fn resolves_recursive_relative_directory() {
        let temp = tempdir().expect("tempdir");
        let jpg_root = temp.path().join("jpg");
        let raw_root = temp.path().join("raw");
        let jpg = jpg_root.join("day1/DSC00003.JPG");
        let xmp = raw_root.join("day1/DSC00003.XMP");
        let raf = raw_root.join("day1/DSC00003.RAF");

        touch(&xmp);
        touch(&raf);

        let found_xmp = find_matching_xmp(&jpg_root, &raw_root, &jpg, true);
        let found_raw = find_matching_raw(&jpg_root, &raw_root, &jpg, true);

        let found_xmp = found_xmp.expect("xmp should be found");
        let found_raw = found_raw.expect("raw should be found");
        assert!(found_xmp.exists());
        assert!(found_raw.exists());
        assert!(found_xmp
            .extension()
            .and_then(|v| v.to_str())
            .map(|v| v.eq_ignore_ascii_case("xmp"))
            .unwrap_or(false));
        assert!(found_raw
            .extension()
            .and_then(|v| v.to_str())
            .map(|v| v.eq_ignore_ascii_case("raf"))
            .unwrap_or(false));

        let index = build_raw_match_index(&jpg_root, &raw_root, true);
        assert_eq!(index.find_xmp(&jpg).as_deref(), Some(xmp.as_path()));
        assert_eq!(index.find_raw(&jpg).as_deref(), Some(raf.as_path()));
    }
}
