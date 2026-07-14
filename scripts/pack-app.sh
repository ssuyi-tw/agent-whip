#!/usr/bin/env bash
# Package agent-whip as a macOS .app bundle (menu-bar agent) so it can be
# launched from Finder / Spotlight / Raycast. Builds to ./dist/AgentWhip.app.
#
#   scripts/pack-app.sh            # build the bundle into dist/
#   scripts/pack-app.sh --install  # also copy it to /Applications
set -euo pipefail
cd "$(dirname "$0")/.."

APP="dist/AgentWhip.app"
BIN="target/release/agent-whip"
SRC_ICON="assets/icon/whip-appicon.png"

echo "==> building release binary"
cargo build --release

echo "==> assembling $APP"
rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
cp "$BIN" "$APP/Contents/MacOS/agent-whip"

# Build AppIcon.icns from the whip app-icon PNG.
ICONSET="dist/AppIcon.iconset"
rm -rf "$ICONSET"; mkdir -p "$ICONSET"
for s in 16 32 128 256 512; do
  sips -z "$s" "$s"       "$SRC_ICON" --out "$ICONSET/icon_${s}x${s}.png"    >/dev/null
  sips -z "$((s*2))" "$((s*2))" "$SRC_ICON" --out "$ICONSET/icon_${s}x${s}@2x.png" >/dev/null
done
iconutil -c icns "$ICONSET" -o "$APP/Contents/Resources/AppIcon.icns"
rm -rf "$ICONSET"

cat > "$APP/Contents/Info.plist" <<'PLIST'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleName</key><string>AgentWhip</string>
  <key>CFBundleDisplayName</key><string>agent-whip</string>
  <key>CFBundleIdentifier</key><string>com.github.ssuyi-tw.agent-whip</string>
  <key>CFBundleExecutable</key><string>agent-whip</string>
  <key>CFBundleIconFile</key><string>AppIcon</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleShortVersionString</key><string>0.1.0</string>
  <key>CFBundleVersion</key><string>0.1.0</string>
  <key>LSMinimumSystemVersion</key><string>11.0</string>
  <key>LSUIElement</key><true/>
  <key>NSHighResolutionCapable</key><true/>
</dict>
</plist>
PLIST

# Ad-hoc sign so the app has a stable identity (keeps the Accessibility grant
# from resetting on every rebuild).
codesign --force --sign - "$APP" >/dev/null 2>&1 || echo "   (codesign skipped)"

echo "==> built $APP"

if [[ "${1:-}" == "--install" ]]; then
  if cp -R "$APP" /Applications/ 2>/dev/null; then
    echo "==> installed to /Applications/AgentWhip.app"
  else
    mkdir -p "$HOME/Applications"
    cp -R "$APP" "$HOME/Applications/"
    echo "==> installed to ~/Applications/AgentWhip.app"
  fi
fi
