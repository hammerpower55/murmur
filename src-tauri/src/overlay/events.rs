//! What the overlay shows, driven from the dictation controller. The page
//! listens for `overlay-phase` and `audio-level`; this side decides timing.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::Serialize;
use tauri::{AppHandle, Emitter};

use super::window::{OverlayWindow, LABEL};
use crate::audio::levels::Level;

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

#[derive(Clone)]
pub struct Overlay {
    app: AppHandle,
    window: Option<Arc<OverlayWindow>>,
    shown: Arc<Mutex<Shown>>,
}

impl Overlay {
    /// `enabled` false gives an overlay that does nothing (setting "overlay
    /// off"). Setup failures are logged and also leave it inert.
    pub fn new(app: AppHandle, enabled: bool) -> Self {
        let window = if enabled {
            match OverlayWindow::setup(&app) {
                Ok(w) => Some(Arc::new(w)),
                Err(err) => {
                    log::warn!("overlay disabled: {err:#}");
                    None
                }
            }
        } else {
            None
        };
        Self {
            app,
            window,
            shown: Arc::new(Mutex::new(Shown {
                generation: 0,
                phase: Phase::Hidden,
            })),
        }
    }

    /// A new recording: move to the monitor under the mouse and show.
    pub fn recording(&self) {
        let Some(window) = &self.window else {
            return;
        };
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
        let generation = match self.shown.lock() {
            Ok(mut shown) if shown.phase != phase => {
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
        if self.window.is_some() {
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
