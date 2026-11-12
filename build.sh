#!/bin/bash
# Builds Biblifilm.app into ./dist and signs it.
set -euo pipefail
cd "$(dirname "$0")"

CONFIG="${1:-release}"
APP="dist/Biblifilm.app"

# The player needs VLCKit; fetch it on first build.
./fetch-vlckit.sh

echo "▸ Compiling ($CONFIG)…"
swift build -c "$CONFIG"
BIN="$(swift build -c "$CONFIG" --show-bin-path)/Biblifilm"

echo "▸ Assembling bundle…"
rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources" "$APP/Contents/Frameworks"
cp "$BIN" "$APP/Contents/MacOS/Biblifilm"

echo "▸ Embedding VLCKit…"
cp -R Frameworks/VLCKit.xcframework/macos-arm64_x86_64/VLCKit.framework \
      "$APP/Contents/Frameworks/"

# VLCKit ships arm64 and x86_64 fused into one binary, so an app built here
# carries 43 MB of Intel code this Mac will never execute. Dropping the slice
# that is not ours takes the bundle from 82 MB to 38 MB. Only the copy inside
# the app is thinned: the download under Frameworks/ stays whole, so
# fetch-vlckit.sh remains the source of truth and a build on an Intel Mac
# keeps its own slice instead. The framework is signed further down, which it
# has to be -- lipo invalidates the signature it arrived with.
VLC_BINARY="$APP/Contents/Frameworks/VLCKit.framework/Versions/A/VLCKit"
if lipo -info "$VLC_BINARY" 2>/dev/null | grep -q "fat file"; then
    echo "  (dégraissage vers $(uname -m))"
    lipo -thin "$(uname -m)" "$VLC_BINARY" -output "$VLC_BINARY.thin"
    mv "$VLC_BINARY.thin" "$VLC_BINARY"
fi

cat > "$APP/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleName</key>                  <string>Biblifilm</string>
    <key>CFBundleDisplayName</key>           <string>Biblifilm</string>
    <key>CFBundleExecutable</key>            <string>Biblifilm</string>
    <key>CFBundleIdentifier</key>            <string>com.biblifilm.app</string>
    <key>CFBundleVersion</key>               <string>1.3.5</string>
    <key>CFBundleShortVersionString</key>    <string>1.3.5</string>
    <key>CFBundlePackageType</key>           <string>APPL</string>
    <key>CFBundleIconFile</key>              <string>AppIcon</string>
    <key>LSMinimumSystemVersion</key>        <string>14.0</string>
    <key>NSHighResolutionCapable</key>       <true/>
    <key>NSSupportsAutomaticGraphicsSwitching</key> <true/>
    <key>NSHumanReadableCopyright</key>      <string>Biblifilm</string>
</dict>
</plist>
PLIST

if [ -f Resources/AppIcon.icns ]; then
    cp Resources/AppIcon.icns "$APP/Contents/Resources/AppIcon.icns"
fi

# The debug map names every source file by its absolute path on this Mac;
# a release build has no use for it.
[ "$CONFIG" = "release" ] && strip -S "$APP/Contents/MacOS/Biblifilm" 2>/dev/null

# Signed with the local certificate when there is one (./signing-cert.sh makes
# it). It is what the in-app updater checks a new version against, and what
# lets macOS keep its permissions from one build to the next. Ad hoc otherwise.
IDENTITY="-"
if security find-identity -p codesigning 2>/dev/null | grep -q "VGioM Local Signing"; then
    IDENTITY="VGioM Local Signing"
fi
if [ "$IDENTITY" = "-" ]; then
    echo "▸ Signing (ad hoc — run ./signing-cert.sh for the local certificate)…"
else
    echo "▸ Signing ($IDENTITY)…"
fi
# The embedded framework must be signed before the app that contains it.
codesign --force --sign "$IDENTITY" "$APP/Contents/Frameworks/VLCKit.framework" 2>/dev/null
codesign --force --sign "$IDENTITY" "$APP/Contents/MacOS/Biblifilm" 2>/dev/null
codesign --force --sign "$IDENTITY" "$APP" 2>/dev/null

echo "✓ Built $APP"
