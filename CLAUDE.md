# Murmur

Free, open source, fully offline voice dictation for Windows on ARM64 (Snapdragon X). Hold a hotkey, speak, release, and clean text is pasted into the focused app. The full build spec lives outside the repo; this file keeps the rules every session must follow.

Build phases run in order. Each phase ends with its acceptance checks, one commit, and a stop for review.

## Hard constraints (never violate)

1. Windows ARM64 only. Every binary, dependency and artefact is `aarch64-pc-windows-msvc`. No x64 binaries anywhere: no x64 ONNX Runtime DLLs, sidecars or NSIS plugins. If a crate cannot build for aarch64, replace it. Never fall back to x64 emulation.
2. Offline by default. The only network request is downloading a model the user clicked Download on. No telemetry, analytics, update pings, accounts or cloud transcription.
3. All user data (transcripts, stats, dictionary, snippets, settings) stays in `%APPDATA%\Murmur`. Transcripts live in SQLite only.
4. Microphone audio is never written to disk unless "Keep audio clips" is on (default off).
5. UI copy: no em or en dashes, one clean line telling the user what to do, no subtext, no exclamation marks, sentence case.
6. Decisions in the spec are final. If truly blocked, stop, state the blocker in one paragraph, propose exactly one fix.
7. The app name lives in exactly two places: `src-tauri/tauri.conf.json` `productName` and `APP_NAME` in `src/lib/app.ts`. Everything else reads from those.

## Stack

