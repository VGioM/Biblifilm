#!/bin/bash
# Construit dist/<App>-<version>-linux-x86_64.AppImage.
#
# À lancer sur une Fedora (ou dans un conteneur toolbox Fedora, voir README),
# avec Rust, squashfs-tools et patchelf — et pour Biblifilm le paquet mpv-devel
# de RPM Fusion : libmpv et ses dépendances (FFmpeg complet, libass,
# libplacebo…) sont copiées dans l'AppImage. Jamais la glibc, les pilotes
# graphiques, le son ni les bibliothèques de base du bureau : celles du système.
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
for t in mksquashfs patchelf; do command -v $t >/dev/null || { echo "✗ $t manquant (dnf install squashfs-tools patchelf)."; exit 1; }; done

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

# Biblifilm : libmpv (chargée à l'exécution) et tout ce qu'elle tire, sauf le socle du système.
if grep -q '^mod mpv;' src/main.rs; then
    MPV="$(ls /usr/lib64/libmpv.so.2)"
    # Le socle du système, par nom exact (« libm » ne doit pas emporter « libmujs ») :
    # glibc, pilotes et API graphiques, son, bus et chiffrement présents partout.
    SYSTEM='^(ld-linux-x86-64|libc|libm|libdl|libpthread|librt|libresolv|libutil|libgcc_s|libstdc\+\+|libz|libGL|libEGL|libGLX|libGLdispatch|libOpenGL|libgbm|libdrm|libvulkan|libva|libva-drm|libva-x11|libva-wayland|libvdpau|libX11|libX11-xcb|libXau|libXdmcp|libXext|libXrender|libXfixes|libXi|libXrandr|libXcursor|libXinerama|libXss|libXxf86vm|libxcb|libxkbcommon|libwayland-client|libwayland-cursor|libwayland-egl|libasound|libpulse|libpulse-simple|libpipewire-0\.3|libjack|libfontconfig|libfreetype|libexpat|libdbus-1|libsystemd|libudev|libselinux|libuuid|libcap|libmount|libblkid|libgio-2\.0|libglib-2\.0|libgobject-2\.0|libgmodule-2\.0|libffi|libpcre2-8|libcrypt|libgnutls|libssl|libcrypto|libp11-kit|libtasn1|libnettle|libhogweed|libgmp|libidn2|libunistring)\.so'
    cp -L "$MPV" "$APPDIR/usr/lib/libmpv.so.2"
    ldd "$MPV" | awk '/=> \//{print $1" "$3}' | while read -r soname path; do
        grep -qE "$SYSTEM" <<<"$soname" && continue
        cp -L "$path" "$APPDIR/usr/lib/$soname"
    done
    for lib in "$APPDIR"/usr/lib/*.so*; do patchelf --set-rpath '$ORIGIN' "$lib"; done
    echo "  $(ls "$APPDIR/usr/lib" | wc -l) bibliothèques, $(du -sh "$APPDIR/usr/lib" | cut -f1)"
fi

echo "▸ AppImage…"
rm -f "$OUT" "$OUT.squashfs"
mksquashfs "$APPDIR" "$OUT.squashfs" -root-owned -noappend -comp xz -b 1048576 -quiet >/dev/null
cat "$RUNTIME" "$OUT.squashfs" > "$OUT"
rm -f "$OUT.squashfs"
chmod +x "$OUT"
ls -la "$OUT"
