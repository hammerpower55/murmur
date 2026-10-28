//! Every #[tauri::command]. Errors come back as one line for the UI.

use std::sync::Arc;
use std::time::Duration;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_autostart::ManagerExt;
use tauri_plugin_dialog::DialogExt;
use tauri_plugin_opener::OpenerExt;

use crate::audio::capture;
use crate::db::insights::Insights;
use crate::db::stats::HomeSummary;
use crate::db::transcripts::TranscriptRow;
use crate::db::Db;
use crate::dictation::{self, Dictation};
use crate::hotkey::keys;
use crate::inject::icons;
use crate::models::manager::{ModelManager, ModelsView};
use crate::overlay::events::Style;
use crate::settings::Settings;
use crate::text::dictionary::{self, DictEntry};
use crate::text::snippets::{self, Snippet};
use crate::text::store::{TextStore, UrduRow};
use crate::{paths, settings, tray};

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
pub fn delete_transcript(app: AppHandle, db: History, id: i64) -> Result<bool, String> {
    let deleted = db.delete_transcript(id).map_err(line)?;
    if let Ok(dir) = paths::clips_dir(&app) {
        let _ = std::fs::remove_file(dictation::clip_path(&dir, id));
    }
    Ok(deleted)
}

/// Deletes every transcript and every kept audio clip.
#[tauri::command]
pub fn clear_history(app: AppHandle, db: History) -> Result<(), String> {
    db.clear_history().map_err(line)?;
    if let Ok(dir) = paths::clips_dir(&app) {
        if dir.exists() {
            std::fs::remove_dir_all(&dir)
                .map_err(|_| "Could not delete the saved audio clips.".to_string())?;
        }
    }
    Ok(())
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
        settings::current(&app).typing_wpm as f64,
    )
    .map_err(line)
}

// Settings

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingsView {
    settings: Settings,
    defaults: Settings,
}

#[tauri::command]
pub fn get_settings(app: AppHandle) -> SettingsView {
    SettingsView {
        settings: settings::current(&app),
        defaults: Settings::default(),
    }
}

/// Saves the whole record and applies what changed. Returns what was saved.
#[tauri::command]
pub fn set_settings(
    app: AppHandle,
    shared: State<settings::Shared>,
    dictation: State<Dictation>,
    models: Models,
    next: Settings,
) -> Result<Settings, String> {
    next.validate().map_err(|e| e.to_string())?;
    let previous = settings::current(&app);
    if next == previous {
        return Ok(next);
    }
    if next.launch_at_login != previous.launch_at_login {
        let autostart = app.autolaunch();
        let result = if next.launch_at_login {
            autostart.enable()
        } else {
            autostart.disable()
        };
        result.map_err(|e| {
            log::warn!("launch at login: {e}");
            "Could not change launch at login.".to_string()
        })?;
    }
    settings::save(&app, &next).map_err(line)?;
    if let Ok(mut s) = shared.write() {
        *s = next.clone();
    }
    dictation.apply(&next);
    if next.show_in_tray != previous.show_in_tray {
        tray::set_visible(&app, next.show_in_tray);
    }
    if next.roman_urdu != previous.roman_urdu {
        models.reload_active();
    }
    if next.typing_wpm != previous.typing_wpm {
        let _ = app.emit("insights-changed", ());
    }
    log::info!("settings saved");
    Ok(next)
}

#[tauri::command]
pub fn list_microphones() -> Vec<String> {
    capture::input_devices()
}

/// Runs the microphone for the meter in Settings; levels arrive as
/// `mic-level` events. Stops on its own after a minute.
#[tauri::command]
pub fn test_microphone(dictation: State<Dictation>, on: bool) {
    dictation.test_microphone(on);
}

/// Swallows the next key combination and returns its key names. None when
/// cancelled with Escape or after 10 seconds.
#[tauri::command]
pub async fn capture_hotkey(dictation: State<'_, Dictation>) -> Result<Option<Vec<String>>, String> {
    let control = dictation.hotkeys().clone();
    tauri::async_runtime::spawn_blocking(move || {
        control
            .capture(Duration::from_secs(10))
            .map(|k| keys::names_of(&k))
    })
    .await
    .map_err(|_| "Could not record the hotkey.".to_string())
}

#[tauri::command]
pub fn overlay_style(dictation: State<Dictation>) -> Style {
    dictation.overlay_style()
}

#[tauri::command]
pub fn open_data_folder(app: AppHandle) -> Result<(), String> {
    let dir = paths::data_dir(&app).map_err(line)?;
    app.opener()
        .open_path(dir.to_string_lossy(), None::<&str>)
        .map_err(|_| "Could not open the data folder.".to_string())
}

