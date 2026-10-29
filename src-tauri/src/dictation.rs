//! Drives one dictation from hotkey to result: owns the state machine, the
//! recorder, the speech engine and the hotkey listener, and runs on its own
//! thread. Settings are read live, so changes apply to the next dictation.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use tauri::{AppHandle, Emitter};

use crate::audio::capture::{InputConfig, Recorder, Recording, Tap};
use crate::db::transcripts::NewTranscript;
use crate::db::Db;
use crate::engine::fixer::Fixer;
use crate::engine::host::EngineHost;
use crate::engine::{has_arabic_script, Transcript};
use crate::hotkey::keys;
use crate::hotkey::listener::{self, Combo, HotkeyControl, HotkeyEvent};
use crate::hotkey::state::{Effect, Input, Machine, Mode, State};
use crate::inject::{field, focus, icons, paste};
use crate::models::catalog::Family;
use crate::overlay::events::{Overlay, Phase, Style};
use crate::settings::{self, Settings};
use crate::sound::player::{Cue, SoundPlayer};
use crate::text::dictionary::DictEntry;
use crate::text::fixup::Context as FixContext;
use crate::text::learn;
use crate::text::smart::AppKind;
use crate::text::store::TextStore;
use crate::text::{self, snippets::SnippetContext, TextSettings};

/// Recordings shorter than this are discarded.
const MIN_AUDIO_MS: u64 = 300;
/// Capture starts this long after the start cue begins playing (spec 6.5).
const START_CUE_LEAD: Duration = Duration::from_millis(60);
/// Longest wait for the text before the cursor once transcription is done.
/// It is read while the user speaks, so it is almost always there already.
const CONTEXT_WAIT: Duration = Duration::from_millis(150);
/// Earlier dictation into the same app counts as context for this long.
const EARLIER_FOR: Duration = Duration::from_secs(10 * 60);
/// How long a field is watched for corrections after a paste.
const WATCH_FOR: Duration = Duration::from_secs(60);

/// Where things live on disk.
pub struct Dirs {
    pub icons: PathBuf,
    /// Audio clips, written only while "Keep audio clips" is on.
    pub clips: PathBuf,
}

/// The running dictation pipeline, for the tray and the settings commands.
#[derive(Clone)]
pub struct Dictation {
    tx: Sender<(HotkeyEvent, Instant)>,
    hotkeys: Arc<HotkeyControl>,
    recorder: Recorder,
    overlay: Overlay,
    sound: SoundPlayer,
    recording: Arc<AtomicBool>,
}

fn combos(s: &Settings) -> (Combo, Option<Combo>) {
    let main = keys::combo(&s.main_hotkey).unwrap_or_else(|_| Combo::ctrl_win());
    let toggle = s
        .toggle_hotkey_enabled
        .then(|| keys::combo(&s.toggle_hotkey).unwrap_or_else(|_| Combo::ctrl_shift_space()));
    (main, toggle)
}

fn input_config(s: &Settings) -> InputConfig {
    InputConfig {
        device: s.microphone.clone(),
        gain: s.input_gain as f32 / 100.0,
    }
}

fn style(s: &Settings) -> Style {
    Style {
        bar_colour: s.overlay_colour.clone(),
    }
}

impl Dictation {
    /// Pushes changed settings to the hook, microphone, overlay and sounds.
    pub fn apply(&self, s: &Settings) {
        let (main, toggle) = combos(s);
        self.hotkeys.set_combos(main, toggle);
        self.recorder.configure(input_config(s));
        self.overlay
            .configure(s.overlay_enabled, s.overlay_position, style(s));
        self.sound.set(s.sound_volume, s.sounds_enabled);
    }

    /// Starts or stops a hands free dictation, as the toggle hotkey does.
    pub fn toggle(&self) {
        let _ = self.tx.send((HotkeyEvent::ToggleDown, Instant::now()));
    }

    pub fn is_recording(&self) -> bool {
        self.recording.load(Ordering::SeqCst)
    }