- Tauri 2, Rust edition 2021, React 18 + TypeScript + Vite, Tailwind CSS 4. Shaped like Handy (github.com/cjpais/Handy) so its code is a usable reference.
- Speech: whisper-rs (whisper.cpp, CPU with NEON, no CUDA or OpenBLAS) for Whisper GGML; `ort` (ONNX Runtime 1.19+, prebuilt win-arm64, CPU EP) for Parakeet, Moonshine, SenseVoice. Vulkan and QNN are Phase 9 stretch goals, off by default.
- ONNX Runtime: Microsoft's official `onnxruntime-win-arm64-1.31.0.zip` (pure ARM64, never the arm64x package, which contains x64 code), fetched into git-ignored `src-tauri/vendor/` by `scripts/fetch_onnxruntime.ps1` (SHA-256 checked). `ort` =2.0.0-rc.15 with `load-dynamic`: the DLL is loaded from an explicit path (bundle resource, or the vendor copy in debug builds) because Windows ships an older onnxruntime.dll in System32.
- Models: `src-tauri/src/models/catalog.json` is the catalogue; URLs are pinned to a repository commit and every file has a SHA-256 (or a git SHA-1 for small git-stored files). Downloads go to `%APPDATA%\Murmur\models\<id>\` as `name.part`, resume with HTTP Range, and are renamed into place only after the hash matches. `engine::host::EngineHost` holds the active engine; switching unloads the old model before loading the new one so two large models are never resident together.
- Settings: `%APPDATA%\Murmur\settings.json` through tauri-plugin-store given an absolute path (its default would be `%APPDATA%\<identifier>`).
- History: `%APPDATA%\Murmur\history.db` (rusqlite bundled, WAL, schema versioned with PRAGMA user_version in `db/schema.rs`). One connection behind a mutex shared by the dictation thread and the commands. A row is written even when pasting fails, so the text can be copied from History. `word_counts` is updated in the same transaction as each insert and delete (stopwords from `text/stopwords.json` excluded). App icons are extracted from the exe on first use and saved to `%APPDATA%\Murmur\icons\<exe>.png`.
- Audio: cpal at device rate, resampled to 16 kHz mono f32 with rubato. The input stream is built at startup and kept paused between dictations (building takes 250 to 500 ms, play is instant); it is rebuilt when cpal reports the device changed.
- Hotkey events are timestamped inside the hook. The state machine uses those timestamps, never the time the controller got to them.
- Hotkey: our own `WH_KEYBOARD_LL` hook via the `windows` crate (`src-tauri/src/hotkey/listener.rs`), for key down and key up. Do not use tauri-plugin-global-shortcut for the main hotkey. Do not use rdev: its hook calls `AttachThreadInput` and `ToUnicodeEx` on every key, which stalls when the focused window is busy (Windows then skips the hook and the key leaks) and breaks dead keys. The hook proc must stay fast and never block; input Murmur injects is tagged with `MURMUR_INPUT_TAG` in `dwExtraInfo` so the hook ignores it.
- Injection: arboard + enigo. Save clipboard, write text, Ctrl+V (Ctrl+Shift+V for terminals), wait 120 ms, restore clipboard.
- Sounds: rodio playing bundled WAVs from `src-tauri/sounds`.
- DB: rusqlite with `bundled`. Settings: tauri-plugin-store `settings.json`. Frontend state: zustand. Charts: recharts.
- Packaging: NSIS arm64 installer `Murmur_<version>_arm64-setup.exe` plus portable zip.
- Tests: `cargo test`, vitest. No end to end framework.
- `.cargo/config.toml` pins the build target to aarch64 and sets `LIBCLANG_PATH`. LLVM `bin` must also be on PATH (some C deps call `clang` directly).
- whisper.cpp refuses MSVC on ARM, so `.cargo/config.toml` also makes whisper-rs-sys build it with LLVM `clang-cl` through native ARM64 Ninja (winget `Ninja-build.Ninja`), adds back `/EHsc`, and forces `CMAKE_BUILD_TYPE=Release` so dev timings match release. cargo does not rebuild whisper-rs-sys when these change: `cargo clean -p whisper-rs-sys`.

## Decisions made during the build (deviations from the spec, approved)

- Default model is `ggml-large-v3-turbo-q8_0.bin` (models\whisper-large-v3-turbo-q8), not q5_0. ggml repacks Q8_0 weights for i8mm at load; Q5_0 has no repacked ARM kernel. q5_0 took 10 to 14 s for a 5 s clip on the X Plus, q8_0 takes about 4.5 s with the same accuracy. Measured RAM with q8_0 loaded: 1.28 GB working set, 1.75 GB private after a dictation.
- Whisper always encodes its full 30 s window. Shrinking it with `audio_ctx` was 3x faster but garbled Roman Urdu and repeated sentences on real recordings. Do not reintroduce without rerunning the fixture WER check.
- Whisper output that is just a run of three or more words copied from the initial prompt is dropped (it happens on silence).
- Fixture clips: debug builds with `MURMUR_FIXTURE_DIR` set save each dictation as the next free `clip_NN.wav` and skip transcription. Release builds never write audio.
- The overlay window is never hidden. It stays shown, transparent, click through and non focusable (`focusable: false` gives WS_EX_NOACTIVATE; click through gives WS_EX_TRANSPARENT | WS_EX_LAYERED), and the page draws nothing when idle. Hiding it breaks things: WebView2 stops rendering hidden windows, and showing a window through Tauri activates it and steals focus. Placement uses Win32 `SetWindowPos` with SWP_NOACTIVATE. Idle CPU with overlay and sound output open: about 0.25%.
- Testing with injected keys: keep Notepad (never this chat app, never Murmur) in front. Injected keys do not reach the hook while Murmur's own window has focus, and anything the mic hears gets pasted into the focused app.
- Moonshine and SenseVoice are left out of catalog.json until their engines exist (Phase 8); Moonshine Base has no 120 MB build, so pick its files then.
- Measured on the fixture clips (X Plus, AC): Parakeet v2 transcribes a 5 s clip in about 0.1 to 0.16 s with 1.5% English WER, but cannot do Roman Urdu; Whisper turbo Q8_0 takes about 4.5 s with 6.1% English WER and is the only usable Roman Urdu option.
- Insights (`db/insights.rs`) are computed in SQL on each request and cached in the `useInsights` store, refreshed on `transcript-added` and after Clear history. Weeks start Monday; last week is compared over the same number of days. Average speed is words over audio minutes for the last 30 days. Time saved is total words at the `typingWpm` setting (default 40) minus total audio time, never below zero. Streak counts consecutive days back from today (`db/stats.rs`), so it is 0 until the first dictation of the day.
- Insights fixture: `py tests/seed_insights.py` writes 200 transcripts anchored to today to `%TEMP%\murmur-insights-fixture.db` and prints the expected numbers; run a debug build with `MURMUR_HISTORY_DB` set to that path to check the page. The same fixture is the `two_hundred_transcript_fixture` unit test.
- WER check: `cargo test fixture_wer -- --ignored --nocapture` (needs the model and `tests/audio`; `MURMUR_MODEL_ID` picks a catalogue model). Timings are only meaningful on AC power with the machine quiet; a rebuild in this OneDrive folder triggers OneDrive and Defender scans that slow it 2x.

## Architecture

```
src/            React frontend
  app/          routes: Home, Insights, History, Models, Dictionary, Snippets, Settings
  overlay/      floating pill window (separate Vite entry)
  components/  store/ (zustand)  lib/ (invoke wrappers, formatting)
