#!/bin/bash
# Unit tests. Plain `swift test` cannot load the bundle: VLCKit is linked as
# @loader_path/../Frameworks, i.e. inside the bundle, as in the app -- so it
# is put there before the run.
set -euo pipefail
cd "$(dirname "$0")/.."

./fetch-vlckit.sh
swift build --build-tests
BIN="$(swift build --show-bin-path)"
for bundle in "$BIN"/*.xctest; do
    mkdir -p "$bundle/Contents/Frameworks"
    ln -sfn "$PWD/Frameworks/VLCKit.xcframework/macos-arm64_x86_64/VLCKit.framework" \
            "$bundle/Contents/Frameworks/VLCKit.framework"
done
swift test --skip-build "$@"
