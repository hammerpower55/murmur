# Building Murmur on Windows x64 (local development only)

Murmur ships ARM64 only: every released binary is `aarch64`. This page covers
building a working copy on an ordinary Intel or AMD Windows machine, for
development and for checking behaviour on hardware you have. Nothing here
changes the released target, and nothing built this way is shipped.

## What is different

Three small pieces, everything else is shared:

| File | Change |
| --- | --- |
| `src-tauri/src/engine/onnx.rs` | `VENDORED_RUNTIME` names the ONNX Runtime vendor folder for the architecture being built (`onnxruntime-win-arm64-1.31.0` or `onnxruntime-win-x64-1.31.0`) instead of hard-coding arm64 |
| `src-tauri/src/lib.rs` | The debug-build ONNX Runtime fallback uses that constant |
| `scripts/fetch_onnxruntime.ps1` | `-Arch arm64|x64` picks the Microsoft zip and its SHA-256; arm64 stays the default |
| `src-tauri/tauri.x64.conf.json` | `--config` overlay: clears the bundle resource list (the arm64 DLL and the QNN plugin do not exist here) and changes the description |
| `scripts/x64_env.ps1` | Puts the local portable toolchain on `PATH` |

The Rust source has no `cfg(target_arch)` gates: hotkey, injection, audio,
text pipeline, database and the Whisper engine are all architecture neutral.

## Prerequisites

MSVC Build Tools 2019 or later with the x64 C++ tools, plus a portable
toolchain in `%LOCALAPPDATA%\murmur-build`:

- Rust stable, `x86_64-pc-windows-msvc`
- Node LTS with pnpm 9
- CMake and Ninja
- LLVM, used as `clang-cl` for every C and C++ compile

LLVM as the C/C++ compiler is not cosmetic: MSVC's own `cl.exe` fails on this
machine with `C1083: Cannot open compiler intermediate file ... Permission
denied` from inside cargo build scripts, while the same command succeeds when
run by hand. `clang-cl` produces the same MSVC ABI output and does not hit it.

## Build

```powershell
. .\scripts\x64_env.ps1
powershell -ExecutionPolicy Bypass -File scripts\fetch_onnxruntime.ps1 -Arch x64
pnpm install
pnpm tauri build --target x86_64-pc-windows-msvc --no-bundle `
  --config src-tauri/tauri.x64.conf.json
```

The exe lands in `src-tauri/target/x86_64-pc-windows-msvc/release/murmur.exe`.

`--no-bundle` skips the installer. Add `--bundles nsis` instead and the
result lands in
`src-tauri/target/x86_64-pc-windows-msvc/release/bundle/nsis/Murmur_<version>_x64-setup.exe`.

A bare `cargo` build places the app beside no runtime, and the Parakeet,
Moonshine and SenseVoice engines cannot load without `onnxruntime.dll`. The
installer carries it under `vendor/onnxruntime-win-x64-1.31.0/lib/`, which
`lib.rs` looks in after the resource root; for a loose exe, copy it beside the
app:

```powershell
copy src-tauri\vendor\onnxruntime-win-x64-1.31.0\lib\onnxruntime.dll `
  src-tauri\target\x86_64-pc-windows-msvc\release\
```

## What does not work here

- **The NPU.** Qualcomm's QNN plugin is ARM64 Windows on Snapdragon only, so
  `npu_available` is false and NPU models are never listed. Whisper, Parakeet,
  Moonshine, SenseVoice and the word fixer run on the CPU.
- **The release and packaging scripts**, which check for ARM64 PE headers.
- **`cargo test` end to end**: the fixture clips in `tests/audio` are the
  developer's own voice and git-ignored, and `fixture_wer` needs a downloaded
  model. Unit tests that do not need a model still run.

## Running the installer

Copy the setup exe out of OneDrive before running it. Started from a OneDrive
folder it fails at once with "Error writing temporary file. Make sure your
temp folder is valid." and writes nothing; the same file run from a local
folder such as `C:\` installs normally.

## Keeping it out of the release

Nothing built this way ships. `CLAUDE.md` hard constraint 1 allows the local
x64 build and forbids an x64 release: CI, `scripts/verify_arm64.ps1` and
`scripts/package_portable.ps1` accept ARM64 only, and the release workflow
runs on ARM64 runners.
