#!/usr/bin/env bash
set -euo pipefail

# Package waitagent binary into a .dmg for macOS distribution.
# Usage: ./scripts/package-dmg.sh <binary-path> <output-dmg> [version]

BINARY="${1:?missing binary path}"
OUTPUT_DMG="${2:?missing output dmg path}"
VERSION="${3:-0.1.0}"

if [[ ! -f "$BINARY" ]]; then
  echo "error: binary not found at $BINARY" >&2
  exit 1
fi

STAGING="$(mktemp -d)"
trap 'rm -rf "$STAGING"' EXIT

APP_NAME="WaitAgent"
APP_DIR="$STAGING/$APP_NAME.app"
mkdir -p "$APP_DIR/Contents/MacOS"

cp "$BINARY" "$APP_DIR/Contents/MacOS/waitagent"
chmod +x "$APP_DIR/Contents/MacOS/waitagent"

# Create a minimal Info.plist
cat > "$APP_DIR/Contents/Info.plist" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN"
  "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleExecutable</key>
  <string>waitagent</string>
  <key>CFBundleIdentifier</key>
  <string>com.waitagent.cli</string>
  <key>CFBundleName</key>
  <string>WaitAgent</string>
  <key>CFBundleVersion</key>
  <string>${VERSION}</string>
  <key>CFBundleShortVersionString</key>
  <string>${VERSION}</string>
  <key>CFBundlePackageType</key>
  <string>APPL</string>
</dict>
</plist>
EOF

ln -s /Applications "$STAGING/Applications"

# Create compressed DMG directly (single-pass, no intermediate RW image).
hdiutil create -volname "$APP_NAME" -srcfolder "$STAGING" \
  -ov -format UDZO -imagekey zlib-level=9 "$OUTPUT_DMG"

echo "created $OUTPUT_DMG"
