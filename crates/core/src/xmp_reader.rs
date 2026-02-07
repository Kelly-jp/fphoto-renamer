use crate::metadata::PartialMetadata;
use anyhow::{Context, Result};
use chrono::{DateTime, Local, NaiveDateTime, TimeZone};
use std::fs;
use std::path::Path;

pub fn read_xmp_metadata(path: &Path) -> Result<PartialMetadata> {
    let xml = fs::read_to_string(path)
        .with_context(|| format!("XMPを開けませんでした: {}", path.display()))?;

    let date = find_value(&xml, &["datetimeoriginal", "createdate", "datecreated"])
        .as_deref()
        .and_then(parse_date);
    let camera_make = find_value(&xml, &["make"]);
    let camera_model = find_value(&xml, &["model"]);
    let lens_make = find_value(&xml, &["lensmake"]);
    let lens_model = find_value(&xml, &["lensmodel", "lens"]);
    let film_sim = find_value(&xml, &["filmsimulation", "filmmode", "filmsimulationname"]);

    Ok(PartialMetadata {
        date,
        camera_make: normalize(camera_make),
        camera_model: normalize(camera_model),
        lens_make: normalize(lens_make),
        lens_model: normalize(lens_model),
        film_sim: normalize(film_sim),
    })
}

fn find_value(xml: &str, keys: &[&str]) -> Option<String> {
    let mut cursor = 0usize;
    while let Some(start) = xml[cursor..].find('<') {
        let start = cursor + start;
        let end = xml[start..].find('>')? + start;
        let raw_tag = &xml[start + 1..end];

        if raw_tag.starts_with('/') || raw_tag.starts_with('?') || raw_tag.starts_with('!') {
            cursor = end + 1;
            continue;
        }

        let tag_name = raw_tag.split_whitespace().next().unwrap_or_default();
        let suffix = normalize_tag_name(tag_name);
        if !keys.iter().any(|k| k == &suffix) {
            cursor = end + 1;
            continue;
        }

        let close_tag = format!("</{}>", tag_name);
        if let Some(close_pos) = xml[end + 1..].find(&close_tag) {
            let close_pos = end + 1 + close_pos;
            let content = xml[end + 1..close_pos].trim();
            if !content.is_empty() {
                return Some(html_unescape_basic(content));
            }
        }

        cursor = end + 1;
    }

    None
}

fn normalize(value: Option<String>) -> Option<String> {
    value
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
}

fn normalize_tag_name(tag: &str) -> String {
    tag.rsplit(':')
        .next()
        .unwrap_or_default()
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .collect::<String>()
        .to_ascii_lowercase()
}

fn html_unescape_basic(input: &str) -> String {
    input
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
}

fn parse_date(input: &str) -> Option<DateTime<Local>> {
    let candidates = [
        "%Y:%m:%d %H:%M:%S",
        "%Y-%m-%d %H:%M:%S",
        "%Y-%m-%dT%H:%M:%S",
        "%Y-%m-%dT%H:%M:%S%.f",
        "%Y-%m-%dT%H:%M:%S%:z",
        "%Y-%m-%dT%H:%M:%S%.f%:z",
    ];

    for fmt in candidates {
        if let Ok(dt) = DateTime::parse_from_str(input, fmt) {
            return Some(dt.with_timezone(&Local));
        }
        if let Ok(naive) = NaiveDateTime::parse_from_str(input, fmt) {
            if let Some(local) = Local.from_local_datetime(&naive).single() {
                return Some(local);
            }
        }
    }

    None
}
