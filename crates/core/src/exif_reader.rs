use crate::metadata::PartialMetadata;
use anyhow::{anyhow, Context, Result};
use chrono::{DateTime, Local, NaiveDateTime, TimeZone};
use exif::{Field, Reader as KamadakReader, Value as ExifValue};
use exiftool::ExifTool;
use serde_json::Value as JsonValue;
use std::fs::File;
use std::io::BufReader;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

const EXIFTOOL_PATH_ENV: &str = "FPHOTO_EXIFTOOL_PATH";
const FUJIFILM_MAKER_NOTE_PREFIX: &[u8] = b"FUJIFILM";
const FUJIFILM_TAG_FILM_MODE: u16 = 0x1401;
const EXIFTOOL_TAGS: &[&str] = &[
    "DateTimeOriginal",
    "DateTimeDigitized",
    "DateTime",
    "Make",
    "Model",
    "Saturation",
    "ColorMode",
    "LensMake",
    "LensManufacturer",
    "LensModel",
    "Lens",
    "LensType",
    "LensInfo",
    "LensSpecification",
    "FilmMode",
    "FilmSimulation",
    "FilmSimulationName",
    "PictureMode",
];

static EXIFTOOL_INSTANCE: OnceLock<Option<Mutex<ExifTool>>> = OnceLock::new();

