#!/usr/bin/env bash
# Récupère libVLC (la version Windows 64 bits officielle de VideoLAN) dans
# extra/vlc/ : libvlc.dll, libvlccore.dll, plugins/ et vlc.exe (pour « Ouvrir
# dans VLC »). build-windows.sh copie extra/ à côté de Biblifilm.exe.
# L'équivalent Windows de fetch-vlckit.sh sur Mac.
set -euo pipefail
cd "$(dirname "$0")"

VLC_VERSION="${VLC_VERSION:-3.0.21}"
URL="https://get.videolan.org/vlc/$VLC_VERSION/win64/vlc-$VLC_VERSION-win64.zip"
WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

echo "→ $URL"
if [ -n "${VLC_ZIP:-}" ]; then cp "$VLC_ZIP" "$WORK/vlc.zip"; else curl -fSL -o "$WORK/vlc.zip" "$URL"; fi
WANT="$(curl -fsSL "$URL.sha256" | cut -d' ' -f1)"
GOT="$(sha256sum "$WORK/vlc.zip" | cut -d' ' -f1)"
[ "$WANT" = "$GOT" ] || { echo "Empreinte SHA-256 inattendue : $GOT (attendu $WANT)" >&2; exit 1; }

mkdir -p "$WORK/x"
"$(cygpath -u "${SYSTEMROOT:-C:\Windows}")/System32/tar.exe" -xf "$(cygpath -w "$WORK/vlc.zip")" -C "$(cygpath -w "$WORK/x")"
SRC="$WORK/x/vlc-$VLC_VERSION"
rm -rf extra/vlc
mkdir -p extra/vlc
cp "$SRC/libvlc.dll" "$SRC/libvlccore.dll" "$SRC/vlc.exe" "$SRC/COPYING.txt" extra/vlc/
cp -r "$SRC/plugins" extra/vlc/
# Ce dont un lecteur intégré n'a pas besoin : interfaces, scripts Lua, extensions.
rm -rf extra/vlc/plugins/gui extra/vlc/plugins/lua extra/vlc/plugins/control extra/vlc/plugins/visualization
# vlc.exe a besoin de son interface pour « Ouvrir dans VLC » : on la remet.
cp -r "$SRC/plugins/gui" extra/vlc/plugins/
cp -r "$SRC/plugins/control" extra/vlc/plugins/
# Pas de plugins.dat : VLC le vérifie par la date des fichiers, que la
# décompression du zip change ; il relit les plugins au lancement (en arrière-plan).
rm -f extra/vlc/plugins/plugins.dat
echo "→ extra/vlc ($(du -sh extra/vlc | cut -f1))"
