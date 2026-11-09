#!/usr/bin/env bash
# Installs the latest Murmur for Apple Silicon Macs from its GitHub release.
#
#   curl -fsSL https://raw.githubusercontent.com/hammerpower55/murmur/main/scripts/install_mac.sh | bash
#
# Murmur is not notarised (that needs a paid Apple Developer account), so a
# disk image downloaded in a browser is blocked by Gatekeeper. A file fetched
# with curl carries no "downloaded from the internet" flag, so macOS does not
# block it. Read this script before running it; it only downloads the release
# disk image, copies Murmur.app to Applications and opens it.
#
# MURMUR_DMG_URL uses another disk image, MURMUR_APPS_DIR another install
# folder and MURMUR_NO_OPEN=1 skips opening the app (for testing).

set -euo pipefail

REPO="hammerpower55/murmur"
APP="Murmur.app"

fail() {
  echo "$1" >&2
  exit 1
}

[ "$(uname -s)" = Darwin ] || fail "This installer is for Macs."
[ "$(uname -m)" = arm64 ] || fail "Murmur needs a Mac with Apple Silicon (M1 or later)."
[ "$(sw_vers -productVersion | cut -d. -f1)" -ge 14 ] || fail "Murmur needs macOS 14 or later."

url="${MURMUR_DMG_URL:-}"
if [ -z "$url" ]; then
  echo "Finding the latest Murmur release"
  url="$(curl -fsSL "https://api.github.com/repos/$REPO/releases/latest" \
    | grep -o '"browser_download_url": *"[^"]*_aarch64\.dmg"' \
    | head -1 \
    | sed 's/.*"\(https[^"]*\)"/\1/')" || true
fi
[ -n "$url" ] || fail "Could not find a Mac download. See https://github.com/$REPO/releases"

work="$(mktemp -d)"
mount="$work/mount"
cleanup() {
  hdiutil detach "$mount" -quiet 2>/dev/null || true
  rm -rf "$work"
}
trap cleanup EXIT

echo "Downloading $(basename "$url")"
curl -fL --progress-bar -o "$work/Murmur.dmg" "$url"

mkdir -p "$mount"
hdiutil attach -nobrowse -readonly -quiet -mountpoint "$mount" "$work/Murmur.dmg"
[ -d "$mount/$APP" ] || fail "The disk image does not contain $APP."

dest="${MURMUR_APPS_DIR:-/Applications}"
if [ ! -w "$dest" ]; then
  dest="$HOME/Applications"
  mkdir -p "$dest"
fi

# A running copy would keep its old files open.
if pgrep -x murmur > /dev/null 2>&1; then
  echo "Closing the running Murmur"
  osascript -e 'tell application "Murmur" to quit' > /dev/null 2>&1 || pkill -x murmur || true
  sleep 2
fi

rm -rf "$dest/$APP"
ditto "$mount/$APP" "$dest/$APP"
# Nothing here is flagged, but clear it in case a proxy or tool added one.
xattr -dr com.apple.quarantine "$dest/$APP" 2>/dev/null || true

echo "Installed $dest/$APP"
if [ "${MURMUR_NO_OPEN:-}" != 1 ]; then
  open "$dest/$APP"
  echo "Murmur is open. Allow the microphone and Accessibility when macOS asks."
fi
