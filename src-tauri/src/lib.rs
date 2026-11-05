mod audio;
mod chat;
mod commands;
mod db;
mod dictation;
mod engine;
mod hotkey;
mod inject;
mod models;
mod overlay;
mod paths;
mod settings;
mod sound;
mod text;
mod tray;

use std::path::PathBuf;
use std::sync::{Arc, RwLock};

use tauri::{AppHandle, Manager, WindowEvent};
use tauri_plugin_autostart::{MacosLauncher, ManagerExt};
use tauri_plugin_log::{Target, TargetKind};

use db::Db;
use engine::host::EngineHost;
use engine::fixer::Fixer;
use models::manager::ModelManager;
use text::store::TextStore;

/// Launch at login starts Murmur with this flag: tray only, no window.
const HIDDEN_FLAG: &str = "--hidden";

/// onnxruntime.dll ships next to the app as a bundle resource. Dev builds
/// fall back to the copy scripts/fetch_onnxruntime.ps1 puts in vendor/.
fn onnxruntime_path(app: &AppHandle) -> PathBuf {
    let bundled = app
        .path()
        .resource_dir()
        .map(|dir| dir.join("onnxruntime.dll"))
        .ok()
        .filter(|p| p.is_file());
    #[cfg(debug_assertions)]
    let bundled = bundled.or_else(|| {
        Some(
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("vendor/onnxruntime-win-arm64-1.31.0/lib/onnxruntime.dll"),
        )
    });
    bundled.unwrap_or_else(|| PathBuf::from("onnxruntime.dll"))
}

/// Qualcomm's QNN plugin ships in a qnn folder next to the app. Dev builds
/// fall back to the copy scripts/fetch_qnn.ps1 puts in vendor/.
fn qnn_dir(app: &AppHandle) -> PathBuf {
    let bundled = app
        .path()
        .resource_dir()
        .map(|dir| dir.join("qnn"))
        .ok()
        .filter(|p| p.is_dir());
    #[cfg(debug_assertions)]
    let bundled = bundled.or_else(|| {
        Some(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("vendor/onnxruntime-qnn-2.7.0"))
    });
    bundled.unwrap_or_else(|| PathBuf::from("qnn"))
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let result = tauri::Builder::default()
        .plugin(
            tauri_plugin_log::Builder::new()
                .clear_targets()
                .target(Target::new(TargetKind::Stdout))
                .level(log::LevelFilter::Info)
                // whisper.cpp logs every load step at info.
                .level_for("whisper_rs", log::LevelFilter::Warn)
                .build(),
        )
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_store::Builder::new().build())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_autostart::init(
            MacosLauncher::LaunchAgent,
            Some(vec![HIDDEN_FLAG]),
        ))
        .invoke_handler(tauri::generate_handler![
            commands::list_models,
            commands::download_model,
            commands::cancel_download,
            commands::delete_model,
            commands::use_model,
            commands::onboarding_needed,
            commands::finish_onboarding,
            commands::check_microphone,
            commands::open_microphone_settings,
            commands::list_transcripts,
            commands::delete_transcript,
            commands::clear_history,
            commands::home_summary,
            commands::app_icon,
            commands::insights,
            commands::get_settings,
            commands::set_settings,
            commands::list_microphones,
            commands::test_microphone,
            commands::hotkey_recording,
            commands::overlay_style,
            commands::open_data_folder,
            commands::open_licences,
            commands::app_info,
            commands::text_data,
            commands::set_dictionary,
            commands::set_snippets,
            commands::set_roman_urdu,
            commands::reset_roman_urdu,
            commands::export_csv,
            commands::import_csv,
            commands::chat_send,
            commands::chat_stop,
        ])
        .on_window_event(|window, event| {

            // Closing the main window hides it to the tray when that is on;
            // otherwise it quits, since the overlay window never closes.
            if let (WindowEvent::CloseRequested { api, .. }, "main") = (event, window.label()) {
                let s = settings::current(window.app_handle());
                if s.close_to_tray && s.show_in_tray {
                    api.prevent_close();
                    let _ = window.hide();
                } else {
                    window.app_handle().exit(0);
                }
            }
        })
        .setup(|app| {
            let handle = app.handle().clone();
            // Window title comes from productName so the name lives in one place.
            if let Some(window) = app.get_webview_window("main") {
                window.set_title(&app.package_info().name)?;
            }

            let prefs = settings::load(&handle);
            let shared: settings::Shared = Arc::new(RwLock::new(prefs.clone()));
            app.manage(shared.clone());

            // Launch at login lives in the registry; keep it in step with the
            // setting in case the user changed it in Windows.
            let autostart = handle.autolaunch();
            if autostart.is_enabled().unwrap_or(false) != prefs.launch_at_login {
                let synced = if prefs.launch_at_login {
                    autostart.enable()
                } else {
                    autostart.disable()
                };
                if let Err(err) = synced {
                    log::warn!("could not update launch at login: {err}");
                }
            }

            engine::onnx::set_runtime_path(onnxruntime_path(&handle));
            engine::onnx::set_qnn_dir(qnn_dir(&handle));
            // Loading the runtime and looking for an NPU takes a moment; do it
            // now, off the main thread, so the Models page does not wait.
            let _ = std::thread::Builder::new()
                .name("murmur-npu-check".into())
                .spawn(engine::onnx::npu_available);
            let host = EngineHost::new();
            let fixer = Fixer::new();
            let models = ModelManager::new(
                handle.clone(),
                paths::models_dir(&handle)?,
                host.clone(),
                fixer.clone(),
            );
            models.load_at_startup();
            app.manage(models);
            app.manage(fixer.clone());
            app.manage(chat::Chat::new());

            let data_dir = paths::data_dir(&handle)?;
            let db_path = data_dir.join(db::FILE);
            // Debug builds can be pointed at a fixture (tests/seed_insights.py).
            #[cfg(debug_assertions)]
            let db_path = std::env::var_os("MURMUR_HISTORY_DB")
                .map(PathBuf::from)
                .unwrap_or(db_path);
            log::info!("history: {}", db_path.display());
            let db = Arc::new(Db::open(&db_path)?);
            app.manage(db.clone());

            let texts = Arc::new(TextStore::open(data_dir));
            app.manage(texts.clone());

            let dirs = dictation::Dirs {
                icons: paths::icons_dir(&handle)?,
                clips: paths::clips_dir(&handle)?,
            };
            let dictation = dictation::spawn(handle.clone(), host, fixer, db, dirs, shared, texts)?;
            app.manage(dictation);

            if let Err(err) = tray::setup(&handle, prefs.show_in_tray) {
                log::warn!("no tray icon: {err:#}");
            }

            // Started at login: stay in the tray. Without a tray icon there
            // would be no way back to the window, so show it then.
            let hidden = std::env::args().any(|a| a == HIDDEN_FLAG) && prefs.show_in_tray;
            if !hidden {
                tray::show_main(&handle);
            }
            Ok(())
        })
        .run(tauri::generate_context!());

    if let Err(err) = result {
        eprintln!("failed to start: {err}");
        std::process::exit(1);
    }
}