pub fn read_exif_metadata(path: &Path) -> Result<PartialMetadata> {
    match read_exif_metadata_with_exiftool(path) {
        Ok(mut exiftool_meta) => {
            if metadata_has_missing_fields(&exiftool_meta) {
                if let Ok(kamadak_meta) = read_exif_metadata_with_kamadak(path) {
                    exiftool_meta.merge_missing_from(&kamadak_meta);
                }
            }
            Ok(exiftool_meta)
        }
        Err(exiftool_err) => match read_exif_metadata_with_kamadak(path) {
            Ok(kamadak_meta) => Ok(kamadak_meta),
            Err(kamadak_err) => Err(anyhow!(
                "EXIFを解析できませんでした: {} (exiftool: {}; kamadak-exif: {})",
                path.display(),
                exiftool_err,
                kamadak_err
            )),
        },
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

fn exiftool_instance() -> Option<&'static Mutex<ExifTool>> {
    EXIFTOOL_INSTANCE.get_or_init(init_exiftool).as_ref()
}

fn init_exiftool() -> Option<Mutex<ExifTool>> {
    if let Some(path) = configured_exiftool_path() {
        if let Ok(exiftool) = ExifTool::with_executable(&path) {
            return Some(Mutex::new(exiftool));
        }
    }

    if let Ok(exiftool) = ExifTool::new() {
        return Some(Mutex::new(exiftool));
    }

    None
}

fn configured_exiftool_path() -> Option<PathBuf> {
    let raw = std::env::var_os(EXIFTOOL_PATH_ENV)?;
    if raw.is_empty() {
        return None;
    }
    Some(PathBuf::from(raw))
}

fn read_exif_metadata_with_exiftool(path: &Path) -> Result<PartialMetadata> {
    let exiftool_mutex = exiftool_instance().ok_or_else(|| anyhow!("ExifTool が利用できません"))?;
    let exiftool = exiftool_mutex
        .lock()
        .map_err(|_| anyhow!("ExifTool のロック取得に失敗しました"))?;

    let args_owned = EXIFTOOL_TAGS
        .iter()
        .map(|tag| format!("-{tag}"))
        .collect::<Vec<_>>();
    let args = args_owned.iter().map(String::as_str).collect::<Vec<_>>();

    let json = exiftool
        .json(path, &args)
        .map_err(|err| anyhow!("ExifTool 取得失敗: {err}"))?;

    let date = pick_json_string(
        &json,
        &["DateTimeOriginal", "DateTimeDigitized", "DateTime"],
    )
    .and_then(|raw| parse_date(&raw));
    let camera_make = pick_json_string(&json, &["Make"]);
    let camera_model = pick_json_string(&json, &["Model"]);
    let lens_make = pick_json_string(&json, &["LensMake", "LensManufacturer"]);
    let lens_model = pick_json_string(
        &json,
        &[
            "LensModel",
            "Lens",
            "LensType",
            "LensInfo",
            "LensSpecification",
        ],
    );
    let film_sim = pick_film_simulation_from_json(&json);

    Ok(PartialMetadata {
        date,
        camera_make: normalize(camera_make),
        camera_model: normalize(camera_model),
        lens_make: normalize(lens_make),
        lens_model: normalize(lens_model),
        film_sim: normalize(film_sim),
    })
}

fn pick_json_string(json: &JsonValue, keys: &[&str]) -> Option<String> {
    for key in keys {
        if let Some(value) = json.get(*key) {
            if let Some(text) = json_value_to_string(value) {
                return Some(text);
            }
        }
    }
    None
}

fn json_value_to_string(value: &JsonValue) -> Option<String> {
    match value {
        JsonValue::String(v) => {
            let text = v.trim();
            if text.is_empty() {
                None
            } else {
                Some(text.to_string())
            }
        }
        JsonValue::Number(v) => Some(v.to_string()),
        JsonValue::Bool(v) => Some(v.to_string()),
        JsonValue::Array(values) => values.iter().find_map(json_value_to_string),
        _ => None,
    }
}

fn pick_film_simulation_from_json(json: &JsonValue) -> Option<String> {
    if let Some(raw) = pick_json_string(json, &["Saturation"]) {
        if let Some(mapped) = normalize_film_simulation_from_saturation(&raw) {
            return Some(mapped);
        }
    }

    if let Some(raw) = pick_json_string(json, &["ColorMode"]) {
        if let Some(mapped) = normalize_film_simulation_name(&raw, false) {
            return Some(mapped);
        }
    }

    for key in ["FilmSimulationName", "FilmSimulation", "FilmMode"] {
        if let Some(raw) = pick_json_string(json, &[key]) {
            if let Some(mapped) = normalize_film_simulation_name(&raw, true) {
                return Some(mapped);
            }
        }
    }

    if let Some(raw) = pick_json_string(json, &["PictureMode"]) {
        return normalize_film_simulation_name(&raw, false);
    }

    None
}

fn normalize_film_simulation_from_saturation(raw: &str) -> Option<String> {
    let upper = raw.trim().to_ascii_uppercase();
    if upper.is_empty() {
        return None;
    }

    if upper.contains("ACROS") {
        if upper.contains("RED FILTER") {
            return Some("ACROS+ R FILTER".to_string());
        }
        if upper.contains("YELLOW FILTER") {
            return Some("ACROS+ Ye FILTER".to_string());
        }
        if upper.contains("GREEN FILTER") {
            return Some("ACROS+ G FILTER".to_string());
        }
        return Some("ACROS".to_string());
    }

    if upper.contains("B&W SEPIA") || upper.contains("SEPIA") {
        return Some("SEPIA".to_string());
    }

    if upper.contains("B&W")
        || upper.contains("MONOCHROME")
        || upper.contains("(B&W)")
        || upper.contains("BLACK & WHITE")
        || upper.contains("BLACK-WHITE")
    {
        if upper.contains("RED FILTER") {
            return Some("MONOCHROME+ R FILTER".to_string());
        }
        if upper.contains("YELLOW FILTER") {
            return Some("MONOCHROME+ Ye FILTER".to_string());
        }
        if upper.contains("GREEN FILTER") {
            return Some("MONOCHROME+ G FILTER".to_string());
        }
        if upper.contains("+R") {
            return Some("MONOCHROME+ R FILTER".to_string());
        }
        if upper.contains("+Y") {
            return Some("MONOCHROME+ Ye FILTER".to_string());
        }
        if upper.contains("+G") {
            return Some("MONOCHROME+ G FILTER".to_string());
        }
        return Some("MONOCHROME".to_string());
    }

    None
}

fn normalize_film_simulation_name(raw: &str, allow_unmapped: bool) -> Option<String> {
    let text = raw.trim().trim_matches('"');
    if text.is_empty() {
        return None;
    }

    let upper = text.to_ascii_uppercase();
    let mapped = if upper.contains("REALA ACE") || upper.contains("REALA-ACE") {
        Some("REALA ACE")
    } else if upper.contains("NOSTALGIC NEG") || upper.contains("NOSTALGIC-NEG") {
        Some("NOSTALGIC Neg")
    } else if upper.contains("BLEACH BYPASS") || upper.contains("BLEACH-BYPASS") {
        Some("ETERNA BLEACH BYPASS")
    } else if upper.contains("CLASSIC CHROME") || upper.contains("CLASSIC-CHROME") {
        Some("CLASSIC CHROME")
    } else if upper.contains("CLASSIC NEGATIVE")
        || upper.contains("CLASSIC NEG")
        || upper.contains("CLASSIC-NEG")
    {
        Some("CLASSIC Neg")
    } else if upper.contains("PRO NEG") && upper.contains("STD") {
        Some("PRO Neg Std")
    } else if upper.contains("PRO NEG") && upper.contains("HI") {
        Some("PRO Neg Hi")
    } else if upper.contains("PROVIA") || upper.contains("F0/STANDARD") {
        Some("PROVIA")
    } else if upper.contains("VELVIA") {
        Some("Velvia")
    } else if upper.contains("ASTIA") {
        Some("ASTIA")
    } else if upper.contains("ETERNA") {
        Some("ETERNA")
    } else if upper.contains("ACROS") {
        Some("ACROS")
    } else if upper.contains("MONOCHROME")
        || upper.contains("BLACK & WHITE")
        || upper.contains("BLACK-WHITE")
        || upper.contains("B&W")
    {
        Some("MONOCHROME")
    } else if upper.contains("SEPIA") {
        Some("SEPIA")
    } else {
        None
    };

    if let Some(name) = mapped {
        return Some(name.to_string());
    }

    if allow_unmapped {
        return Some(text.to_string());
    }

    None
}

fn read_exif_metadata_with_kamadak(path: &Path) -> Result<PartialMetadata> {
    let file = File::open(path)
        .with_context(|| format!("EXIF読み込み対象を開けませんでした: {}", path.display()))?;
    let mut buf = BufReader::new(file);
    let mut reader = KamadakReader::new();
    reader.continue_on_error(true);
    let exif = reader
        .read_from_container(&mut buf)
        .or_else(|err| err.distill_partial_result(|_| {}))
        .with_context(|| format!("EXIFを解析できませんでした: {}", path.display()))?;

    let date = find_field_value(
        &exif,
        &["DateTimeOriginal", "DateTimeDigitized", "DateTime"],
    )
    .and_then(|raw| parse_date(&raw));

    let camera_make = find_field_value(&exif, &["Make", "CameraMake"]);
    let camera_model = find_field_value(&exif, &["Model", "CameraModel", "UniqueCameraModel"]);
    let lens_make = find_field_value(&exif, &["LensMake", "LensManufacturer"]);
    let lens_model = find_field_value(
        &exif,
        &[
            "LensModel",
            "Lens",
            "LensType",
            "LensInfo",
            "LensSpecification",
        ],
    );
    let film_sim = find_field_value(
        &exif,
        &[
            "FilmMode",
            "FilmSimulation",
            "FilmSimulationName",
            "PictureMode",
        ],
    )
    .or_else(|| find_fujifilm_film_simulation(&exif));

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
    for name in names {
        if let Some(value) = exif.fields().find_map(|field| {
            let tag_name = field.tag.to_string();
            if name.eq_ignore_ascii_case(&tag_name) {
                field_value_to_string(field, exif)
            } else {
                None
            }
        }) {
            return Some(value);
        }
    }
    None
}

fn field_value_to_string(field: &Field, exif: &exif::Exif) -> Option<String> {
    match &field.value {
        ExifValue::Ascii(values) => values
            .iter()
            .find_map(|raw| {
                let text = String::from_utf8_lossy(raw)
                    .trim_matches('\0')
                    .trim()
                    .to_string();
                if text.is_empty() {
                    None
                } else {
                    Some(text)
                }
            })
            .or_else(|| display_value_string(field, exif)),
        _ => display_value_string(field, exif),
    }
}

fn display_value_string(field: &Field, exif: &exif::Exif) -> Option<String> {
    let value = field.display_value().with_unit(exif).to_string();
    let value = value.trim().trim_matches('"').to_string();
    if value.is_empty() {
        None
    } else {
        Some(value)
    }
}

fn find_fujifilm_film_simulation(exif: &exif::Exif) -> Option<String> {
    let maker_note = exif.fields().find_map(|field| {
        if !field.tag.to_string().eq_ignore_ascii_case("MakerNote") {
            return None;
        }
        match &field.value {
            ExifValue::Undefined(bytes, _) | ExifValue::Byte(bytes) => Some(bytes.as_slice()),
            _ => None,
        }
    })?;

    let code = parse_fujifilm_film_mode_code(maker_note)?;
    let name = map_fujifilm_film_mode(code)?;
    Some(name.to_string())
}

fn parse_fujifilm_film_mode_code(maker_note: &[u8]) -> Option<u16> {
    if maker_note.len() < 16 || !maker_note.starts_with(FUJIFILM_MAKER_NOTE_PREFIX) {
        return None;
    }

    let mut offsets = Vec::new();
    if let Some(offset) = read_le_u32(maker_note, 12) {
        offsets.push(offset as usize);
    }
    if let Some(offset) = read_le_u32(maker_note, 8) {
        offsets.push(offset as usize);
    }

    for offset in offsets {
        if let Some(code) = parse_fujifilm_ifd_short_tag(maker_note, offset, FUJIFILM_TAG_FILM_MODE)
        {
            return Some(code);
        }
    }

    None
}

fn parse_fujifilm_ifd_short_tag(data: &[u8], ifd_offset: usize, target_tag: u16) -> Option<u16> {
    let entry_count = read_le_u16(data, ifd_offset)? as usize;
    let entries_start = ifd_offset.checked_add(2)?;

    for index in 0..entry_count {
        let entry_offset = entries_start.checked_add(index.checked_mul(12)?)?;
        if entry_offset.checked_add(12)? > data.len() {
            break;
        }

        let tag = read_le_u16(data, entry_offset)?;
        if tag != target_tag {
            continue;
        }

        let field_type = read_le_u16(data, entry_offset + 2)?;
        let count = read_le_u32(data, entry_offset + 4)? as usize;
        if count == 0 {
            return None;
        }

        if field_type == 3 {
            if count <= 2 {
                return read_le_u16(data, entry_offset + 8);
            }

            let value_offset = read_le_u32(data, entry_offset + 8)? as usize;
            return read_le_u16(data, value_offset);
        }

        if field_type == 4 && count == 1 {
            let value = read_le_u32(data, entry_offset + 8)?;
            return Some((value & 0xffff) as u16);
        }

        return None;
    }

    None
}

fn read_le_u16(data: &[u8], offset: usize) -> Option<u16> {
    let bytes: [u8; 2] = data.get(offset..offset.checked_add(2)?)?.try_into().ok()?;
    Some(u16::from_le_bytes(bytes))
}

fn read_le_u32(data: &[u8], offset: usize) -> Option<u32> {
    let bytes: [u8; 4] = data.get(offset..offset.checked_add(4)?)?.try_into().ok()?;
    Some(u32::from_le_bytes(bytes))
}

fn map_fujifilm_film_mode(code: u16) -> Option<&'static str> {
    match code {
        0x000 => Some("PROVIA"),
        0x100 => Some("STUDIO-PORTRAIT"),
        0x110 => Some("STUDIO-PORTRAIT-ENHANCED-SATURATION"),
        0x120 => Some("ASTIA"),
        0x130 => Some("STUDIO-PORTRAIT-INCREASED-SHARPNESS"),
        0x200 => Some("VELVIA"),
        0x300 => Some("STUDIO-PORTRAIT-EX"),
        0x400 => Some("VELVIA"),
        0x500 => Some("PRO-NEG-STD"),
        0x501 => Some("PRO-NEG-HI"),
        0x600 => Some("CLASSIC-CHROME"),
        0x700 => Some("ETERNA"),
        0x800 => Some("CLASSIC-NEG"),
        0x900 => Some("BLEACH-BYPASS"),
        0xA00 => Some("NOSTALGIC-NEG"),
        0xB00 => Some("REALA ACE"),
        _ => None,
    }
}

