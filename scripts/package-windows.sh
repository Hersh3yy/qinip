#!/usr/bin/env bash
# Build a portable Windows zip for Qinip (run on Windows or in CI).
#
# Usage:
#   bash scripts/package-windows.sh
#
# Output:
#   target/release/qinip-windows-x64.zip

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(dirname "$SCRIPT_DIR")"
OUT_DIR="$REPO_ROOT/target/release"
ZIP_NAME="qinip-windows-x64.zip"

cd "$REPO_ROOT"

echo "==> Compiling release binary (Windows)..."
cargo build --release --bin dj_player

EXE="$OUT_DIR/dj_player.exe"
if [ ! -f "$EXE" ]; then
  echo "ERROR: expected $EXE" >&2
  exit 1
fi

STAGE="$OUT_DIR/qinip-windows"
rm -rf "$STAGE"
mkdir -p "$STAGE"
cp "$EXE" "$STAGE/Qinip.exe"
cp "$REPO_ROOT/USER_MANUAL.md" "$STAGE/" 2>/dev/null || true
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

echo ""
echo "Done! Archive at:"
echo "  $OUT_DIR/$ZIP_NAME"
echo ""
echo "Unzip and run Qinip.exe"
