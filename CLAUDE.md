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
- Audio: cpal at device rate, resampled to 16 kHz mono f32 with rubato. The input stream is built at startup and kept paused between dictations (building takes 250 to 500 ms, play is instant); it is rebuilt when cpal reports the device changed.
- Hotkey events are timestamped inside the hook. The state machine uses those timestamps, never the time the controller got to them.
- Hotkey: our own `WH_KEYBOARD_LL` hook via the `windows` crate (`src-tauri/src/hotkey/listener.rs`), for key down and key up. Do not use tauri-plugin-global-shortcut for the main hotkey. Do not use rdev: its hook calls `AttachThreadInput` and `ToUnicodeEx` on every key, which stalls when the focused window is busy (Windows then skips the hook and the key leaks) and breaks dead keys. The hook proc must stay fast and never block; input Murmur injects is tagged with `MURMUR_INPUT_TAG` in `dwExtraInfo` so the hook ignores it.
- Injection: arboard + enigo. Save clipboard, write text, Ctrl+V (Ctrl+Shift+V for terminals), wait 120 ms, restore clipboard.
- Sounds: rodio playing bundled WAVs from `src-tauri/sounds`.
- DB: rusqlite with `bundled`. Settings: tauri-plugin-store `settings.json`. Frontend state: zustand. Charts: recharts.
- Packaging: NSIS arm64 installer `Murmur_<version>_arm64-setup.exe` plus portable zip.
- Tests: `cargo test`, vitest. No end to end framework.
- `.cargo/config.toml` pins the build target to aarch64 and sets `LIBCLANG_PATH`. LLVM `bin` must also be on PATH (some C deps call `clang` directly).

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
- Idle: under 1% CPU, under 150 MB RAM with model unloaded; under 1.2 GB with whisper-large-v3-turbo-q5 loaded; never above 3 GB with any catalogue model.
- Hotkey down to pill visible under 100 ms. Key up to pasted text under 3 s for a 10 s clip on the default model, under 1.5 s with Parakeet v2.
- `tests/audio/` holds 10 reference clips (5 English, 5 English + Roman Urdu). Every engine change reruns them and prints WER.
- Every screen handles its empty state with one line of copy.