    pub fn hotkeys(&self) -> &Arc<HotkeyControl> {
        &self.hotkeys
    }

    pub fn test_microphone(&self, on: bool) {
        self.recorder.monitor(on);
    }

    pub fn overlay_style(&self) -> Style {
        self.overlay.style()
    }
}

#[allow(clippy::too_many_arguments)]
pub fn spawn(
    app: AppHandle,
    engine: Arc<EngineHost>,
    fixer: Arc<Fixer>,
    db: Arc<Db>,
    dirs: Dirs,
    shared: settings::Shared,
    text: Arc<TextStore>,
) -> Result<Dictation> {
    let initial = shared.read().map(|s| s.clone()).unwrap_or_default();
    let (tx, rx) = mpsc::channel();
    let recording = Arc::new(AtomicBool::new(false));
    let hotkeys = HotkeyControl::new();
    let (main, toggle) = combos(&initial);
    listener::spawn(main, toggle, recording.clone(), tx.clone(), hotkeys.clone());

    let overlay = Overlay::new(
        app.clone(),
        initial.overlay_enabled,
        initial.overlay_position,
        style(&initial),
    );
    let meter = overlay.clone();
    let main_window = app.clone();
    let recorder = Recorder::spawn(
        input_config(&initial),
        Box::new(move |level, tap| match tap {
            Tap::Recording => meter.level(level),
            Tap::Test => {
                let _ = main_window.emit_to("main", "mic-level", level);
            }
        }),
    )?;
    let sound = SoundPlayer::spawn(initial.sound_volume, initial.sounds_enabled);
    let handle = Dictation {
        tx,
        hotkeys,
        recorder: recorder.clone(),
        overlay: overlay.clone(),
        sound: sound.clone(),
        recording: recording.clone(),
    };

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
                fixer,
                before_cursor: None,
                watching: Arc::new(AtomicBool::new(false)),
                recent: None,
                db,
                dirs,
                ignore_before: None,
                settings: shared,
                text,
                last_used: Instant::now(),
            }
            .run(rx)
        })
        .context("could not start dictation thread")?;
    Ok(handle)
}

/// Debug builds only, and only with MURMUR_FIXTURE_DIR set: saves each
/// dictation as the next free clip_NN.wav so test fixtures can be recorded by
/// simply dictating. Release builds never write audio to disk here. Returns
/// true in fixture mode.
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

/// The clip file for a transcript, kept while "Keep audio clips" is on.
pub fn clip_path(clips: &std::path::Path, id: i64) -> PathBuf {
    clips.join(format!("{id}.wav"))
}

/// Watches a paste for corrections and adds what the user fixed to the
/// dictionary when the watch ends.
struct LearnWatcher {
    pasted: String,
    /// Corrections in the latest read that still had the pasted text.
    found: Option<Vec<DictEntry>>,
    text: Arc<TextStore>,
    app: AppHandle,
}

impl field::Watcher for LearnWatcher {
    fn snapshot(&mut self, field: &str) {
        if let Some(found) = learn::corrections(&self.pasted, field) {
            self.found = Some(found);
        }
    }

    fn end(self: Box<Self>) {
        let Some(found) = self.found.filter(|f| !f.is_empty()) else {
            return;
        };
        match self.text.learn(found) {
            Ok(added) if !added.is_empty() => {
                for e in &added {
                    log::info!("learned \"{}\" for \"{}\"", e.written, e.spoken);
                }
                let _ = self.app.emit("dictionary-changed", ());
            }
            Ok(_) => {}
            Err(err) => log::warn!("could not save learned words: {err:#}"),
        }
    }
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
    /// The fix-up model, used when it is downloaded and switched on.
    fixer: Arc<Fixer>,
    /// The text before the cursor, being read while the user speaks.
    before_cursor: Option<Receiver<Option<String>>>,
    /// Set to stop watching the last paste for corrections.
    watching: Arc<AtomicBool>,
    /// The last dictation: app, final text and when.
    recent: Option<(String, String, Instant)>,
    db: Arc<Db>,
    dirs: Dirs,
    /// Key events from before this instant happened while transcribing and
    /// are dropped, so a press during Processing does not start a recording
    /// late.
    ignore_before: Option<Instant>,
    settings: settings::Shared,
    text: Arc<TextStore>,
    /// Last dictation, for unloading the model after a quiet spell.
    last_used: Instant,
}

