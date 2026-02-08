use std::fs;
use std::path::{Path, PathBuf};

const RAW_EXT_PRIORITY: &[&str] = &["dng", "raf"];
const XMP_EXT_PRIORITY: &[&str] = &["xmp"];

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

#[cfg(test)]
mod tests {
    use super::{find_matching_raw, find_matching_xmp};
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
    }
}