src-tauri/src/
  audio/        capture, resample, vad, levels
  engine/       mod.rs (SpeechEngine trait), whisper, parakeet, moonshine, sensevoice
  models/       catalog.rs + catalog.json, download, registry
  hotkey/       listener (WH_KEYBOARD_LL hook), state (push to talk + toggle)
  inject/       paste, focus
  text/         fillers, dictionary, snippets, format
  db/           schema, transcripts, stats
  overlay/      window, events
  sound/        player
  commands.rs   all #[tauri::command] fns
  settings.rs
src-tauri/sounds/   start.wav, stop.wav, cancel.wav
scripts/gen_sounds.py
tests/
```

One dictation:
1. Hotkey down: state Recording, play start.wav, show overlay, start capture 60 ms after playback starts.
2. Every 50 ms emit `audio-level { rms, peak }` to the overlay.
3. Hotkey up (or second press in toggle mode): state Processing, stop capture, then play stop.wav.
4. Under 300 ms or all silence: state Cancelled, play cancel.wav, hide overlay, record nothing.
5. Active engine transcribes the 16 kHz buffer.
6. Text pipeline in this order: trim, filler removal, dictionary, (Roman Urdu normalisation), snippets, capitalisation and spacing.
7. Inject into the focused app.
8. Insert transcript row, emit `transcript-added`.
9. Overlay shows done for 400 ms, then hides. State Idle.

Escape while Recording cancels. Hotkey states: Idle, Recording, Processing, Cancelled, Error, every transition unit tested.

Engine trait:

```rust
pub trait SpeechEngine: Send + Sync {
    fn id(&self) -> &'static str;
    fn load(&mut self, model_path: &Path, opts: &EngineOptions) -> Result<()>;
    fn unload(&mut self);
    fn is_loaded(&self) -> bool;
    fn transcribe(&self, pcm16k_mono: &[f32], language: Option<&str>) -> Result<Transcript>;
}
```

The active model loads once at start (or on switch) and stays in memory; "Unload model after idle" (default 10 min, 0 = never) frees it.

## Quality bar

- `cargo clippy --target aarch64-pc-windows-msvc -- -D warnings` clean.
- `cargo test` and `pnpm test` green at every commit.
- No `unwrap()` outside tests. Errors surface in the UI as one line.
- Idle: under 1% CPU, under 150 MB RAM with model unloaded; under 1.2 GB with whisper-large-v3-turbo-q5 loaded (the q8_0 default measures 1.28 GB working set); never above 3 GB with any catalogue model.
- Hotkey down to pill visible under 100 ms. Key up to pasted text under 3 s for a 10 s clip on the default model, under 1.5 s with Parakeet v2.
- `tests/audio/` holds 10 reference clips (5 English, 5 English + Roman Urdu). Every engine change reruns them and prints WER.
- Every screen handles its empty state with one line of copy.
