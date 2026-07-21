#!/usr/bin/env bash
# Build a macOS .app bundle for DJ Player.
#
# Usage:
#   ./scripts/package-macos.sh
#
# Output:
#   target/release/bundle/osx/DJ Player.app

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(dirname "$SCRIPT_DIR")"
APP_DIR="$REPO_ROOT/target/release/bundle/osx/DJ Player.app"

cd "$REPO_ROOT"

echo "==> Compiling release binary..."
cargo build --release --bin dj_player

echo "==> Assembling .app bundle..."
mkdir -p "$APP_DIR/Contents/MacOS"
mkdir -p "$APP_DIR/Contents/Resources"

cp target/release/dj_player "$APP_DIR/Contents/MacOS/DJ Player"

cat > "$APP_DIR/Contents/Info.plist" <<'PLIST'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleDevelopmentRegion</key>
    <string>en</string>
    <key>CFBundleDisplayName</key>
    <string>DJ Player</string>
    <key>CFBundleExecutable</key>
    <string>DJ Player</string>
    <key>CFBundleIdentifier</key>
    <string>com.hirenbudhrani.dj-player</string>
    <key>CFBundleInfoDictionaryVersion</key>
    <string>6.0</string>
    <key>CFBundleName</key>
    <string>DJ Player</string>
    <key>CFBundlePackageType</key>
    <string>APPL</string>
    <key>CFBundleShortVersionString</key>
    <string>1.0.0</string>
    <key>CFBundleVersion</key>
    <string>1.0.0</string>
    <key>LSApplicationCategoryType</key>
    <string>public.app-category.music</string>
    <key>LSMinimumSystemVersion</key>
    <string>11.0</string>
    <key>NSHighResolutionCapable</key>
    <true/>
    <key>NSHumanReadableCopyright</key>
    <string>Copyright © 2026 Hiren Budhrani</string>
</dict>
</plist>
PLIST

echo "==> Ad-hoc code signing..."
codesign --force --deep -s - "$APP_DIR"

echo ""
echo "Done! Bundle at:"
echo "  $APP_DIR"
echo ""
echo "To install, drag it to ~/Applications."
echo ""
echo "If macOS blocks it on first launch:"
echo "  xattr -cr \"$APP_DIR\""
echo "Then right-click > Open."
