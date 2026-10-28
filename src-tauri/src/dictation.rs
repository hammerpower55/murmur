//! Drives one dictation from hotkey to result: owns the state machine, the
//! recorder, the speech engine and the hotkey listener, and runs on its own
//! thread.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use tauri::{AppHandle, Emitter};

use crate::audio::capture::{Recorder, Recording};
use crate::engine::has_arabic_script;
use crate::engine::host::EngineHost;
use crate::hotkey::listener::{self, Combo, HotkeyEvent};
use crate::hotkey::state::{Effect, Input, Machine, Mode, State};
use crate::inject::{focus, paste};
use crate::overlay::events::{Overlay, Phase};
use crate::settings::{LANGUAGE, ROMAN_URDU_MIXING};
use crate::sound::player::{Cue, SoundPlayer};
use crate::text::{self, snippets::SnippetContext, TextSettings};

/// Recordings shorter than this are discarded.
const MIN_AUDIO_MS: u64 = 300;
/// The toggle only hotkey is off by default. Becomes a setting in Phase 8.
const TOGGLE_HOTKEY_ENABLED: bool = false;
/// Overlay and sound settings, fixed until Phase 8.
const OVERLAY_ENABLED: bool = true;
const SOUND_VOLUME: u8 = 60;
const SOUNDS_MUTED: bool = false;
/// Capture starts this long after the start cue begins playing (spec 6.5).
const START_CUE_LEAD: Duration = Duration::from_millis(60);

pub fn spawn(app: AppHandle, engine: Arc<EngineHost>) -> Result<()> {
    let (tx, rx) = mpsc::channel();
    let recording = Arc::new(AtomicBool::new(false));
    listener::spawn(
        Combo::ctrl_win(),
        TOGGLE_HOTKEY_ENABLED.then(Combo::ctrl_shift_space),
        recording.clone(),
        tx,
    );

    let overlay = Overlay::new(app.clone(), OVERLAY_ENABLED);
    let meter = overlay.clone();
    let recorder = Recorder::spawn(Box::new(move |level| meter.level(level)))?;
    let sound = SoundPlayer::spawn(SOUND_VOLUME, SOUNDS_MUTED);
    std::thread::Builder::new()
        .name("murmur-dictation".into())
        .spawn(move || {
            Controller {
                app,
                machine: Machine::new(),
                recorder,
                recording,
                overlay,
                sound,
                published: (State::Idle, None),
                event_at: Instant::now(),
                capture_started: None,
                engine,
                ignore_before: None,
                // Dictionary, snippets and toggles load from settings in Phase 8.
                text: TextSettings {
                    roman_urdu: ROMAN_URDU_MIXING,
                    ..TextSettings::default()
                },
            }
            .run(rx)
        })
        .context("could not start dictation thread")?;
    Ok(())
}

/// Debug builds only, and only with MURMUR_FIXTURE_DIR set: saves each
/// dictation as the next free clip_NN.wav so test fixtures can be recorded by
/// simply dictating. Release builds never write audio to disk. Returns true
/// in fixture mode.
#[cfg(debug_assertions)]
fn save_fixture(rec: &Recording) -> bool {
    let Some(dir) = std::env::var_os("MURMUR_FIXTURE_DIR") else {
        return false;
    };
    let dir = std::path::PathBuf::from(dir);
    let path = (1..1000)
        .map(|n| dir.join(format!("clip_{n:02}.wav")))
        .find(|p| !p.exists());
    match path {
        Some(path) => match crate::audio::wav::write_16k_mono(&path, &rec.samples) {
            Ok(()) => log::info!("fixture saved: {}", path.display()),
            Err(err) => log::warn!("could not save fixture: {err:#}"),
        },
        None => log::warn!("fixture folder is full"),
    }
    true
}

