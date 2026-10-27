# Murmur

Free, open source, fully offline voice dictation for Windows on ARM64.

Hold a hotkey, speak, release, and the text is pasted into whatever app has focus. Speech models run locally. Nothing leaves your machine.

## Build

Requires Rust (aarch64-pc-windows-msvc), VS 2022 Build Tools with the MSVC ARM64 tools, LLVM (on PATH), CMake, Node and pnpm 9.

```
pnpm install
cargo tauri dev
```

## Licence

MIT. See LICENSE and NOTICE.
