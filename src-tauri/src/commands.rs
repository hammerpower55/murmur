//! Every #[tauri::command]. Errors come back as one line for the UI.

use std::sync::Arc;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use tauri::{AppHandle, State};
use tauri_plugin_opener::OpenerExt;

use crate::db::insights::Insights;
use crate::db::stats::HomeSummary;
use crate::db::transcripts::TranscriptRow;
use crate::db::Db;
use crate::inject::icons;
use crate::models::manager::{ModelManager, ModelsView};
use crate::{paths, settings};

type Models<'a> = State<'a, Arc<ModelManager>>;

fn line(err: anyhow::Error) -> String {
    log::warn!("{err:#}");
    err.to_string()
}

#[tauri::command]
pub fn list_models(models: Models) -> ModelsView {
    models.view()
}

#[tauri::command]
pub fn download_model(models: Models, id: String) -> Result<(), String> {
    models.start_download(&id).map_err(line)
}

#[tauri::command]
pub fn cancel_download(models: Models, id: String) {
    models.cancel_download(&id);
}

#[tauri::command]
pub fn delete_model(models: Models, id: String) -> Result<u64, String> {
    models.delete(&id).map_err(line)
}

#[tauri::command]
pub fn use_model(models: Models, id: String) -> Result<(), String> {
    models.activate(&id).map_err(line)
}

/// First launch, or nothing installed: show onboarding.
#[tauri::command]
pub fn onboarding_needed(app: AppHandle, models: Models) -> bool {
    !settings::onboarded(&app) || !models.has_installed_model()
}

#[tauri::command]
pub fn finish_onboarding(app: AppHandle) -> Result<(), String> {
    settings::set_onboarded(&app).map_err(line)
}

/// Opens the default microphone briefly. Fails when there is no microphone
/// or Windows privacy settings block desktop apps from using it.
#[tauri::command]
pub fn check_microphone() -> Result<(), String> {
    let blocked =
        "Murmur cannot use the microphone. Allow desktop apps to use it in Windows settings.";
    let device = cpal::default_host()
        .default_input_device()
        .ok_or("Connect a microphone to start dictating.")?;
    let config = device.default_input_config().map_err(|_| blocked)?;
    let stream = device
        .build_input_stream::<f32, _, _>(config.into(), |_, _| {}, |_| {}, None)
        .map_err(|_| blocked)?;
    stream.play().map_err(|_| blocked)?;
    Ok(())
}

#[tauri::command]
pub fn open_microphone_settings(app: AppHandle) -> Result<(), String> {
    app.opener()
        .open_url("ms-settings:privacy-microphone", None::<&str>)
        .map_err(|e| e.to_string())
}

type History<'a> = State<'a, Arc<Db>>;

#[tauri::command]
pub fn list_transcripts(
    db: History,
    query: String,
    limit: u32,
    offset: u32,
) -> Result<Vec<TranscriptRow>, String> {
    db.list_transcripts(&query, limit.min(500), offset)
        .map_err(line)
}

#[tauri::command]
pub fn delete_transcript(db: History, id: i64) -> Result<bool, String> {
    db.delete_transcript(id).map_err(line)
}

#[tauri::command]
pub fn clear_history(db: History) -> Result<(), String> {
    db.clear_history().map_err(line)
}

#[tauri::command]
pub fn home_summary(db: History) -> Result<HomeSummary, String> {
    db.home_summary(chrono::Local::now().date_naive())
        .map_err(line)
}

/// The app's icon as a data URL, if Murmur has saved one.
#[tauri::command]
pub fn app_icon(app: AppHandle, app_name: String) -> Option<String> {
    icons::data_url(&paths::icons_dir(&app).ok()?, &app_name)
}

#[tauri::command]
pub fn insights(app: AppHandle, db: History) -> Result<Insights, String> {
    db.insights(
        chrono::Local::now().date_naive(),
        settings::typing_wpm(&app),
    )
    .map_err(line)
}
