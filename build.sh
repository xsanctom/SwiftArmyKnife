#!/usr/bin/env bash
# Build the Swift Army Knife .app: compile the Rust core, then the Swift app,
# then assemble a bundle. Apple-silicon only, no Xcode project required.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")" && pwd)"
CORE="$ROOT/core"
APP="$ROOT/app"
GEN="$CORE/generated"
BUILD="$ROOT/build"
BUNDLE="$BUILD/SwiftArmyKnife.app"
BIN="$BUNDLE/Contents/MacOS/SwiftArmyKnife"

export PATH="$HOME/.cargo/bin:$PATH"
export DEVELOPER_DIR="${DEVELOPER_DIR:-/Applications/Xcode.app/Contents/Developer}"

echo "==> Rust core (release)"
( cd "$CORE" && cargo build --release )

echo "==> Swift app"
rm -rf "$BUNDLE"
mkdir -p "$(dirname "$BIN")" "$BUNDLE/Contents/Resources"

# All app sources plus the generated swift-bridge glue, compiled as one module.
swift_sources=$(find "$APP/Sources" -name '*.swift')
swiftc \
    -O \
    -target arm64-apple-macos14.0 \
    -import-objc-header "$APP/Bridging.h" \
    -I "$GEN" \
    $swift_sources \
    "$GEN/SwiftBridgeCore.swift" \
    "$GEN/swift_army_knife_core/swift_army_knife_core.swift" \
    -L "$CORE/target/release" -lswift_army_knife_core \
    -framework SwiftUI -framework AppKit -framework UniformTypeIdentifiers \
    -o "$BIN"

cp "$APP/Info.plist" "$BUNDLE/Contents/Info.plist"

# Bundle the static ffmpeg/ffprobe if present, so the app is self-contained.
if [ -f "$ROOT/Resources/bin/ffmpeg" ] && [ -f "$ROOT/Resources/bin/ffprobe" ]; then
    mkdir -p "$BUNDLE/Contents/Resources/bin"
    cp "$ROOT/Resources/bin/ffmpeg" "$ROOT/Resources/bin/ffprobe" "$BUNDLE/Contents/Resources/bin/"
    chmod +x "$BUNDLE/Contents/Resources/bin/ffmpeg" "$BUNDLE/Contents/Resources/bin/ffprobe"
    # Sign nested executables first (Apple silicon requires a valid signature).
    codesign --force --sign - "$BUNDLE/Contents/Resources/bin/ffmpeg" \
        "$BUNDLE/Contents/Resources/bin/ffprobe" >/dev/null 2>&1 || true
    echo "==> bundled ffmpeg + ffprobe"
else
    echo "==> no bundled binaries found; app will fall back to Homebrew ffmpeg"
fi

# Ad-hoc sign the whole bundle so macOS is happy launching it locally.
codesign --force --sign - "$BUNDLE" >/dev/null 2>&1 || true

echo "==> built $BUNDLE"
