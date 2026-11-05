#!/usr/bin/env bash
# Fetches Microsoft's official ONNX Runtime for Apple Silicon Macs into
# src-tauri/vendor/ (git-ignored), the macOS counterpart of
# fetch_onnxruntime.ps1. The Parakeet, Moonshine and SenseVoice engines load
# libonnxruntime from there in debug builds; release builds bundle it in the
# app's Frameworks folder (tauri.macos.conf.json).
#
#   ./scripts/fetch_onnxruntime.sh
#
# The osx-arm64 build only: never the x86_64 or universal2 packages.

set -euo pipefail

VERSION="1.31.0"
SHA256="1031b36dd3bdaa8d976a6130d9cf2a05e9be320b93ae804f8427b91dc3432afc"
NAME="onnxruntime-osx-arm64-$VERSION"
URL="https://github.com/microsoft/onnxruntime/releases/download/v$VERSION/$NAME.tgz"

VENDOR="$(cd "$(dirname "$0")/.." && pwd)/src-tauri/vendor"
TARGET="$VENDOR/$NAME"
LIB="$TARGET/lib/libonnxruntime.$VERSION.dylib"
if [ -f "$LIB" ]; then
  echo "Already present: $TARGET"
  exit 0
fi

mkdir -p "$VENDOR"
ARCHIVE="$VENDOR/$NAME.tgz"
echo "Downloading $URL"
curl -L --fail -sS -o "$ARCHIVE" "$URL"

ACTUAL="$(shasum -a 256 "$ARCHIVE" | cut -d' ' -f1)"
if [ "$ACTUAL" != "$SHA256" ]; then
  rm -f "$ARCHIVE"
  echo "SHA256 mismatch: expected $SHA256, got $ACTUAL" >&2
  exit 1
fi

tar -xzf "$ARCHIVE" -C "$VENDOR"
rm -f "$ARCHIVE"
# Debug symbols and test libraries are not needed to build or run.
rm -rf "$TARGET"/lib/*.dSYM "$TARGET/testdata"

if [ "$(lipo -archs "$LIB")" != "arm64" ]; then
  echo "$LIB is not arm64 only: $(lipo -archs "$LIB")" >&2
  exit 1
fi
echo "ONNX Runtime $VERSION ready in $TARGET"
