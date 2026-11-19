#!/bin/bash
# Construit dist/<App>-<version>-linux-x86_64.AppImage.
#
# À lancer sur une Fedora (ou dans un conteneur toolbox Fedora, voir README),
# avec Rust et squashfs-tools. Aucune bibliothèque n'est embarquée : celles du
# système servent, et Biblifilm confie la lecture à VLC.
#
# Le « runtime » placé en tête de l'AppImage (le petit programme qui la monte et
# la lance) est un fichier binaire de type 2 du projet AppImage ; donnez son
# chemin dans APPIMAGE_RUNTIME (par défaut : celui que met en cache
# electron-builder, déjà utilisé pour les versions précédentes).
set -euo pipefail
cd "$(dirname "$0")"

NAME="$(sed -n 's/^name = "\(.*\)"/\1/p' Cargo.toml | head -1)"
VERSION="$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)"
PRETTY="$(tr '[:lower:]' '[:upper:]' <<<"${NAME:0:1}")${NAME:1}"
OUT="dist/$PRETTY-$VERSION-linux-x86_64.AppImage"
RUNTIME="${APPIMAGE_RUNTIME:-$(ls "$HOME"/.cache/electron-builder/appimage-*/appimage-*/runtime-x64 2>/dev/null | head -1)}"
[ -f "$RUNTIME" ] || { echo "✗ Runtime AppImage introuvable : donnez son chemin dans APPIMAGE_RUNTIME."; exit 1; }
command -v mksquashfs >/dev/null || { echo "✗ mksquashfs manquant (dnf install squashfs-tools)."; exit 1; }

export PATH="$HOME/.cargo/bin:$PATH"
echo "▸ Tests et compilation…"
cargo test --release --quiet >/dev/null || { echo "✗ Les tests échouent."; exit 1; }
cargo build --release --quiet

echo "▸ AppDir…"
APPDIR="$(mktemp -d)"
trap 'rm -rf "$APPDIR"' EXIT
mkdir -p "$APPDIR/usr/bin" "$APPDIR/usr/lib" dist
cp "target/release/$NAME" "$APPDIR/usr/bin/"
cp "resources/$NAME.desktop" "$APPDIR/"
cp resources/icon.png "$APPDIR/$NAME.png"
ln -s "$NAME.png" "$APPDIR/.DirIcon"
cat > "$APPDIR/AppRun" <<RUN
#!/bin/sh
HERE="\$(dirname "\$(readlink -f "\$0")")"
export APPDIR="\${APPDIR:-\$HERE}"
exec "\$HERE/usr/bin/$NAME" "\$@"
RUN
chmod +x "$APPDIR/AppRun"

echo "▸ AppImage…"
rm -f "$OUT" "$OUT.squashfs"
# gzip, pas xz : le lancement reste instantané (zstd : non lu par ce runtime).
mksquashfs "$APPDIR" "$OUT.squashfs" -root-owned -noappend -comp gzip -b 131072 -quiet >/dev/null
cat "$RUNTIME" "$OUT.squashfs" > "$OUT"
rm -f "$OUT.squashfs"
chmod +x "$OUT"
ls -la "$OUT"
