# Murmur

Free, open source, fully offline voice dictation for Windows on ARM64.

Hold a hotkey, speak, release, and the text is pasted into whatever app has focus. Speech models run locally. Nothing leaves your machine.

## Build

Requires Rust (aarch64-pc-windows-msvc), VS 2022 Build Tools with the MSVC ARM64 tools, LLVM (on PATH), CMake, Ninja, Node and pnpm 9.

```
powershell -ExecutionPolicy Bypass -File scripts/fetch_onnxruntime.ps1
pnpm install
cargo tauri dev
```

The first command fetches Microsoft's official ONNX Runtime for Windows ARM64 into `src-tauri/vendor/`, which the Parakeet engine needs.

## Licence

MIT. See LICENSE and NOTICE.
