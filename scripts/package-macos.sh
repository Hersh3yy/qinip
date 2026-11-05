#!/usr/bin/env bash
# Build a macOS .app bundle (and .dmg + .zip) for Qinip.
#
# Usage:
#   ./scripts/package-macos.sh
#
# Two modes, picked by environment:
#
#   Local / dev (default)      ad-hoc signed. Runs on this Mac; Gatekeeper
#                              blocks it on other Macs.
#
#   Release (trustworthy)      Developer ID signed + hardened runtime, then
#                              notarized by Apple and stapled. Opens cleanly
#                              on any Mac. Needs:
#     SIGN_IDENTITY   "Developer ID Application: Your Name (TEAMID)"
#                     (see: security find-identity -v -p codesigning)
#     NOTARY_PROFILE  a notarytool keychain profile, created once with
#                     xcrun notarytool store-credentials <name> ...
#                     Optional: without it the build is signed but not
#                     notarized (useful to test signing on its own).
#
# Output (target/release/bundle/):
#   osx/Qinip.app
#   qinip-macos-arm64.dmg   (stable name, so a GitHub "latest" link never changes)
#   qinip-macos-arm64.zip

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(dirname "$SCRIPT_DIR")"
BUNDLE_DIR="$REPO_ROOT/target/release/bundle"
APP_DIR="$BUNDLE_DIR/osx/Qinip.app"
ENTITLEMENTS="$SCRIPT_DIR/qinip.entitlements"

SIGN_IDENTITY="${SIGN_IDENTITY:-}"
NOTARY_PROFILE="${NOTARY_PROFILE:-}"

cd "$REPO_ROOT"

VERSION="$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)"
# CFBundleVersion must only ever go up; the commit count does that for free.
BUILD_NUMBER="$(git rev-list --count HEAD 2>/dev/null || echo 1)"
DMG_PATH="$BUNDLE_DIR/qinip-macos-arm64.dmg"
ZIP_PATH="$BUNDLE_DIR/qinip-macos-arm64.zip"

if [ -n "$NOTARY_PROFILE" ] && [ -z "$SIGN_IDENTITY" ]; then
  echo "NOTARY_PROFILE is set but SIGN_IDENTITY is not. Notarization needs a Developer ID signature." >&2
  exit 1
fi

echo "==> Compiling release binary (v$VERSION, build $BUILD_NUMBER)..."
# Match the binary's minimum macOS to LSMinimumSystemVersion below.
MACOSX_DEPLOYMENT_TARGET=11.0 cargo build --release --locked --bin qinip

echo "==> Assembling .app bundle..."
rm -rf "$APP_DIR"
mkdir -p "$APP_DIR/Contents/MacOS"
mkdir -p "$APP_DIR/Contents/Resources"

cp target/release/qinip "$APP_DIR/Contents/MacOS/Qinip"

ICON_SRC="$REPO_ROOT/assets/icon.png"
ICONSET="$REPO_ROOT/target/icon.iconset"
if [ -f "$ICON_SRC" ]; then
  echo "==> Building AppIcon.icns..."
  rm -rf "$ICONSET"
  mkdir -p "$ICONSET"
  for size in 16 32 128 256 512; do
    sips -z "$size" "$size" "$ICON_SRC" --out "$ICONSET/icon_${size}x${size}.png" >/dev/null
    double=$((size * 2))
    sips -z "$double" "$double" "$ICON_SRC" --out "$ICONSET/icon_${size}x${size}@2x.png" >/dev/null
  done
  iconutil -c icns "$ICONSET" -o "$APP_DIR/Contents/Resources/AppIcon.icns"
  rm -rf "$ICONSET"
fi

# GPL-3.0 obliges shipping the license; MIT/Apache deps oblige their notices.
cp "$REPO_ROOT/LICENSE" "$APP_DIR/Contents/Resources/LICENSE.txt"
cp "$REPO_ROOT/assets/THIRD_PARTY_LICENSES.html" "$APP_DIR/Contents/Resources/"

cat > "$APP_DIR/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleDevelopmentRegion</key>
    <string>en</string>
    <key>CFBundleDisplayName</key>
    <string>Qinip</string>
    <key>CFBundleExecutable</key>
    <string>Qinip</string>
    <key>CFBundleIdentifier</key>
    <string>com.hirenbudhrani.qinip</string>
    <key>CFBundleInfoDictionaryVersion</key>
    <string>6.0</string>
    <key>CFBundleName</key>
    <string>Qinip</string>
    <key>CFBundlePackageType</key>
    <string>APPL</string>
    <key>CFBundleShortVersionString</key>
    <string>$VERSION</string>
    <key>CFBundleVersion</key>
    <string>$BUILD_NUMBER</string>
    <key>CFBundleIconFile</key>
    <string>AppIcon</string>
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

