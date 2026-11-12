#!/usr/bin/env bash
# Build the Windows downloads for Qinip (run on Windows or in CI):
# a portable zip, and the installer when Inno Setup is installed
# (https://jrsoftware.org/isinfo.php; CI installs it with choco).
#
# Usage:
#   bash scripts/package-windows.sh
#
# Output:
#   target/release/qinip-windows-x64.zip
#   target/release/qinip-setup-x64.exe   (if ISCC is available)

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(dirname "$SCRIPT_DIR")"
OUT_DIR="$REPO_ROOT/target/release"
ZIP_NAME="qinip-windows-x64.zip"

cd "$REPO_ROOT"

echo "==> Compiling release binary (Windows)..."
cargo build --release --bin qinip

EXE="$OUT_DIR/qinip.exe"
if [ ! -f "$EXE" ]; then
  echo "ERROR: expected $EXE" >&2
  exit 1
fi

STAGE="$OUT_DIR/qinip-windows"
rm -rf "$STAGE"
mkdir -p "$STAGE"
cp "$EXE" "$STAGE/Qinip.exe"
cp "$REPO_ROOT/README.md" "$STAGE/" 2>/dev/null || true

(
  cd "$OUT_DIR"
  rm -f "$ZIP_NAME"
  if command -v powershell.exe >/dev/null 2>&1; then
    powershell.exe -NoProfile -Command "Compress-Archive -Path 'qinip-windows\\*' -DestinationPath '$ZIP_NAME' -Force"
  elif command -v zip >/dev/null 2>&1; then
    (cd qinip-windows && zip -r "../$ZIP_NAME" .)
  else
    echo "ERROR: need zip or powershell to create archive" >&2
    exit 1
  fi
)

ISCC=""
for candidate in "/c/Program Files (x86)/Inno Setup 6/ISCC.exe" "$(command -v iscc 2>/dev/null || true)" "$(command -v ISCC 2>/dev/null || true)"; do
  if [ -n "$candidate" ] && [ -x "$candidate" ]; then ISCC="$candidate"; break; fi
done
if [ -n "$ISCC" ]; then
  VERSION="$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)"
  echo "==> Building installer (Inno Setup)..."
  # MSYS_NO_PATHCONV: Git Bash must not rewrite "/D..." into a drive path.
  MSYS_NO_PATHCONV=1 "$ISCC" "/DAppVersion=$VERSION" scripts/windows/qinip.iss
else
  echo "==> Inno Setup not found; skipping the installer (zip only)."
fi

echo ""
echo "Done!"
echo "  Portable zip: $OUT_DIR/$ZIP_NAME"
[ -n "$ISCC" ] && echo "  Installer:    $OUT_DIR/qinip-setup-x64.exe"
echo ""
