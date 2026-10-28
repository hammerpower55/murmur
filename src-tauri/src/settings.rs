//! settings.json in %APPDATA%\Murmur through tauri-plugin-store. The store
//! is given an absolute path so it lands with the rest of the user's data
//! rather than in the plugin's default %APPDATA%\<identifier> folder.

use std::sync::Arc;

use anyhow::{Context, Result};
use serde_json::{json, Value};
use tauri::{AppHandle, Wry};
use tauri_plugin_store::{Store, StoreExt};

use crate::paths;

/// Language settings, fixed until Phase 8. Roman Urdu mixing (default on)
/// forces English decoding so Urdu words stay in Latin script.
pub const LANGUAGE: &str = "en";
pub const ROMAN_URDU_MIXING: bool = true;

const FILE: &str = "settings.json";
const ACTIVE_MODEL: &str = "activeModel";
const ONBOARDED: &str = "onboarded";

fn store(app: &AppHandle) -> Result<Arc<Store<Wry>>> {
    let path = paths::data_dir(app)?.join(FILE);
    app.store(path).context("could not open settings")
}

fn set(app: &AppHandle, key: &str, value: Value) -> Result<()> {
    let s = store(app)?;
    s.set(key, value);
    s.save().context("could not save settings")
}

pub fn active_model(app: &AppHandle) -> Option<String> {
    store(app)
        .ok()?
        .get(ACTIVE_MODEL)?
        .as_str()
        .map(String::from)
}

pub fn set_active_model(app: &AppHandle, id: Option<&str>) -> Result<()> {
    set(app, ACTIVE_MODEL, id.map_or(Value::Null, |id| json!(id)))
}

pub fn onboarded(app: &AppHandle) -> bool {
    store(app)
        .ok()
        .and_then(|s| s.get(ONBOARDED))
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
}

pub fn set_onboarded(app: &AppHandle) -> Result<()> {
    set(app, ONBOARDED, json!(true))
}
