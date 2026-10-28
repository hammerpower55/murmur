mod audio;
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

use std::path::PathBuf;
use std::sync::Arc;

use tauri::{AppHandle, Manager};
use tauri_plugin_log::{Target, TargetKind};

use db::Db;
use engine::host::EngineHost;
use models::manager::ModelManager;

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
        ])
        .setup(|app| {
            let handle = app.handle().clone();
            // Window title comes from productName so the name lives in one place.
            if let Some(window) = app.get_webview_window("main") {
                window.set_title(&app.package_info().name)?;
            }

            engine::parakeet::set_runtime_path(onnxruntime_path(&handle));
            let host = EngineHost::new();
            let models =
                ModelManager::new(handle.clone(), paths::models_dir(&handle)?, host.clone());
            models.load_at_startup();
            app.manage(models);

            let db = Arc::new(Db::open(&paths::data_dir(&handle)?.join(db::FILE))?);
            app.manage(db.clone());

            dictation::spawn(handle.clone(), host, db, paths::icons_dir(&handle)?)?;
            Ok(())
        })
        .run(tauri::generate_context!());

    if let Err(err) = result {
        eprintln!("failed to start: {err}");
        std::process::exit(1);
    }
}
