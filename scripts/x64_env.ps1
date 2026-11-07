# Local x64 build environment. Dot-source it before any cargo or pnpm
# command on an Intel or AMD Windows machine:
#
#   . .\scripts\x64_env.ps1
#   pnpm tauri dev
#
# It only adds the portable toolchain used for these local builds; nothing
# here is part of the released ARM64 pipeline.

$Root = Join-Path $env:LOCALAPPDATA "murmur-build"

$env:CARGO_HOME = Join-Path $Root "cargo"
$env:RUSTUP_HOME = Join-Path $Root "rustup"
$env:LIBCLANG_PATH = Join-Path $Root "llvm\bin"
$env:CMAKE_GENERATOR = "Ninja"

$paths = @(
    (Join-Path $Root "cargo\bin"),
    (Join-Path $Root "node"),
    (Join-Path $Root "cmake\bin"),
    (Join-Path $Root "ninja"),
    (Join-Path $Root "llvm\bin")
)
foreach ($p in $paths) {
    if ((Test-Path $p) -and ($env:Path -notlike "*$p*")) {
        $env:Path = "$p;$env:Path"
    }
}

# whisper-rs-sys builds whisper.cpp through CMake. This machine has clang-cl
# from the portable LLVM, which .cargo/config.toml already points CMake at.
$env:CMAKE_C_COMPILER = Join-Path $Root "llvm\bin\clang-cl.exe"
$env:CMAKE_CXX_COMPILER = Join-Path $Root "llvm\bin\clang-cl.exe"

$env:TAURI_TARGET = "x86_64-pc-windows-msvc"
