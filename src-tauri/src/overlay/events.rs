//! What the overlay shows, driven from the dictation controller. The page
//! listens for `overlay-phase` and `audio-level`; this side decides timing.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::Serialize;
use tauri::{AppHandle, Emitter};

use super::window::{OverlayWindow, LABEL};
use crate::audio::levels::Level;
use crate::settings::OverlayPosition;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Phase {
    Hidden,
    Recording,
    Processing,
    Done,
    Cancelled,
    Error,
}

impl Phase {
    /// How long a closing phase stays up before the pill fades out.
    fn hold(self) -> Option<Duration> {
        match self {
            Phase::Done => Some(Duration::from_millis(400)),
            Phase::Cancelled => Some(Duration::from_millis(300)),
            Phase::Error => Some(Duration::from_millis(1500)),
            _ => None,
        }
    }
}

struct Shown {
    /// Bumped on every new recording so a stale fade out is ignored.
    generation: u64,
    phase: Phase,
}

/// How the pill looks; the page asks for it on load and gets
/// `overlay-style` whenever it changes.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Style {
    pub wave_colour: String,
}

#[derive(Clone)]
pub struct Overlay {
    app: AppHandle,
    window: Option<Arc<OverlayWindow>>,
    shown: Arc<Mutex<Shown>>,
    /// Setting "Show overlay"; when off nothing is shown.
    enabled: Arc<AtomicBool>,
    style: Arc<Mutex<Style>>,
}

impl Overlay {
    /// Setup failures are logged and leave the overlay inert.
    pub fn new(app: AppHandle, enabled: bool, position: OverlayPosition, style: Style) -> Self {
        let window = match OverlayWindow::setup(&app, position) {
            Ok(w) => Some(Arc::new(w)),
            Err(err) => {
                log::warn!("overlay disabled: {err:#}");
                None
            }
        };
        Self {
            app,
            window,
            shown: Arc::new(Mutex::new(Shown {
                generation: 0,
                phase: Phase::Hidden,
            })),
            enabled: Arc::new(AtomicBool::new(enabled)),
            style: Arc::new(Mutex::new(style)),
        }
    }

    /// Applies the overlay settings. A pill on screen keeps going.
    pub fn configure(&self, enabled: bool, position: OverlayPosition, style: Style) {
        self.enabled.store(enabled, Ordering::SeqCst);
        if let Some(window) = &self.window {
            window.set_position(position);
        }
        if let Ok(mut s) = self.style.lock() {
            if *s != style {
                *s = style.clone();
                let _ = self.app.emit_to(LABEL, "overlay-style", style);
            }
        }
    }

    pub fn style(&self) -> Style {
        self.style
            .lock()
            .map(|s| s.clone())
            .unwrap_or_else(|_| Style {
                wave_colour: "#f6b83d".into(),
            })
    }

    /// A new recording: move to the monitor under the mouse and show.
    pub fn recording(&self) {
        let Some(window) = &self.window else {
            return;
        };
        if !self.enabled.load(Ordering::SeqCst) {
            return;
        }
        if let Err(err) = window.place_on_cursor_monitor() {
            log::warn!("could not place the overlay: {err:#}");
        }
        if let Ok(mut shown) = self.shown.lock() {
            shown.generation += 1;
            shown.phase = Phase::Recording;
            self.emit(Phase::Recording);
        }
    }

    /// Moves to `phase`. Closing phases fade out after their hold time.
    pub fn set(&self, phase: Phase) {
        if self.window.is_none() {
            return;
        }
        // Only a recording brings the pill up (and not while it is turned
        // off), so later phases move it along but never show it.
        let generation = match self.shown.lock() {
            Ok(mut shown) if shown.phase != phase && shown.phase != Phase::Hidden => {
                shown.phase = phase;
                self.emit(phase);
                shown.generation
            }
            _ => return,
        };
        if let Some(hold) = phase.hold() {
            let overlay = self.clone();
            std::thread::spawn(move || {
                std::thread::sleep(hold);
                if let Ok(mut shown) = overlay.shown.lock() {
                    if shown.generation == generation && shown.phase == phase {
                        shown.phase = Phase::Hidden;
                        overlay.emit(Phase::Hidden);
                    }
                }
            });
        }
    }

    pub fn level(&self, level: Level) {
        // The main window's sound rings follow the voice, overlay or not.
        let _ = self.app.emit_to("main", "audio-level", level);
        if self.window.is_some() && self.enabled.load(Ordering::SeqCst) {
            let _ = self.app.emit_to(LABEL, "audio-level", level);
        }
    }

    fn emit(&self, phase: Phase) {
        let _ = self.app.emit_to(LABEL, "overlay-phase", phase);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hold_times_follow_the_spec() {
        assert_eq!(Phase::Done.hold(), Some(Duration::from_millis(400)));
        assert_eq!(Phase::Cancelled.hold(), Some(Duration::from_millis(300)));
        assert_eq!(Phase::Error.hold(), Some(Duration::from_millis(1500)));
        assert_eq!(Phase::Recording.hold(), None);
        assert_eq!(Phase::Processing.hold(), None);
    }

    #[test]
    fn phases_serialise_as_lowercase_names() {
        assert_eq!(
            serde_json::to_string(&Phase::Processing).unwrap(),
            "\"processing\""
        );
    }
}
