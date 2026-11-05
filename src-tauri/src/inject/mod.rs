//! Getting text into the focused app.

#[cfg(windows)]
pub mod field;
#[cfg(target_os = "macos")]
#[path = "field_mac.rs"]
pub mod field;
pub mod focus;
pub mod icons;
pub mod paste;
