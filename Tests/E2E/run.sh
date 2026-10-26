#!/bin/bash
# Builds the debug app into its own bundle (com.biblifilm.e2e, so it never
# shares settings with the real one) and runs the end-to-end scenarios.
#   ./Tests/E2E/run.sh [scenario…]
set -euo pipefail
cd "$(dirname "$0")/../.."

./fetch-vlckit.sh
swift build -c debug
BIN="$(swift build -c debug --show-bin-path)/Biblifilm"

APP=".build/e2e/BiblifilmE2E.app"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Frameworks"
cp "$BIN" "$APP/Contents/MacOS/Biblifilm"
if [ ! -d "$APP/Contents/Frameworks/VLCKit.framework" ]; then
    cp -R Frameworks/VLCKit.xcframework/macos-arm64_x86_64/VLCKit.framework \
          "$APP/Contents/Frameworks/"
fi
cat > "$APP/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleName</key>            <string>Biblifilm E2E</string>
    <key>CFBundleExecutable</key>      <string>Biblifilm</string>
    <key>CFBundleIdentifier</key>      <string>com.biblifilm.e2e</string>
    <key>CFBundlePackageType</key>     <string>APPL</string>
    <key>LSMinimumSystemVersion</key>  <string>14.0</string>
</dict>
</plist>
PLIST
codesign --force --sign - "$APP" 2>/dev/null

exec python3 -I Tests/E2E/run.py "$@"
