#!/usr/bin/env bash
# Fails unless every Mach-O file under the given paths (an .app bundle, a
# mounted disk image) is arm64 only: no x86_64 slices, no universal binaries.
# The macOS counterpart of verify_arm64.ps1.
#
#   ./scripts/verify_arm64.sh path/to/Murmur.app

set -euo pipefail

checked=0
bad=0
while IFS= read -r -d '' file; do
  if file -b "$file" | grep -q 'Mach-O'; then
    archs="$(lipo -archs "$file" 2>/dev/null || echo unknown)"
    checked=$((checked + 1))
    if [ "$archs" != "arm64" ]; then
      echo "not arm64 only ($archs): $file" >&2
      bad=$((bad + 1))
    fi
  fi
done < <(find "$@" -type f -print0)

if [ "$checked" -eq 0 ]; then
  echo "no Mach-O files found in $*" >&2
  exit 1
fi
if [ "$bad" -ne 0 ]; then
  exit 1
fi
echo "$checked Mach-O files, all arm64"
