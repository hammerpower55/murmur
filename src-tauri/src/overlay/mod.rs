//! The floating voice orb (spec 6.4).

pub mod events;
#[cfg(windows)]
pub mod window;
#[cfg(target_os = "macos")]
#[path = "window_mac.rs"]
pub mod window;
