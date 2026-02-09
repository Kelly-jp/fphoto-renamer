use chrono::{DateTime, Local};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum MetadataSource {
    JpgExif,
    Xmp,
    RawExif,
    XmpAndRawExif,
    FallbackFileModified,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PhotoMetadata {
    pub source: MetadataSource,
    pub date: DateTime<Local>,
    pub camera_make: Option<String>,
    pub camera_model: Option<String>,
    pub lens_make: Option<String>,
    pub lens_model: Option<String>,
    pub film_sim: Option<String>,
    pub original_name: String,
    pub jpg_path: PathBuf,
}

impl PhotoMetadata {
    pub fn normalized_camera_make(&self) -> Option<&str> {
        self.camera_make
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
    }

    pub fn normalized_lens_make(&self) -> Option<&str> {
        self.lens_make
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
    }
}

#[derive(Debug, Clone, Default)]
pub struct PartialMetadata {
    pub date: Option<DateTime<Local>>,
    pub camera_make: Option<String>,
    pub camera_model: Option<String>,
    pub lens_make: Option<String>,
    pub lens_model: Option<String>,
    pub film_sim: Option<String>,
}

impl PartialMetadata {
    pub fn merge_missing_from(&mut self, fallback: &PartialMetadata) {
        if self.date.is_none() {
            self.date = fallback.date;
        }
        if self.camera_make.is_none() {
            self.camera_make = fallback.camera_make.clone();
        }
        if self.camera_model.is_none() {
            self.camera_model = fallback.camera_model.clone();
        }
        if self.lens_make.is_none() {
            self.lens_make = fallback.lens_make.clone();
        }
        if self.lens_model.is_none() {
            self.lens_model = fallback.lens_model.clone();
        }
        if self.film_sim.is_none() {
            self.film_sim = fallback.film_sim.clone();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{PartialMetadata, PhotoMetadata};
    use crate::metadata::MetadataSource;
    use chrono::Local;
    use std::path::PathBuf;

    #[test]
    fn normalized_make_trims_and_drops_empty() {
        let mut meta = PhotoMetadata {
            source: MetadataSource::JpgExif,
            date: Local::now(),
            camera_make: Some("  FUJIFILM  ".to_string()),
            camera_model: None,
            lens_make: Some("   ".to_string()),
            lens_model: None,
            film_sim: None,
            original_name: "IMG_0001".to_string(),
            jpg_path: PathBuf::from("/tmp/IMG_0001.JPG"),
        };

        assert_eq!(meta.normalized_camera_make(), Some("FUJIFILM"));
        assert_eq!(meta.normalized_lens_make(), None);

        meta.camera_make = Some(" ".to_string());
        assert_eq!(meta.normalized_camera_make(), None);
    }

    #[test]
    fn merge_missing_from_only_fills_missing_fields() {
        let now = Local::now();
        let mut base = PartialMetadata {
            date: Some(now),
            camera_make: Some("SONY".to_string()),
            camera_model: None,
            lens_make: None,
            lens_model: Some("35mm F2".to_string()),
            film_sim: None,
        };
        let fallback = PartialMetadata {
            date: None,
            camera_make: Some("FUJIFILM".to_string()),
            camera_model: Some("X-T5".to_string()),
            lens_make: Some("FUJIFILM".to_string()),
            lens_model: Some("XF16-55".to_string()),
            film_sim: Some("CLASSIC CHROME".to_string()),
        };

        base.merge_missing_from(&fallback);
        assert_eq!(base.date, Some(now));
        assert_eq!(base.camera_make.as_deref(), Some("SONY"));
        assert_eq!(base.camera_model.as_deref(), Some("X-T5"));
        assert_eq!(base.lens_make.as_deref(), Some("FUJIFILM"));
        assert_eq!(base.lens_model.as_deref(), Some("35mm F2"));
        assert_eq!(base.film_sim.as_deref(), Some("CLASSIC CHROME"));
    }
}
