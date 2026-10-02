#!/bin/bash
# Builds Biblifilm and installs it into /Applications, so it runs as a normal
# app independently of this project folder.
set -euo pipefail
cd "$(dirname "$0")"

DEST="${1:-/Applications}"

./build.sh release

echo "▸ Installation dans ${DEST}…"
if pgrep -x Biblifilm >/dev/null; then
    echo "  (Biblifilm est ouvert, fermeture…)"
    osascript -e 'quit app "Biblifilm"' 2>/dev/null || pkill -x Biblifilm || true
    sleep 2
fi

rm -rf "${DEST}/Biblifilm.app"
cp -R dist/Biblifilm.app "${DEST}/Biblifilm.app"

echo "✓ Installé : ${DEST}/Biblifilm.app"
echo "  Réglages et historique restent dans ~/Library/Application Support/Biblifilm"
echo "  Lancement : open -a Biblifilm"
