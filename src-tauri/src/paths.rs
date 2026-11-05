//! Where Murmur keeps user data: %APPDATA%\<productName> on Windows,
//! ~/Library/Application Support/<productName> on macOS.

use std::path::PathBuf;

use anyhow::{Context, Result};
use tauri::AppHandle;

pub fn data_dir(app: &AppHandle) -> Result<PathBuf> {
    data_dir_named(&app.package_info().name)
}

/// Same as `data_dir` for code that has no AppHandle, such as tests. On
/// macOS: ~/Library/Application Support/<productName>.
#[cfg(target_os = "macos")]
pub fn data_dir_named(app_name: &str) -> Result<PathBuf> {
    let home = std::env::var_os("HOME").context("HOME is not set")?;
    Ok(PathBuf::from(home)
        .join("Library/Application Support")
        .join(app_name))
}

/// Same as `data_dir` for code that has no AppHandle, such as tests.
#[cfg(windows)]
pub fn data_dir_named(app_name: &str) -> Result<PathBuf> {
    let appdata = std::env::var_os("APPDATA").context("APPDATA is not set")?;
    Ok(PathBuf::from(appdata).join(app_name))
}

pub fn models_dir(app: &AppHandle) -> Result<PathBuf> {
    Ok(data_dir(app)?.join("models"))
}

pub fn icons_dir(app: &AppHandle) -> Result<PathBuf> {
    Ok(data_dir(app)?.join("icons"))
}

/// Dictation audio, written only while "Keep audio clips" is on.
pub fn clips_dir(app: &AppHandle) -> Result<PathBuf> {
    Ok(data_dir(app)?.join("clips"))
}

/// productName from tauri.conf.json, for tests that have no AppHandle.
#[cfg(test)]
pub fn product_name() -> String {
    let conf = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tauri.conf.json"),
    )
    .unwrap();
    let json: serde_json::Value = serde_json::from_str(&conf).unwrap();
    json["productName"].as_str().unwrap().to_string()
}
