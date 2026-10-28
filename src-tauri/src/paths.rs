//! Where Murmur keeps user data: %APPDATA%\<productName>.

use std::path::PathBuf;

use anyhow::{Context, Result};
use tauri::AppHandle;

pub fn data_dir(app: &AppHandle) -> Result<PathBuf> {
    data_dir_named(&app.package_info().name)
}

/// Same as `data_dir` for code that has no AppHandle, such as tests.
pub fn data_dir_named(app_name: &str) -> Result<PathBuf> {
    let appdata = std::env::var_os("APPDATA").context("APPDATA is not set")?;
    Ok(PathBuf::from(appdata).join(app_name))
}

pub fn models_dir(app: &AppHandle) -> Result<PathBuf> {
    Ok(data_dir(app)?.join("models"))
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
