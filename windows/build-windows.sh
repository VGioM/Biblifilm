#!/usr/bin/env bash
# Compile l'app et fait le zip portable Windows :
#   dist/<App>/<App>.exe (+ ce que contient extra/, s'il existe)
#   dist/<App>-<version>-windows-x64.zip
# Le zip est celui que la mise à jour intégrée cherche dans les releases.
set -euo pipefail
cd "$(dirname "$0")"

APP="$(basename "$(cd .. && pwd)")"
BIN="$(sed -n 's/^name = "\(.*\)"/\1/p' Cargo.toml | head -1)"
VERSION="$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)"

cargo build --release
rm -rf "dist/$APP"
mkdir -p "dist/$APP"
cp "target/release/$BIN.exe" "dist/$APP/$APP.exe"
if [ -d extra ]; then cp -r extra/. "dist/$APP/"; fi

ZIP="dist/$APP-$VERSION-windows-x64.zip"
rm -f "$ZIP"
(cd dist && powershell.exe -NoProfile -Command "Compress-Archive -Path '$APP' -DestinationPath '$APP-$VERSION-windows-x64.zip' -CompressionLevel Optimal")
echo "→ $ZIP ($(du -h "$ZIP" | cut -f1))"