impl Controller {
    fn settings(&self) -> Settings {
        self.settings.read().map(|s| s.clone()).unwrap_or_default()
    }

    /// When to unload the model, if it is loaded and the setting is on.
    fn idle_deadline(&self) -> Option<Instant> {
        let minutes = self.settings().unload_after_minutes;
        (minutes > 0 && self.machine.state() == State::Idle && self.engine.is_loaded())
            .then(|| self.last_used + Duration::from_secs(minutes as u64 * 60))
    }

    fn run(mut self, rx: Receiver<(HotkeyEvent, Instant)>) {
        loop {
            let machine = self.machine.deadline();
            let idle = self.idle_deadline();
            let deadline = match (machine, idle) {
                (Some(a), Some(b)) => Some(a.min(b)),
                (a, b) => a.or(b),
            };
            let event = match deadline {
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
                    if idle.is_some_and(|t| t <= Instant::now()) {
                        self.fixer.park();
                        if self.engine.park() {
                            let _ = self.app.emit("models-changed", ());
                        }
                    }
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
                // An unloaded model starts loading while the user speaks.
                self.engine.wake();
                self.last_used = Instant::now();
                self.watching.store(true, Ordering::SeqCst);
                self.before_cursor = None;
                if self.settings().fix_misheard_words && self.fixer.has_model() {
                    self.fixer.wake();
                    self.before_cursor = Some(field::read_before_cursor());
                }
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
                        self.last_used = Instant::now();
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
        let prefs = self.settings();
        let whisper = self.engine.active_family() == Some(Family::Whisper);
        let language = prefs.engine_language(whisper);
        let (model, transcript) = self.transcribe(rec, language.as_deref())?;
        let raw = transcript.text.clone();
        if raw.trim().is_empty() {
            return Ok(false);
        }

        let user = self.text.get();
        let heard = if prefs.fix_misheard_words {
            self.fix_up(&raw, &target, &user.dictionary)
        } else {
            raw.clone()
        };
        let text_settings = TextSettings {
            remove_fillers: prefs.remove_fillers,
            voice_punctuation: prefs.voice_punctuation,
            add_trailing_space: prefs.add_trailing_space,
            roman_urdu: prefs.roman_urdu,
            smart_formatting: prefs.smart_formatting,
            app: AppKind::detect(&target.exe, &target.title),
            dictionary: user.dictionary,
            snippets: user.snippets,
            roman_urdu_map: user.roman_urdu_map,
        };
        let clipboard = if text_settings
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
        let text = text::process(&heard, &text_settings, &ctx);
        if text.trim().is_empty() {
            log::info!("nothing left after clean up");
            return Ok(false);
        }

        let started = Instant::now();
        let pasted = paste::paste(&text, &target);
        // Saved even if pasting failed, so the text can be copied from History.
        let language = transcript.language.clone().or(language);
        self.record(
            rec,
            &model,
            &transcript,
            &raw,
            text.trim(),
            &target,
            language,
            prefs.keep_audio_clips,
        );
        pasted?;
        self.recent = Some((target.exe.clone(), text.trim().to_string(), Instant::now()));
        if prefs.learn_words && !target.is_terminal() {
            self.watching = Arc::new(AtomicBool::new(false));
            field::watch(
                Box::new(LearnWatcher {
                    pasted: text.trim().to_string(),
                    found: None,
                    text: self.text.clone(),
                    app: self.app.clone(),
                }),
                self.watching.clone(),
                WATCH_FOR,
            );
        }
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

    /// The transcript with misheard words fixed, using what is already in
    /// the field, earlier dictation and the dictionary as context. As heard
    /// when the fix-up model is missing, still loading or finds nothing.
    fn fix_up(&mut self, raw: &str, target: &focus::FocusedApp, dictionary: &[DictEntry]) -> String {
        let before = self.before_cursor.take();
        if !self.fixer.is_loaded() {
            return raw.to_string();
        }
        let before = before
            .and_then(|rx| rx.recv_timeout(CONTEXT_WAIT).ok().flatten())
            .unwrap_or_default();
        // The field usually holds the earlier dictation already.
        let earlier = match &self.recent {
            Some((exe, text, at))
                if before.trim().is_empty() && *exe == target.exe && at.elapsed() < EARLIER_FOR =>
            {
                text.clone()
            }
            _ => String::new(),
        };
        let ctx = FixContext {
            before,
            earlier,
            vocabulary: dictionary.iter().map(|e| e.written.clone()).collect(),
        };
        self.fixer.fix(raw, &ctx).unwrap_or_else(|| raw.to_string())
    }

    /// Writes the transcript row (spec 5.2 step 8), tells the UI, and saves
    /// the app's icon the first time Murmur sees that app. Failures are
    /// logged: history must never break dictation.
    #[allow(clippy::too_many_arguments)]
    fn record(
        &self,
        rec: &Recording,
        model: &str,
        transcript: &Transcript,
        raw: &str,
        final_text: &str,
        target: &focus::FocusedApp,
        language: Option<String>,
        keep_audio: bool,
    ) {
        let row = NewTranscript {
            created_at: chrono::Local::now().naive_local(),
            raw_text: raw.to_string(),
            final_text: final_text.to_string(),
            audio_ms: rec.duration_ms(),
            inference_ms: transcript.inference_ms,
            model_id: model.to_string(),
            language,
            app_name: (!target.exe.is_empty()).then(|| target.exe.clone()),
            app_title: (!target.title.is_empty()).then(|| target.title.clone()),
        };
        match self.db.insert_transcript(&row) {
            Ok(id) => {
                if keep_audio {
                    let path = clip_path(&self.dirs.clips, id);
                    let saved = std::fs::create_dir_all(&self.dirs.clips)
                        .map_err(anyhow::Error::from)
                        .and_then(|_| crate::audio::wav::write_16k_mono(&path, &rec.samples));
                    if let Err(err) = saved {
                        log::warn!("could not keep the audio clip: {err:#}");
                    }
                }
                let _ = self.app.emit("transcript-added", id);
            }
            Err(err) => log::error!("could not save the transcript: {err:#}"),
        }
        if !target.exe.is_empty() {
            let (dir, exe, path) = (
                self.dirs.icons.clone(),
                target.exe.clone(),
                target.exe_path.clone(),
            );
            std::thread::spawn(move || {
                if let Err(err) = icons::ensure_saved(&dir, &exe, &path) {
                    log::debug!("no icon for {exe}: {err:#}");
                }
            });
        }
    }

    /// Runs the active engine. Returns the model id and its transcript.
    fn transcribe(&mut self, rec: &Recording, language: Option<&str>) -> Result<(String, Transcript)> {
        let (model, transcript) = self.engine.transcribe(&rec.samples, language)?;
        let audio_ms = rec.duration_ms();
        log::info!(
            "{} transcribed {:.2} s of audio in {} ms ({:.2}x real time), language {}",
            model,
            audio_ms as f64 / 1000.0,
            transcript.inference_ms,
            transcript.inference_ms as f64 / audio_ms.max(1) as f64,
            transcript
                .language
                .as_deref()
                .or(language)
                .unwrap_or("auto")
        );
        if has_arabic_script(&transcript.text) {
            log::warn!("transcript contains Arabic script");
        }
        log::info!("raw text: {}", transcript.text);
        Ok((model, transcript))
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
