use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let result = tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            // Window title comes from productName so the name lives in one place.
            if let Some(window) = app.get_webview_window("main") {
                window.set_title(&app.package_info().name)?;
            }
            Ok(())
        })
        .run(tauri::generate_context!());

    if let Err(err) = result {
        eprintln!("failed to start: {err}");
        std::process::exit(1);
    }
}
