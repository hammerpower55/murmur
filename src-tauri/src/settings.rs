//! settings.json in %APPDATA%\Murmur through tauri-plugin-store. The store
//! is given an absolute path so it lands with the rest of the user's data
//! rather than in the plugin's default %APPDATA%\<identifier> folder.
//!
//! User preferences are one `Settings` record, stored as one key per field
//! so the file stays readable. Each key is read on its own: a bad value falls
//! back to its default without resetting anything else. App state that is
//! not a preference (active model, onboarding) has its own keys.

use std::sync::{Arc, RwLock};

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use tauri::{AppHandle, Manager, Wry};
use tauri_plugin_store::{Store, StoreExt};

use crate::hotkey::keys;
use crate::paths;

const FILE: &str = "settings.json";
const ACTIVE_MODEL: &str = "activeModel";
const ONBOARDED: &str = "onboarded";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum OverlayPosition {
    BottomCentre,
    TopCentre,
    BottomRight,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    // General
    pub launch_at_login: bool,
    pub show_in_tray: bool,
    pub close_to_tray: bool,
    // Hotkeys: key names from hotkey::keys, e.g. ["ctrl", "win"].
    pub main_hotkey: Vec<String>,
    pub toggle_hotkey_enabled: bool,
    pub toggle_hotkey: Vec<String>,
    // Microphone: device name, None for the Windows default.
    pub microphone: Option<String>,
    /// Percent, 100 leaves the signal as it is.
    pub input_gain: u32,
    // Models: minutes without a dictation before the model is unloaded.
    pub unload_after_minutes: u32,
    // Language: a language code or "auto".
    pub language: String,
    pub roman_urdu: bool,
    // Overlay
    pub overlay_enabled: bool,
    pub overlay_position: OverlayPosition,
    pub overlay_colour: String,
    // Sounds
    pub sounds_enabled: bool,
    pub sound_volume: u32,
    // Text
    pub remove_fillers: bool,
    pub add_trailing_space: bool,
    pub voice_punctuation: bool,
    /// Lists, corrections, spoken breaks, email and chat layout.
    pub smart_formatting: bool,
    /// Fix misheard words with the fix-up model, when it is downloaded.
    pub fix_misheard_words: bool,
    /// Add names and terms the user corrects to the dictionary.
    pub learn_words: bool,
    pub typing_wpm: u32,
    // Privacy
    pub keep_audio_clips: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            launch_at_login: false,
            show_in_tray: true,
            close_to_tray: true,
            main_hotkey: vec!["ctrl".into(), "win".into()],
            toggle_hotkey_enabled: false,
            toggle_hotkey: vec!["ctrl".into(), "shift".into(), "space".into()],
            microphone: None,
            input_gain: 100,
            unload_after_minutes: 10,
            language: "en".into(),
            roman_urdu: true,
            overlay_enabled: true,
            overlay_position: OverlayPosition::BottomCentre,
            overlay_colour: "#ffffff".into(),
            sounds_enabled: true,
            sound_volume: 60,
            remove_fillers: true,
            add_trailing_space: true,
            voice_punctuation: false,
            smart_formatting: true,
            fix_misheard_words: true,
            learn_words: true,
            typing_wpm: 40,
            keep_audio_clips: false,
        }
    }
}

pub const GAIN_RANGE: (u32, u32) = (25, 400);
pub const TYPING_WPM_RANGE: (u32, u32) = (10, 200);
pub const UNLOAD_CHOICES: [u32; 5] = [0, 5, 10, 30, 60];

fn is_colour(s: &str) -> bool {
    s.len() == 7 && s.starts_with('#') && s[1..].chars().all(|c| c.is_ascii_hexdigit())
}

impl Settings {
    /// Checks a record coming from the UI. Errors are one line for the user.
    pub fn validate(&self) -> Result<()> {
        keys::validate_hotkey(&self.main_hotkey)?;
        keys::validate_hotkey(&self.toggle_hotkey)?;
        if self.toggle_hotkey_enabled
            && keys::same_combo(&self.main_hotkey, &self.toggle_hotkey)
        {
            bail!("Pick a hands free hotkey that differs from the main one.");
        }
        if !(GAIN_RANGE.0..=GAIN_RANGE.1).contains(&self.input_gain) {
            bail!("Input gain must be between 25% and 400%.");
        }
        if !(TYPING_WPM_RANGE.0..=TYPING_WPM_RANGE.1).contains(&self.typing_wpm) {
            bail!("Typing speed must be between 10 and 200 words per minute.");
        }
        if !UNLOAD_CHOICES.contains(&self.unload_after_minutes) {
            bail!("Pick one of the listed idle times.");
        }
        if self.sound_volume > 100 {
            bail!("Volume must be between 0 and 100.");
        }
        if !is_colour(&self.overlay_colour) {
            bail!("Pick a bar colour like #ffffff.");
        }
        let lang = self.language.as_str();
        if !(lang == "auto"
            || (2..=3).contains(&lang.len()) && lang.chars().all(|c| c.is_ascii_lowercase()))
        {
            bail!("Pick a language from the list.");
        }
        Ok(())
    }

    /// The language to ask the engine for. Roman Urdu mixing pins Whisper to
    /// English so Urdu words stay in Latin script.
    pub fn engine_language(&self, whisper: bool) -> Option<String> {
        if whisper && self.roman_urdu {
            return Some("en".into());
        }
        (self.language != "auto").then(|| self.language.clone())
    }
}

