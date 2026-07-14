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
  <key>CFBundleShortVersionString</key><string>0.2.0</string>
  <key>CFBundleVersion</key><string>0.2.0</string>
  <key>LSMinimumSystemVersion</key><string>11.0</string>
  <key>LSUIElement</key><true/>
  <key>NSHighResolutionCapable</key><true/>
</dict>
</plist>
PLIST

# Sign with a STABLE identity if one exists, so macOS keeps the Accessibility
# grant across rebuilds. Ad-hoc (`-`) pins the signature to the binary's cdhash,
# which changes every build → the grant resets. A real cert (Developer ID, or a
# self-signed "agent-whip" code-signing cert) gives a stable designated
# requirement. Override with SIGN_ID=... ; falls back to ad-hoc if none found.
if [[ -z "${SIGN_ID:-}" ]]; then
  SIGN_ID="$(security find-identity -v -p codesigning 2>/dev/null \
             | grep -m1 'Developer ID Application' | sed -E 's/.*"(.*)"/\1/' || true)"
fi
if [[ -z "${SIGN_ID:-}" ]]; then
  SIGN_ID="$(security find-identity -v -p codesigning 2>/dev/null \
             | grep -m1 'agent-whip' | sed -E 's/.*"(.*)"/\1/' || true)"
fi
SIGN_ID="${SIGN_ID:--}"
if [[ "$SIGN_ID" == "-" ]]; then
  echo "   (ad-hoc signing — Accessibility grant will reset on rebuild; make a stable cert, see README)"
else
  echo "   (signing with stable identity: $SIGN_ID)"
fi
codesign --force --sign "$SIGN_ID" "$APP" >/dev/null 2>&1 || echo "   (codesign skipped)"

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
