const WINDOWS_RESERVED_NAMES: &[&str] = &[
    "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
    "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];

pub fn apply_exclusions(mut value: String, exclusions: &[String]) -> String {
    for exclusion in exclusions {
        let term = exclusion.trim();
        if term.is_empty() {
            continue;
        }
        for variant in build_exclusion_variants(term) {
            value = replace_case_insensitive(&value, &variant);
        }
    }
    value
}

pub fn normalize_spaces_to_underscore(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    let mut in_space = false;

    for ch in value.chars() {
        if ch.is_whitespace() {
            if !in_space {
                out.push('_');
                in_space = true;
            }
            continue;
        }

        in_space = false;
        out.push(ch);
    }

    out
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

fn build_exclusion_variants(term: &str) -> Vec<String> {
    let mut variants = Vec::<String>::new();
    let canonical = normalize_plus_char(term);
    push_unique_case_insensitive(&mut variants, canonical.clone());
    push_unique_case_insensitive(&mut variants, replace_whitespace_runs(&canonical, '-'));
    push_unique_case_insensitive(&mut variants, replace_whitespace_runs(&canonical, '_'));

    let hyphen = normalize_separator_variant(&canonical, '-');
    push_unique_case_insensitive(&mut variants, hyphen);

    let underscore = normalize_separator_variant(&canonical, '_');
    push_unique_case_insensitive(&mut variants, underscore);

    let snapshot = variants.clone();
    for value in snapshot {
        push_unique_case_insensitive(&mut variants, compact_separator_before_plus(&value));
        push_unique_case_insensitive(&mut variants, ensure_separator_after_plus(&value, '-'));
        push_unique_case_insensitive(&mut variants, ensure_separator_after_plus(&value, '_'));
    }

    variants
}

fn normalize_separator_variant(term: &str, separator: char) -> String {
    let mut out = String::with_capacity(term.len());
    let mut in_sep = false;

    for ch in term.chars() {
        if ch.is_whitespace() || ch == '-' || ch == '_' {
            if !in_sep {
                out.push(separator);
                in_sep = true;
            }
            continue;
        }

        in_sep = false;
        out.push(ch);
    }

    out
}

fn replace_whitespace_runs(term: &str, separator: char) -> String {
    let mut out = String::with_capacity(term.len());
    let mut in_space = false;
    for ch in term.chars() {
        if ch.is_whitespace() {
            if !in_space {
                out.push(separator);
                in_space = true;
            }
            continue;
        }
        in_space = false;
        out.push(ch);
    }
    out
}

fn normalize_plus_char(input: &str) -> String {
    input
        .chars()
        .map(|ch| if ch == '＋' { '+' } else { ch })
        .collect()
}

fn compact_separator_before_plus(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut pending_separators = String::new();

    for ch in input.chars() {
        if ch.is_whitespace() || ch == '-' || ch == '_' {
            pending_separators.push(ch);
            continue;
        }

        if ch == '+' {
            pending_separators.clear();
            out.push(ch);
            continue;
        }

        out.push_str(&pending_separators);
        pending_separators.clear();
        out.push(ch);
    }

    out.push_str(&pending_separators);
    out
}

fn ensure_separator_after_plus(input: &str, separator: char) -> String {
    let mut out = String::with_capacity(input.len() + 4);
    let chars: Vec<char> = input.chars().collect();

    for (index, ch) in chars.iter().enumerate() {
        out.push(*ch);
        if *ch != '+' {
            continue;
        }

        let next = chars.get(index + 1).copied();
        if matches!(next, Some(n) if !(n.is_whitespace() || n == '-' || n == '_' || n == '+')) {
            out.push(separator);
        }
    }

    out
}

fn push_unique_case_insensitive(values: &mut Vec<String>, value: String) {
    if value.is_empty() {
        return;
    }
    if values
        .iter()
        .any(|existing| existing.eq_ignore_ascii_case(&value))
    {
        return;
    }
    values.push(value);
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

    #[test]
    fn exclusions_match_space_hyphen_underscore_variants() {
        let value = apply_exclusions(
            "REALA-ACE_REALA_ACE_REALA ACE".to_string(),
            &["reala ace".to_string()],
        );
        assert_eq!(value, "__");
    }

    #[test]
    fn exclusions_can_remove_hyphenized_filter_name_by_spaced_input() {
        let value = apply_exclusions(
            "ACROS+-R-FILTER_DSC0001".to_string(),
            &["ACROS+ R FILTER".to_string()],
        );
        assert_eq!(value, "_DSC0001");
    }

    #[test]
    fn exclusions_can_remove_hyphenized_filter_name_with_space_before_plus() {
        let value = apply_exclusions(
            "ACROS+-R-FILTER_DSC0002".to_string(),
            &["ACROS + R FILTER".to_string()],
        );
        assert_eq!(value, "_DSC0002");
    }

    #[test]
    fn exclusions_can_remove_hyphenized_filter_name_without_space_after_plus() {
        let value = apply_exclusions(
            "MONOCHROME+-Ye-FILTER_DSC0003".to_string(),
            &["MONOCHROME+Ye FILTER".to_string()],
        );
        assert_eq!(value, "_DSC0003");
    }

    #[test]
    fn exclusions_can_remove_user_provided_dxo_suffixes() {
        let value = apply_exclusions(
            "IMG0001-強化-NR-DxO_DeepPRIME-XD2s_XD-DxO_DeepPRIME-3-DxO_DeepPRIME-XD3-X-Trans"
                .to_string(),
            &[
                "-強化-NR".to_string(),
                "-DxO_DeepPRIME XD2s_XD".to_string(),
                "-DxO_DeepPRIME 3".to_string(),
                "-DxO_DeepPRIME XD3 X-Trans".to_string(),
            ],
        );
        assert_eq!(value, "IMG0001");
    }

    #[test]
    fn normalize_spaces_to_underscore_runs_once() {
        let value = normalize_spaces_to_underscore("A  B   C");
        assert_eq!(value, "A_B_C");
    }
}
