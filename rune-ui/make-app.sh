#!/usr/bin/env bash
# Wrap the Rune spike in a minimal macOS app bundle so Spotlight (Cmd+Space) can launch it
# as "Qinip Rune". Builds the release binary first; installs into ~/Applications.
#
#   bash rune-ui/make-app.sh
set -euo pipefail
HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(dirname "$HERE")"
APP="$HOME/Applications/Qinip Rune.app"

cd "$HERE" && cargo build --release --quiet

rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
cp "$HERE/target/release/qinip-rune" "$APP/Contents/MacOS/Qinip Rune"

# Same icon as Qinip, built the way scripts/package-macos.sh does it.
ICONSET="$(mktemp -d)/icon.iconset"; mkdir -p "$ICONSET"
for size in 16 32 128 256 512; do
  sips -z "$size" "$size" "$ROOT/assets/icon.png" --out "$ICONSET/icon_${size}x${size}.png" >/dev/null
  sips -z $((size*2)) $((size*2)) "$ROOT/assets/icon.png" --out "$ICONSET/icon_${size}x${size}@2x.png" >/dev/null
done
iconutil -c icns "$ICONSET" -o "$APP/Contents/Resources/AppIcon.icns"

cat > "$APP/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
  <key>CFBundleName</key><string>Qinip Rune</string>
  <key>CFBundleDisplayName</key><string>Qinip Rune</string>
  <key>CFBundleExecutable</key><string>Qinip Rune</string>
  <key>CFBundleIdentifier</key><string>com.hirenbudhrani.qinip-rune</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleShortVersionString</key><string>0.1.0</string>
  <key>CFBundleVersion</key><string>$(git -C "$ROOT" rev-list --count HEAD)</string>
  <key>CFBundleIconFile</key><string>AppIcon</string>
  <key>LSMinimumSystemVersion</key><string>11.0</string>
  <key>NSHighResolutionCapable</key><true/>
  <key>LSApplicationCategoryType</key><string>public.app-category.music</string>
</dict></plist>
PLIST
codesign --force -s - "$APP" 2>/dev/null
echo "Installed: $APP  (Cmd+Space, type: Qinip Rune)"