/// Reads a stored record, keeping every key that parses on its own.
pub fn from_stored(stored: &Map<String, Value>) -> Settings {
    let defaults = Settings::default();
    let mut merged = match serde_json::to_value(&defaults) {
        Ok(Value::Object(map)) => map,
        _ => return defaults,
    };
    let keys: Vec<String> = merged.keys().cloned().collect();
    for key in keys {
        let Some(value) = stored.get(&key) else {
            continue;
        };
        let mut candidate = merged.clone();
        candidate.insert(key.clone(), value.clone());
        let ok = serde_json::from_value::<Settings>(Value::Object(candidate))
            .is_ok_and(|s| s.validate().is_ok());
        if ok {
            merged.insert(key, value.clone());
        } else {
            log::warn!("ignoring bad setting {key}: {value}");
        }
    }
    serde_json::from_value(Value::Object(merged)).unwrap_or(defaults)
}

/// Live settings, shared by the dictation thread, tray and commands.
pub type Shared = Arc<RwLock<Settings>>;

fn store(app: &AppHandle) -> Result<Arc<Store<Wry>>> {
    let path = paths::data_dir(app)?.join(FILE);
    app.store(path).context("could not open settings")
}

fn set(app: &AppHandle, key: &str, value: Value) -> Result<()> {
    let s = store(app)?;
    s.set(key, value);
    s.save().context("could not save settings")
}

pub fn load(app: &AppHandle) -> Settings {
    let stored: Map<String, Value> = store(app)
        .map(|s| s.entries().into_iter().collect())
        .unwrap_or_default();
    from_stored(&stored)
}

pub fn save(app: &AppHandle, settings: &Settings) -> Result<()> {
    let s = store(app)?;
    if let Value::Object(map) = serde_json::to_value(settings)? {
        for (key, value) in map {
            s.set(key, value);
        }
    }
    s.save().context("could not save settings")
}

/// A copy of the live settings.
pub fn current(app: &AppHandle) -> Settings {
    app.try_state::<Shared>()
        .and_then(|s| s.read().ok().map(|s| s.clone()))
        .unwrap_or_default()
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

#[cfg(test)]
mod tests {
    use super::*;

    fn stored(v: Value) -> Map<String, Value> {
        match v {
            Value::Object(m) => m,
            _ => panic!("not an object"),
        }
    }

    #[test]
    fn defaults_follow_the_spec() {
        let s = Settings::default();
        assert_eq!(s.main_hotkey, ["ctrl", "win"]);
        assert!(!s.toggle_hotkey_enabled);
        assert_eq!(s.toggle_hotkey, ["ctrl", "shift", "space"]);
        assert_eq!(s.language, "en");
        assert!(s.roman_urdu);
        assert_eq!(s.sound_volume, 60);
        assert_eq!(s.overlay_colour, "#ffffff");
        assert_eq!(s.overlay_position, OverlayPosition::BottomCentre);
        assert_eq!(s.typing_wpm, 40);
        assert_eq!(s.unload_after_minutes, 10);
        assert!(!s.keep_audio_clips);
        assert!(s.remove_fillers && s.add_trailing_space && !s.voice_punctuation);
        assert!(s.smart_formatting);
        assert!(s.fix_misheard_words && s.learn_words);
        assert!(s.validate().is_ok());
    }

    #[test]
    fn empty_store_gives_defaults() {
        assert_eq!(from_stored(&Map::new()), Settings::default());
    }

    #[test]
    fn stored_keys_override_defaults_and_other_keys_are_ignored() {
        let s = from_stored(&stored(json!({
            "soundVolume": 30,
            "language": "auto",
            "activeModel": "whisper-small",
            "onboarded": true
        })));
        assert_eq!(s.sound_volume, 30);
        assert_eq!(s.language, "auto");
        assert_eq!(s.typing_wpm, 40);
    }

    #[test]
    fn a_bad_key_falls_back_alone() {
        let s = from_stored(&stored(json!({
            "soundVolume": "loud",
            "typingWpm": 5000,
            "overlayColour": "red",
            "mainHotkey": ["a"],
            "inputGain": 150
        })));
        let d = Settings::default();
        assert_eq!(s.sound_volume, d.sound_volume);
        assert_eq!(s.typing_wpm, d.typing_wpm);
        assert_eq!(s.overlay_colour, d.overlay_colour);
        assert_eq!(s.main_hotkey, d.main_hotkey);
        assert_eq!(s.input_gain, 150);
    }

    #[test]
    fn older_float_typing_speed_is_rejected_cleanly() {
        // Phase 7 never wrote the key, but a hand edited 40.5 must not break
        // loading.
        let s = from_stored(&stored(json!({ "typingWpm": 40.5 })));
        assert_eq!(s.typing_wpm, 40);
    }

    #[test]
    fn validation_messages() {
        let same_hotkeys = Settings {
            toggle_hotkey_enabled: true,
            toggle_hotkey: vec!["win".into(), "ctrl".into()],
            ..Settings::default()
        };
        assert!(same_hotkeys.validate().is_err());
        let bad_language = Settings {
            language: "Urdu".into(),
            ..Settings::default()
        };
        assert!(bad_language.validate().is_err());
        let odd_idle = Settings {
            unload_after_minutes: 7,
            ..Settings::default()
        };
        assert!(odd_idle.validate().is_err());
    }

    #[test]
    fn roman_urdu_pins_whisper_to_english() {
        let mut s = Settings {
            language: "auto".into(),
            ..Settings::default()
        };
        assert_eq!(s.engine_language(true).as_deref(), Some("en"));
        assert_eq!(s.engine_language(false), None);
        s.roman_urdu = false;
        assert_eq!(s.engine_language(true), None);
        s.language = "ur".into();
        assert_eq!(s.engine_language(true).as_deref(), Some("ur"));
    }
}
