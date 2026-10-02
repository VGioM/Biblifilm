#!/bin/bash
# Downloads and unpacks VLCKit into Frameworks/ if it isn't there yet.
# Kept out of the project tree because it is ~80 MB of binary. The debug
# symbols that ship with it are dropped on arrival -- see below.
set -euo pipefail
cd "$(dirname "$0")"

VERSION="3.7.3-319ed2c0-79128878"
URL="https://download.videolan.org/cocoapods/prod/VLCKit-${VERSION}.tar.xz"
DEST="Frameworks/VLCKit.xcframework"

if [ -d "$DEST" ]; then
    echo "▸ VLCKit déjà présent"
    exit 0
fi

echo "▸ Téléchargement de VLCKit (~84 Mo)…"
mkdir -p Frameworks
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT
curl -fL --progress-bar -o "$TMP/vlckit.tar.xz" "$URL"

echo "▸ Extraction…"
tar -xJf "$TMP/vlckit.tar.xz" -C "$TMP"
SRC="$(find "$TMP" -maxdepth 3 -type d -name 'VLCKit.xcframework' | head -1)"
[ -n "$SRC" ] || { echo "✗ VLCKit.xcframework introuvable dans l'archive"; exit 1; }
mv "$SRC" "$DEST"

# The archive carries 323 MB of debug symbols for VLCKit itself -- four times
# the library they describe, and only ever useful to someone symbolicating a
# crash inside VLC's own code. They are never embedded in the app, so they buy
# this project nothing. The xcframework's Info.plist points at them through
# DebugSymbolsPath and SwiftPM refuses to build when that path is missing, so
# the key goes with them; an empty directory is not enough, it looks inside.
echo "▸ Retrait des symboles de débogage…"
rm -rf "$DEST"/*/dSYMs
/usr/libexec/PlistBuddy -c "Delete :AvailableLibraries:0:DebugSymbolsPath" \
    "$DEST/Info.plist" 2>/dev/null || true

echo "✓ VLCKit installé dans $DEST"