struct Controller {
    app: AppHandle,
    machine: Machine,
    recorder: Recorder,
    /// Shared with the hotkey hook so it knows when to capture Escape.
    recording: Arc<AtomicBool>,
    overlay: Overlay,
    sound: SoundPlayer,
    published: (State, Option<Mode>),
    /// When the event being handled happened (key time, or now for timers).
    event_at: Instant,
    /// Key time of the event that started the current recording.
    capture_started: Option<Instant>,
    /// The active model; swapped from the Models page without a restart.
    engine: Arc<EngineHost>,
    /// Key events from before this instant happened while transcribing and
    /// are dropped, so a press during Processing does not start a recording
    /// late.
    ignore_before: Option<Instant>,
    text: TextSettings,
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
                Some((_, at)) if self.ignore_before.is_some_and(|t| at < t) => {}
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
            Effect::StartCapture => {
                // Pill first so it shows well inside 100 ms, then the cue,
                // then the microphone once the cue is under way.
                self.overlay.recording();
                self.sound.play(Cue::Start);
                std::thread::sleep(START_CUE_LEAD);
                self.start_capture()
            }
            Effect::StopCapture => {
                self.recording.store(false, Ordering::SeqCst);
                // The stop cue plays only after capture has stopped, so it
                // never lands in the recording.
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
                            // Discarded plays the cancel cue instead.
                            log::info!("too short, discarded");
                            return Some(Input::Discarded);
                        }
                        self.sound.play(Cue::Stop);
                        // Show "Transcribing" now: the state change is only
                        // published after this effect, i.e. after transcription.
                        self.overlay.set(Phase::Processing);
                        // Recording fixtures skips transcription, so presses are
                        // never dropped while a long transcription runs.
                        #[cfg(debug_assertions)]
                        if save_fixture(&rec) {
                            return Some(Input::Finished);
                        }
                        let result = self.dictate(&rec);
                        self.ignore_before = Some(Instant::now());
                        match result {
                            Ok(true) => Some(Input::Finished),
                            Ok(false) => Some(Input::Discarded),
                            Err(err) => {
                                log::error!("dictation failed: {err:#}");
                                Some(Input::Failed)
                            }
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

    fn start_capture(&mut self) -> Option<Input> {
        match self.recorder.start() {
            Ok(()) => {
                self.recording.store(true, Ordering::SeqCst);
                self.capture_started = Some(self.event_at);
                None
            }
            Err(err) => {
                log::error!("could not start recording: {err:#}");
                Some(Input::Failed)
            }
        }
    }

    /// Transcribes, cleans up and pastes one recording into the app that had
    /// focus when the hotkey was released. Returns false if nothing was said.
    fn dictate(&mut self, rec: &Recording) -> Result<bool> {
        let target = focus::foreground();
        let raw = self.transcribe(rec)?;
        if raw.trim().is_empty() {
            return Ok(false);
        }

        let clipboard = if self
            .text
            .snippets
            .iter()
            .any(|s| s.text.contains("{clipboard}"))
        {
            arboard::Clipboard::new()
                .and_then(|mut c| c.get_text())
                .unwrap_or_default()
        } else {
            String::new()
        };
        let ctx = SnippetContext::now(clipboard);
        let text = text::process(&raw, &self.text, &ctx);
        if text.trim().is_empty() {
            log::info!("nothing left after clean up");
            return Ok(false);
        }

        let started = Instant::now();
        paste::paste(&text, &target)?;
        log::info!(
            "pasted {} chars into {} in {} ms{}",
            text.chars().count(),
            if target.exe.is_empty() {
                "unknown app"
            } else {
                &target.exe
            },
            started.elapsed().as_millis(),
            if target.is_terminal() {
                " (terminal)"
            } else {
                ""
            }
        );
        Ok(true)
    }

    /// Runs the engine and returns its raw text.
    fn transcribe(&mut self, rec: &Recording) -> Result<String> {
        let (model, transcript) = self.engine.transcribe(&rec.samples, Some(LANGUAGE))?;
        let audio_ms = rec.duration_ms();
        log::info!(
            "{} transcribed {:.2} s of audio in {} ms ({:.2}x real time), language {}",
            model,
            audio_ms as f64 / 1000.0,
            transcript.inference_ms,
            transcript.inference_ms as f64 / audio_ms.max(1) as f64,
            transcript.language.as_deref().unwrap_or("unknown")
        );
        if has_arabic_script(&transcript.text) {
            log::warn!("transcript contains Arabic script");
        }
        log::info!("raw text: {}", transcript.text);
        Ok(transcript.text)
    }

    fn publish(&mut self) {
        let current = (self.machine.state(), self.machine.mode());
        if current == self.published {
            return;
        }
        let previous = self.published.0;
        let state_changed = current.0 != previous;
        self.published = current;
        if state_changed {
            match (previous, current.0) {
                (_, State::Processing) => self.overlay.set(Phase::Processing),
                (_, State::Cancelled) => {
                    self.sound.play(Cue::Cancel);
                    self.overlay.set(Phase::Cancelled);
                }
                (_, State::Error) => self.overlay.set(Phase::Error),
                (State::Processing, State::Idle) => self.overlay.set(Phase::Done),
                _ => {}
            }
        }
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
