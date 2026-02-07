use crate::metadata::PartialMetadata;
use anyhow::{Context, Result};
use chrono::{DateTime, Local, NaiveDateTime, TimeZone};
use exif::Reader;
use std::fs::File;
use std::io::BufReader;
use std::path::Path;

pub fn read_exif_metadata(path: &Path) -> Result<PartialMetadata> {
    let file = File::open(path)
        .with_context(|| format!("EXIF読み込み対象を開けませんでした: {}", path.display()))?;
    let mut buf = BufReader::new(file);
    let exif = Reader::new()
        .read_from_container(&mut buf)
        .with_context(|| format!("EXIFを解析できませんでした: {}", path.display()))?;

    let date = find_field_value(
        &exif,
        &["DateTimeOriginal", "DateTimeDigitized", "DateTime"],
    )
    .and_then(|raw| parse_date(&raw));

    let camera_make = find_field_value(&exif, &["Make"]);
    let camera_model = find_field_value(&exif, &["Model"]);
    let lens_make = find_field_value(&exif, &["LensMake"]);
    let lens_model = find_field_value(&exif, &["LensModel", "Lens"]);
    let film_sim = find_field_value(
        &exif,
        &[
            "FilmMode",
            "FilmSimulation",
            "FilmSimulationName",
            "PictureMode",
        ],
    );

    Ok(PartialMetadata {
        date,
        camera_make: normalize(camera_make),
        camera_model: normalize(camera_model),
        lens_make: normalize(lens_make),
        lens_model: normalize(lens_model),
        film_sim: normalize(film_sim),
    })
}

fn normalize(value: Option<String>) -> Option<String> {
    value
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
}

fn find_field_value(exif: &exif::Exif, names: &[&str]) -> Option<String> {
    exif.fields().find_map(|field| {
        let tag_name = format!("{:?}", field.tag);
        if names
            .iter()
            .any(|name| name.eq_ignore_ascii_case(&tag_name))
        {
            Some(field.display_value().with_unit(exif).to_string())
        } else {
            None
        }
    })
}

fn parse_date(input: &str) -> Option<DateTime<Local>> {
    let normalized = input.trim();

    let candidates = [
        "%Y:%m:%d %H:%M:%S",
        "%Y-%m-%d %H:%M:%S",
        "%Y-%m-%dT%H:%M:%S",
        "%Y-%m-%dT%H:%M:%S%.f",
        "%Y-%m-%dT%H:%M:%S%:z",
        "%Y-%m-%dT%H:%M:%S%.f%:z",
    ];

    for fmt in candidates {
        if let Ok(dt) = DateTime::parse_from_str(normalized, fmt) {
            return Some(dt.with_timezone(&Local));
        }
        if let Ok(naive) = NaiveDateTime::parse_from_str(normalized, fmt) {
            if let Some(local) = Local.from_local_datetime(&naive).single() {
                return Some(local);
            }
        }
    }

    None
}
