const WINDOWS_RESERVED_NAMES: &[&str] = &[
    "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
    "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];

pub fn apply_exclusions(mut value: String, exclusions: &[String]) -> String {
    for exclusion in exclusions {
        if exclusion.is_empty() {
            continue;
        }
        value = replace_case_insensitive(&value, exclusion);
    }
    value
}

pub fn cleanup_filename(value: &str) -> String {
    let mut out = String::new();
    let mut prev_sep: Option<char> = None;

    for ch in value.chars() {
        if is_collapse_separator(ch) {
            if prev_sep == Some(ch) {
                continue;
            }
            prev_sep = Some(ch);
            out.push(ch);
        } else {
            prev_sep = None;
            out.push(ch);
        }
    }

    out.trim_matches(|c: char| c == '_' || c == '-' || c == ' ' || c == '.')
        .to_string()
}

pub fn sanitize_filename(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for ch in value.chars() {
        if is_disallowed_char(ch) {
            out.push('_');
        } else {
            out.push(ch);
        }
    }

    let mut out = out.trim_end_matches([' ', '.']).trim().to_string();

    if out.is_empty() {
        out = "untitled".to_string();
    }

    if is_windows_reserved(&out) {
        out.push_str("_file");
    }

    out
}

pub fn truncate_filename_if_needed(
    filename_without_ext: &str,
    extension_with_dot: &str,
    limit: usize,
) -> String {
    let ext_len = extension_with_dot.chars().count();
    if filename_without_ext.chars().count() + ext_len <= limit {
        return filename_without_ext.to_string();
    }

    let mut tokens: Vec<&str> = filename_without_ext.split('_').collect();
    while tokens.len() > 1 {
        tokens.pop();
        let candidate = tokens.join("_");
        if candidate.chars().count() + ext_len <= limit {
            return candidate;
        }
    }

    filename_without_ext
        .chars()
        .take(limit.saturating_sub(ext_len))
        .collect()
}

fn replace_case_insensitive(haystack: &str, needle: &str) -> String {
    let lower_hay = haystack.to_lowercase();
    let lower_needle = needle.to_lowercase();
    if lower_needle.is_empty() {
        return haystack.to_string();
    }

    let mut result = String::with_capacity(haystack.len());
    let mut cursor = 0usize;

    while let Some(pos) = lower_hay[cursor..].find(&lower_needle) {
        let abs_pos = cursor + pos;
        result.push_str(&haystack[cursor..abs_pos]);
        cursor = abs_pos + lower_needle.len();
    }

    result.push_str(&haystack[cursor..]);
    result
}

fn is_collapse_separator(ch: char) -> bool {
    matches!(ch, '_' | '-' | ' ')
}

fn is_disallowed_char(ch: char) -> bool {
    matches!(ch, '\\' | '/' | ':' | '*' | '?' | '"' | '<' | '>' | '|')
        || ch == '\0'
        || ch.is_control()
}

fn is_windows_reserved(value: &str) -> bool {
    let stem = value
        .split('.')
        .next()
        .unwrap_or(value)
        .to_ascii_uppercase();
    WINDOWS_RESERVED_NAMES
        .iter()
        .any(|reserved| reserved == &stem)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exclusions_are_case_insensitive() {
        let value = apply_exclusions("Fuji_FUJIFILM_fuji".to_string(), &["fUji".to_string()]);
        assert_eq!(value, "_FILM_");
    }

    #[test]
    fn cleanup_compacts_and_trims() {
        let value = cleanup_filename("__hello___world__");
        assert_eq!(value, "hello_world");
    }

    #[test]
    fn sanitize_handles_disallowed_chars() {
        let value = sanitize_filename("AUX");
        assert_eq!(value, "AUX_file");
    }
}
