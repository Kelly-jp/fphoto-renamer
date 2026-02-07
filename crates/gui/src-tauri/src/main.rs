#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use fphoto_renamer_core::{
    apply_plan, generate_plan, render_preview_sample, undo_last, validate_template, PlanOptions,
    RenamePlan,
};
use serde::Deserialize;
use std::path::PathBuf;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PlanRequest {
    jpg_input: String,
    raw_input: Option<String>,
    recursive: bool,
    include_hidden: bool,
    template: String,
    exclusions: Vec<String>,
    max_filename_len: Option<usize>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SampleRequest {
    template: String,
    exclusions: Vec<String>,
    metadata: fphoto_renamer_core::PhotoMetadata,
    extension_with_dot: String,
    max_filename_len: Option<usize>,
}

#[tauri::command]
fn generate_plan_cmd(request: PlanRequest) -> Result<RenamePlan, String> {
    let options = PlanOptions {
        jpg_input: request.jpg_input.into(),
        raw_input: request.raw_input.map(Into::into),
        recursive: request.recursive,
        include_hidden: request.include_hidden,
        template: request.template,
        exclusions: request.exclusions,
        max_filename_len: request.max_filename_len.unwrap_or(240),
    };

    generate_plan(&options).map_err(|err| err.to_string())
}

#[tauri::command]
fn apply_plan_cmd(plan: RenamePlan) -> Result<fphoto_renamer_core::ApplyResult, String> {
    apply_plan(&plan).map_err(|err| err.to_string())
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
        &request.exclusions,
        &request.metadata,
        &request.extension_with_dot,
        request.max_filename_len.unwrap_or(240),
    )
    .map_err(|err| err.to_string())
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
        .invoke_handler(tauri::generate_handler![
            generate_plan_cmd,
            apply_plan_cmd,
            undo_last_cmd,
            validate_template_cmd,
            render_sample_cmd,
            pick_folder_cmd,
            normalize_to_folder_cmd
        ])
        .run(tauri::generate_context!())
        .expect("Tauriアプリの起動に失敗しました");
}
