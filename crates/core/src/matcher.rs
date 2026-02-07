use std::path::{Path, PathBuf};

const RAW_EXT_PRIORITY: &[&str] = &["dng", "raf"];

pub fn find_matching_raw(
    jpg_root: &Path,
    raw_root: &Path,
    jpg_path: &Path,
    recursive: bool,
) -> Option<PathBuf> {
    let rel_dir = if recursive {
        jpg_path
            .strip_prefix(jpg_root)
            .ok()
            .and_then(|rel| rel.parent().map(|p| p.to_path_buf()))
    } else {
        None
    };

    let stem = jpg_path.file_stem()?.to_string_lossy().to_string();

    for ext in RAW_EXT_PRIORITY {
        let candidate = if let Some(dir) = rel_dir.as_ref() {
            raw_root.join(dir).join(format!("{}.{}", stem, ext))
        } else {
            raw_root.join(format!("{}.{}", stem, ext))
        };
        if candidate.exists() {
            return Some(candidate);
        }

        let upper = if let Some(dir) = rel_dir.as_ref() {
            raw_root
                .join(dir)
                .join(format!("{}.{}", stem, ext.to_ascii_uppercase()))
        } else {
            raw_root.join(format!("{}.{}", stem, ext.to_ascii_uppercase()))
        };
        if upper.exists() {
            return Some(upper);
        }
    }

    None
}

pub fn xmp_for_raw(raw_path: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let stem = raw_path
        .file_stem()
        .map(|v| v.to_string_lossy().to_string())
        .unwrap_or_default();
    if stem.is_empty() {
        return out;
    }

    if let Some(parent) = raw_path.parent() {
        out.push(parent.join(format!("{}.xmp", stem)));
        out.push(parent.join(format!("{}.XMP", stem)));
    }

    out
}
