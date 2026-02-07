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
