#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use chrono::{DateTime, Local, Utc};
use fphoto_renamer_core::{
    apply_plan_with_options, generate_plan, load_config, render_preview_sample, save_config,
    undo_last, validate_template, AppConfig, ApplyOptions, MetadataSource, PhotoMetadata,
    PlanOptions, RenamePlan,
};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tauri::path::BaseDirectory;
use tauri::Manager;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PlanRequest {
    jpg_input: String,
    raw_input: Option<String>,
    recursive: bool,
    include_hidden: bool,
    template: String,
    #[serde(default = "default_true")]
    dedupe_same_make: bool,
    exclusions: Vec<String>,
    max_filename_len: Option<usize>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SampleRequest {
    template: String,
    #[serde(default = "default_true")]
    dedupe_same_make: bool,
    exclusions: Vec<String>,
    metadata: fphoto_renamer_core::PhotoMetadata,
    extension_with_dot: String,
    max_filename_len: Option<usize>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct FixedSampleRequest {
    template: String,
    #[serde(default = "default_true")]
    dedupe_same_make: bool,
    exclusions: Vec<String>,
    max_filename_len: Option<usize>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct GuiSettingsResponse {
    template: String,
    exclusions: Vec<String>,
    backup_originals: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SaveGuiSettingsRequest {
    template: String,
    exclusions: Vec<String>,
    #[serde(default)]
    backup_originals: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ApplyRequest {
    plan: RenamePlan,
    #[serde(default)]
    backup_originals: bool,
}

struct AppState {
    launched_at_utc: DateTime<Utc>,
}

#[tauri::command]
fn generate_plan_cmd(request: PlanRequest) -> Result<RenamePlan, String> {
    let options = PlanOptions {
        jpg_input: request.jpg_input.into(),
        raw_input: request.raw_input.map(Into::into),
        recursive: request.recursive,
        include_hidden: request.include_hidden,
        template: request.template,
        dedupe_same_maker: request.dedupe_same_make,
        exclusions: request.exclusions,
        max_filename_len: request.max_filename_len.unwrap_or(240),
    };

    generate_plan(&options).map_err(|err| err.to_string())
}

#[tauri::command]
fn apply_plan_cmd(request: ApplyRequest) -> Result<fphoto_renamer_core::ApplyResult, String> {
    let options = ApplyOptions {
        backup_originals: request.backup_originals,
    };
    apply_plan_with_options(&request.plan, &options).map_err(|err| err.to_string())
}

#[tauri::command]
fn undo_last_cmd() -> Result<fphoto_renamer_core::UndoResult, String> {
    undo_last().map_err(|err| err.to_string())
}

#[tauri::command]
fn validate_template_cmd(template: String) -> Result<(), String> {
    validate_template(&template).map_err(|err| err.to_string())
}

#[tauri::command]
fn render_sample_cmd(request: SampleRequest) -> Result<String, String> {
    render_preview_sample(
        &request.template,
        request.dedupe_same_make,
        &request.exclusions,
        &request.metadata,
        &request.extension_with_dot,
        request.max_filename_len.unwrap_or(240),
    )
    .map_err(|err| err.to_string())
}

#[tauri::command]
fn render_fixed_sample_cmd(
    state: tauri::State<'_, AppState>,
    request: FixedSampleRequest,
) -> Result<String, String> {
    let metadata = fixed_sample_metadata(state.launched_at_utc.with_timezone(&Local));
    render_preview_sample(
        &request.template,
        request.dedupe_same_make,
        &request.exclusions,
        &metadata,
        ".JPG",
        request.max_filename_len.unwrap_or(240),
    )
    .map_err(|err| err.to_string())
}

#[tauri::command]
fn load_gui_settings_cmd() -> Result<GuiSettingsResponse, String> {
    let config = load_config().map_err(|err| err.to_string())?;
    Ok(GuiSettingsResponse {
        template: config.template,
        exclusions: config.exclude_strings,
        backup_originals: config.backup_originals,
    })
}

#[tauri::command]
fn save_gui_settings_cmd(request: SaveGuiSettingsRequest) -> Result<(), String> {
    let mut config = load_config().unwrap_or_else(|_| AppConfig::default());
    config.template = request.template;
    config.exclude_strings = request.exclusions;
    config.backup_originals = request.backup_originals;
    save_config(&config).map_err(|err| err.to_string())
}

#[tauri::command]
fn pick_folder_cmd(initial: Option<String>) -> Result<Option<String>, String> {
    let mut dialog = rfd::FileDialog::new();
    if let Some(initial_path) = initial
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
    {
        let path = PathBuf::from(initial_path);
        if path.exists() {
            dialog = dialog.set_directory(path);
        }
    }

    let picked = dialog.pick_folder();
    Ok(picked.map(|p| p.to_string_lossy().to_string()))
}

#[tauri::command]
fn normalize_to_folder_cmd(path: String) -> Result<String, String> {
    let trimmed = path.trim();
    if trimmed.is_empty() {
        return Err("パスが空です".to_string());
    }

    let path = PathBuf::from(trimmed);
    if path.is_dir() {
        return Ok(path.to_string_lossy().to_string());
    }

    if path.is_file() {
        if let Some(parent) = path.parent() {
            return Ok(parent.to_string_lossy().to_string());
        }
    }

    Err(format!(
        "存在するフォルダまたはファイルを指定してください: {}",
        trimmed
    ))
}

fn main() {
    tauri::Builder::default()
        .manage(AppState {
            launched_at_utc: Utc::now(),
        })
        .setup(|app| {
            configure_exiftool_path(app.handle());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            generate_plan_cmd,
            apply_plan_cmd,
            undo_last_cmd,
            validate_template_cmd,
            render_sample_cmd,
            render_fixed_sample_cmd,
            load_gui_settings_cmd,
            save_gui_settings_cmd,
            pick_folder_cmd,
            normalize_to_folder_cmd
        ])
        .run(tauri::generate_context!())
        .expect("Tauriアプリの起動に失敗しました");
}

fn configure_exiftool_path<R: tauri::Runtime>(app: &tauri::AppHandle<R>) {
    if std::env::var_os("FPHOTO_EXIFTOOL_PATH").is_some() {
        return;
    }

    let mut candidates = Vec::<PathBuf>::new();

    if let Ok(path) = app
        .path()
        .resolve(resource_rel_path(), BaseDirectory::Resource)
    {
        candidates.push(path);
    }

    let dev_path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(resource_rel_path());
    candidates.push(dev_path);

    for candidate in candidates {
        if candidate.exists() {
            std::env::set_var("FPHOTO_EXIFTOOL_PATH", candidate);
            return;
        }
    }
}

fn resource_rel_path() -> &'static str {
    #[cfg(target_os = "windows")]
    {
        return "resources/bin/windows/exiftool.exe";
    }

    #[cfg(target_os = "macos")]
    {
        return "resources/bin/macos/exiftool";
    }

    #[cfg(target_os = "linux")]
    {
        return "resources/bin/linux/exiftool";
    }

    #[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
    {
        return "resources/bin/exiftool";
    }
}

fn default_true() -> bool {
    true
}

fn fixed_sample_metadata(launched_at: DateTime<Local>) -> PhotoMetadata {
    PhotoMetadata {
        source: MetadataSource::JpgExif,
        date: launched_at,
        camera_make: Some("FUJIFILM".to_string()),
        camera_model: Some("X-H2".to_string()),
        lens_make: Some("FUJIFILM".to_string()),
        lens_model: Some("XF35mm F1.4 R".to_string()),
        film_sim: Some("PROVIA".to_string()),
        original_name: "DSC00001".to_string(),
        jpg_path: PathBuf::from("DSC00001.JPG"),
    }
}
