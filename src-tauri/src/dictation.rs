//! Drives one dictation from hotkey to result: owns the state machine, the
//! recorder and the hotkey listener, and runs on its own thread.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::sync::Arc;
use std::time::Instant;

use anyhow::{Context, Result};
use tauri::{AppHandle, Emitter};

use crate::audio::capture::Recorder;
use crate::hotkey::listener::{self, Combo, HotkeyEvent};
use crate::hotkey::state::{Effect, Input, Machine, Mode, State};

/// Recordings shorter than this are discarded.
const MIN_AUDIO_MS: u64 = 300;
/// The toggle only hotkey is off by default. Becomes a setting in Phase 8.
const TOGGLE_HOTKEY_ENABLED: bool = false;

pub fn spawn(app: AppHandle) -> Result<()> {
    let (tx, rx) = mpsc::channel();
    let recording = Arc::new(AtomicBool::new(false));
    listener::spawn(
        Combo::ctrl_win(),
        TOGGLE_HOTKEY_ENABLED.then(Combo::ctrl_shift_space),
        recording.clone(),
        tx,
    );

    let recorder = Recorder::spawn()?;
    std::thread::Builder::new()
        .name("murmur-dictation".into())
        .spawn(move || {
            Controller {
                app,
                machine: Machine::new(),
                recorder,
                recording,
                published: (State::Idle, None),
                event_at: Instant::now(),
                capture_started: None,
            }
            .run(rx)
        })
        .context("could not start dictation thread")?;
    Ok(())
}

struct Controller {
    app: AppHandle,
    machine: Machine,
    recorder: Recorder,
    /// Shared with the hotkey hook so it knows when to capture Escape.
    recording: Arc<AtomicBool>,
    published: (State, Option<Mode>),
    /// When the event being handled happened (key time, or now for timers).
    event_at: Instant,
    /// Key time of the event that started the current recording.
    capture_started: Option<Instant>,
}

impl Controller {
    fn run(mut self, rx: Receiver<(HotkeyEvent, Instant)>) {
        loop {
            let event = match self.machine.deadline() {
                Some(deadline) => {
                    match rx.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
                        Ok(event) => Some(event),
                        Err(RecvTimeoutError::Timeout) => None,
                        Err(RecvTimeoutError::Disconnected) => break,
                    }
                }
                None => match rx.recv() {
                    Ok(event) => Some(event),
                    Err(_) => break,
                },
            };

            match event {
                Some((event, at)) => {
                    self.event_at = at;
                    // Only timers that expired before the key event happened.
                    let effect = self.machine.tick(at);
                    self.drive(effect);
                    let input = match event {
                        HotkeyEvent::MainDown => Input::HotkeyDown,
                        HotkeyEvent::MainUp => Input::HotkeyUp,
                        HotkeyEvent::ToggleDown => Input::ToggleHotkey,
                        HotkeyEvent::Escape => Input::Escape,
                    };
                    let effect = self.machine.handle(input, at);
                    self.drive(effect);
                }
                None => {
                    self.event_at = Instant::now();
                    let effect = self.machine.tick(self.event_at);
                    self.drive(effect);
                }
            }
        }
    }

    /// Carries out an effect and any follow up it produces.
    fn drive(&mut self, mut effect: Effect) {
        loop {
            let follow_up = self.apply(effect);
            self.publish();
            match follow_up {
                Some(input) => effect = self.machine.handle(input, Instant::now()),
                None => break,
            }
        }
    }

    fn apply(&mut self, effect: Effect) -> Option<Input> {
        match effect {
            Effect::None => None,
            Effect::StartCapture => match self.recorder.start() {
                Ok(()) => {
                    self.recording.store(true, Ordering::SeqCst);
                    self.capture_started = Some(self.event_at);
                    None
                }
                Err(err) => {
                    log::error!("could not start recording: {err:#}");
                    Some(Input::Failed)
                }
            },
            Effect::StopCapture => {
                self.recording.store(false, Ordering::SeqCst);
                match self.recorder.stop() {
                    Ok(rec) => {
                        let ms = rec.duration_ms();
                        let hotkey_ms = self.capture_started.take().map_or(0, |at| {
                            self.event_at.saturating_duration_since(at).as_millis()
                        });
                        log::info!(
                            "captured {:.2} s, hotkey down {:.2} s ({} samples at 16 kHz, mic {} Hz)",
                            ms as f64 / 1000.0,
                            hotkey_ms as f64 / 1000.0,
                            rec.samples.len(),
                            rec.device_rate
                        );
                        if ms < MIN_AUDIO_MS {
                            log::info!("too short, discarded");
                            Some(Input::Discarded)
                        } else {
                            Some(Input::Finished)
                        }
                    }
                    Err(err) => {
                        log::error!("could not stop recording: {err:#}");
                        Some(Input::Failed)
                    }
                }
            }
            Effect::CancelCapture => {
                self.recording.store(false, Ordering::SeqCst);
                self.recorder.cancel();
                None
            }
        }
    }

    fn publish(&mut self) {
        let current = (self.machine.state(), self.machine.mode());
        if current == self.published {
            return;
        }
        let state_changed = current.0 != self.published.0;
        self.published = current;
        let name = format!("{:?}", current.0).to_lowercase();
        match current.1 {
            Some(mode) => log::info!("state: {name} ({mode:?})"),
            None => log::info!("state: {name}"),
        }
        if state_changed {
            let _ = self.app.emit("dictation-state", name);
        }
    }
}