# One Mach-O, no C libraries, no dylibs or frameworks: sign the bundle
# once, no --deep (Apple deprecates it for anything but flat bundles).
if [ -n "$SIGN_IDENTITY" ]; then
  echo "==> Signing with Developer ID + hardened runtime..."
  codesign --force --timestamp --options runtime \
    --entitlements "$ENTITLEMENTS" \
    --sign "$SIGN_IDENTITY" \
    "$APP_DIR"
  codesign --verify --strict --verbose=2 "$APP_DIR"
else
  echo "==> Ad-hoc signing (local build; set SIGN_IDENTITY for a release)..."
  codesign --force -s - "$APP_DIR"
fi

if [ -n "$NOTARY_PROFILE" ]; then
  echo "==> Notarizing .app (usually 1 to 5 minutes)..."
  NOTARIZE_ZIP="$BUNDLE_DIR/qinip-notarize.zip"
  rm -f "$NOTARIZE_ZIP"
  ditto -c -k --sequesterRsrc --keepParent "$APP_DIR" "$NOTARIZE_ZIP"
  xcrun notarytool submit "$NOTARIZE_ZIP" --keychain-profile "$NOTARY_PROFILE" --wait
  rm -f "$NOTARIZE_ZIP"
  # The ticket lives inside the bundle after stapling, so it opens offline too.
  xcrun stapler staple "$APP_DIR"
  echo "==> Gatekeeper assessment:"
  spctl --assess --type execute --verbose=4 "$APP_DIR"
fi

echo "==> Building .dmg..."
rm -f "$DMG_PATH"
DMG_STAGE="$BUNDLE_DIR/dmg-stage"
rm -rf "$DMG_STAGE"
mkdir -p "$DMG_STAGE"
cp -R "$APP_DIR" "$DMG_STAGE/"
# The Applications shortcut gives the familiar drag-to-install window.
ln -s /Applications "$DMG_STAGE/Applications"
# Without Apple notarization, macOS blocks the first launch. Say how to get
# past it right where the user is looking.
if [ -z "$NOTARY_PROFILE" ]; then
  cat > "$DMG_STAGE/If macOS blocks Qinip.txt" <<'TXT'
Qinip is free and not notarized by Apple (that costs 99 USD a year), so the
first time you open it macOS says it can't check it for malicious software.

To open it anyway (only needed once):

1. Drag Qinip into Applications and double-click it. macOS blocks it. Click Done.
2. Open System Settings > Privacy & Security.
3. Scroll down. Next to "Qinip was blocked", click Open Anyway.
4. Confirm with your password or Touch ID, then click Open.

From then on Qinip opens normally.

Source code: https://github.com/Hersh3yy/qinip
Check your download against SHA256SUMS.txt on the release page:
  shasum -a 256 ~/Downloads/qinip-macos-arm64.dmg
TXT
fi
hdiutil create -volname "Qinip" -srcfolder "$DMG_STAGE" -ov -format UDZO "$DMG_PATH" >/dev/null
rm -rf "$DMG_STAGE"

if [ -n "$SIGN_IDENTITY" ]; then
  codesign --force --timestamp --sign "$SIGN_IDENTITY" "$DMG_PATH"
fi
if [ -n "$NOTARY_PROFILE" ]; then
  echo "==> Notarizing .dmg..."
  xcrun notarytool submit "$DMG_PATH" --keychain-profile "$NOTARY_PROFILE" --wait
  xcrun stapler staple "$DMG_PATH"
fi

echo "==> Zipping .app..."
rm -f "$ZIP_PATH"
ditto -c -k --sequesterRsrc --keepParent "$APP_DIR" "$ZIP_PATH"

echo ""
echo "Done."
echo "  App: $APP_DIR"
echo "  DMG: $DMG_PATH"
echo "  ZIP: $ZIP_PATH"
echo ""
if [ -n "$NOTARY_PROFILE" ]; then
  echo "Signed + notarized + stapled. Opens cleanly on any Mac."
elif [ -n "$SIGN_IDENTITY" ]; then
  echo "Developer ID signed, NOT notarized. Set NOTARY_PROFILE before shipping."
else
  echo "Ad-hoc build: fine on this Mac, blocked by Gatekeeper elsewhere."
  echo "Install locally: cp -R \"$APP_DIR\" ~/Applications/"
fi
