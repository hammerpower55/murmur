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
