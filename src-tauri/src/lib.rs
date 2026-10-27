mod audio;
mod dictation;
mod hotkey;

use tauri::Manager;
use tauri_plugin_log::{Target, TargetKind};

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let result = tauri::Builder::default()
        .plugin(
            tauri_plugin_log::Builder::new()
                .clear_targets()
                .target(Target::new(TargetKind::Stdout))
                .level(log::LevelFilter::Info)
                .build(),
        )
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            // Window title comes from productName so the name lives in one place.
            if let Some(window) = app.get_webview_window("main") {
                window.set_title(&app.package_info().name)?;
            }
            dictation::spawn(app.handle().clone())?;
            Ok(())
        })
        .run(tauri::generate_context!());

    if let Err(err) = result {
        eprintln!("failed to start: {err}");
        std::process::exit(1);
    }
}
