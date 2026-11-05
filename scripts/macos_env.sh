# Build settings for Apple Silicon Macs. .cargo/config.toml pins the build to
# Windows ARM64 and points whisper.cpp at LLVM's clang-cl; variables already
# set in the environment win over its [env] table, so source this first:
#
#   source scripts/macos_env.sh
#   cargo test            # from src-tauri
#   pnpm tauri build --target aarch64-apple-darwin
#
# aarch64-apple-darwin only: never x86_64 or universal builds.

export CARGO_BUILD_TARGET="aarch64-apple-darwin"
# whisper.cpp builds with Apple's clang through CMake's default generator.
export CMAKE_GENERATOR="Unix Makefiles"
export CMAKE_C_COMPILER="clang"
export CMAKE_CXX_COMPILER="clang++"
# Matches bundle > macOS > minimumSystemVersion (ONNX Runtime 1.31 needs 14).
export MACOSX_DEPLOYMENT_TARGET="14.0"

# bindgen (whisper-rs-sys) needs libclang from Xcode or the command line tools.
for dir in \
  "$(xcode-select -p 2>/dev/null)/Toolchains/XcodeDefault.xctoolchain/usr/lib" \
  "/Library/Developer/CommandLineTools/usr/lib"; do
  if [ -f "$dir/libclang.dylib" ]; then
    export LIBCLANG_PATH="$dir"
    break
  fi
done