fn parse_date(input: &str) -> Option<DateTime<Local>> {
    let normalized = input.trim();

    let candidates = [
        "%Y:%m:%d %H:%M:%S",
        "%Y:%m:%d %H:%M:%S%:z",
        "%Y:%m:%d %H:%M:%S%.f%:z",
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

#[cfg(test)]
mod tests {
    use super::{
        map_fujifilm_film_mode, normalize_film_simulation_from_saturation,
        normalize_film_simulation_name, parse_fujifilm_film_mode_code,
        pick_film_simulation_from_json,
    };
    use serde_json::json;

    #[test]
    fn parse_fujifilm_film_mode_from_maker_note() {
        // Minimal Fujifilm MakerNote:
        // "FUJIFILM" + 0x0000000c + first IFD offset(0x1a)
        // IFD[1]: tag=0x1401(FilmMode), type=SHORT, count=1, value=0x0700(ETERNA)
        let mut note = vec![0u8; 26 + 2 + 12 + 4];
        note[0..8].copy_from_slice(b"FUJIFILM");
        note[8..12].copy_from_slice(&12u32.to_le_bytes());
        note[12..16].copy_from_slice(&26u32.to_le_bytes());
        note[26..28].copy_from_slice(&1u16.to_le_bytes());

        let entry = 28usize;
        note[entry..entry + 2].copy_from_slice(&0x1401u16.to_le_bytes());
        note[entry + 2..entry + 4].copy_from_slice(&3u16.to_le_bytes());
        note[entry + 4..entry + 8].copy_from_slice(&1u32.to_le_bytes());
        note[entry + 8..entry + 10].copy_from_slice(&0x0700u16.to_le_bytes());

        let code = parse_fujifilm_film_mode_code(&note);
        assert_eq!(code, Some(0x0700));
    }

    #[test]
    fn map_fujifilm_film_mode_name() {
        assert_eq!(map_fujifilm_film_mode(0x000), Some("PROVIA"));
        assert_eq!(map_fujifilm_film_mode(0x700), Some("ETERNA"));
        assert_eq!(map_fujifilm_film_mode(0xB00), Some("REALA ACE"));
        assert_eq!(map_fujifilm_film_mode(0xFFFF), None);
    }

    #[test]
    fn normalize_film_simulation_name_from_text() {
        assert_eq!(
            normalize_film_simulation_name("F0/Standard (Provia)", true).as_deref(),
            Some("PROVIA")
        );
        assert_eq!(
            normalize_film_simulation_name("Reala ACE", true).as_deref(),
            Some("REALA ACE")
        );
        assert_eq!(
            normalize_film_simulation_name("Aperture-priority AE", false),
            None
        );
    }

    #[test]
    fn normalize_film_simulation_from_saturation_values() {
        assert_eq!(
            normalize_film_simulation_from_saturation("Acros").as_deref(),
            Some("ACROS")
        );
        assert_eq!(
            normalize_film_simulation_from_saturation("Acros Red Filter").as_deref(),
            Some("ACROS+ R FILTER")
        );
        assert_eq!(
            normalize_film_simulation_from_saturation("B&W Green Filter").as_deref(),
            Some("MONOCHROME+ G FILTER")
        );
        assert_eq!(
            normalize_film_simulation_from_saturation("B&W Sepia").as_deref(),
            Some("SEPIA")
        );
        assert_eq!(
            normalize_film_simulation_from_saturation("Monochrome +R").as_deref(),
            Some("MONOCHROME+ R FILTER")
        );
        assert_eq!(normalize_film_simulation_from_saturation("+2 (high)"), None);
    }

    #[test]
    fn pick_film_simulation_prefers_saturation_over_film_mode() {
        let json = json!({
            "Saturation": "Acros Red Filter",
            "FilmMode": "F0/Standard (Provia)"
        });
        assert_eq!(
            pick_film_simulation_from_json(&json).as_deref(),
            Some("ACROS+ R FILTER")
        );
    }

    #[test]
    fn pick_film_simulation_uses_film_mode_when_saturation_not_bw_family() {
        let json = json!({
            "Saturation": "+2 (high)",
            "FilmMode": "F0/Standard (Provia)"
        });
        assert_eq!(
            pick_film_simulation_from_json(&json).as_deref(),
            Some("PROVIA")
        );
    }
}