/// NOTICE ships next to the app; dev builds read the repository copy.
#[tauri::command]
pub fn open_licences(app: AppHandle) -> Result<(), String> {
    let bundled = app
        .path()
        .resource_dir()
        .map(|d| d.join("NOTICE"))
        .ok()
        .filter(|p| p.is_file());
    #[cfg(debug_assertions)]
    let bundled = bundled
        .or_else(|| Some(std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../NOTICE")));
    let path = bundled.ok_or("The licence file is missing.")?;
    app.opener()
        .open_path(path.to_string_lossy(), Some("notepad.exe"))
        .map_err(|_| "Could not open the licence file.".to_string())
}

#[derive(Serialize)]
pub struct AppInfo {
    name: String,
    version: String,
}

#[tauri::command]
pub fn app_info(app: AppHandle) -> AppInfo {
    let info = app.package_info();
    AppInfo {
        name: info.name.clone(),
        version: info.version.to_string(),
    }
}

// Dictionary, snippets and the Roman Urdu table

type Texts<'a> = State<'a, Arc<TextStore>>;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TextData {
    dictionary: Vec<DictEntry>,
    snippets: Vec<Snippet>,
    roman_urdu: Vec<UrduRow>,
}

#[tauri::command]
pub fn text_data(texts: Texts) -> TextData {
    let t = texts.get();
    TextData {
        dictionary: t.dictionary,
        snippets: t.snippets,
        roman_urdu: t.roman_urdu,
    }
}

#[tauri::command]
pub fn set_dictionary(texts: Texts, entries: Vec<DictEntry>) -> Result<(), String> {
    texts.set_dictionary(entries).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn set_snippets(texts: Texts, snippets: Vec<Snippet>) -> Result<(), String> {
    texts.set_snippets(snippets).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn set_roman_urdu(texts: Texts, rows: Vec<UrduRow>) -> Result<(), String> {
    texts.set_roman_urdu(rows).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn reset_roman_urdu(texts: Texts) -> Result<Vec<UrduRow>, String> {
    texts.reset_roman_urdu().map_err(line)?;
    Ok(texts.get().roman_urdu)
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ListKind {
    Dictionary,
    Snippets,
}

/// Asks where to save, then writes the list as CSV. False if cancelled.
#[tauri::command]
pub async fn export_csv(app: AppHandle, kind: ListKind) -> Result<bool, String> {
    let texts = app.state::<Arc<TextStore>>().get();
    let (csv, name) = match kind {
        ListKind::Dictionary => (dictionary::export_csv(&texts.dictionary), "dictionary.csv"),
        ListKind::Snippets => (snippets::export_csv(&texts.snippets), "snippets.csv"),
    };
    let dialog = app.dialog().clone();
    let picked = tauri::async_runtime::spawn_blocking(move || {
        dialog
            .file()
            .set_file_name(name)
            .add_filter("CSV", &["csv"])
            .blocking_save_file()
    })
    .await
    .map_err(|_| "Could not open the save dialog.".to_string())?;
    let Some(path) = picked.and_then(|p| p.into_path().ok()) else {
        return Ok(false);
    };
    std::fs::write(&path, csv).map_err(|_| "Could not write that file.".to_string())?;
    Ok(true)
}

/// Asks for a CSV file and merges it in: rows with a known phrase replace
/// the old entry. Returns how many rows were read, or None if cancelled.
#[tauri::command]
pub async fn import_csv(app: AppHandle, kind: ListKind) -> Result<Option<usize>, String> {
    let dialog = app.dialog().clone();
    let picked = tauri::async_runtime::spawn_blocking(move || {
        dialog
            .file()
            .add_filter("CSV", &["csv", "txt"])
            .blocking_pick_file()
    })
    .await
    .map_err(|_| "Could not open the file dialog.".to_string())?;
    let Some(path) = picked.and_then(|p| p.into_path().ok()) else {
        return Ok(None);
    };
    let text = std::fs::read_to_string(&path)
        .map_err(|_| "Could not read that file. Save it as UTF-8 CSV.".to_string())?;
    let store = app.state::<Arc<TextStore>>();
    let current = store.get();
    let key = |s: &str| s.split_whitespace().collect::<Vec<_>>().join(" ").to_lowercase();
    match kind {
        ListKind::Dictionary => {
            let rows = dictionary::import_csv(&text).map_err(|e| e.to_string())?;
            let count = rows.len();
            let mut merged = current.dictionary;
            for row in rows {
                merged.retain(|e| key(&e.spoken) != key(&row.spoken));
                merged.push(row);
            }
            store.set_dictionary(merged).map_err(|e| e.to_string())?;
            Ok(Some(count))
        }
        ListKind::Snippets => {
            let rows = snippets::import_csv(&text).map_err(|e| e.to_string())?;
            let count = rows.len();
            let mut merged = current.snippets;
            for row in rows {
                merged.retain(|s| key(&s.trigger) != key(&row.trigger));
                merged.push(row);
            }
            store.set_snippets(merged).map_err(|e| e.to_string())?;
            Ok(Some(count))
        }
    }
}
