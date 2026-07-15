#!/usr/bin/env bash
# Build a distributable macOS DMG of agent-whip: assemble the .app, sign it with
# a Developer ID + hardened runtime, wrap it in a DMG, notarize, and staple — so
# it opens on other people's Macs with no Gatekeeper warning.
#
#   scripts/make-dmg.sh
#
# Requires (for the full signed + notarized path):
#   1. An Apple Developer Program membership.
#   2. A "Developer ID Application" certificate in your login keychain.
#      Check with:  security find-identity -v -p codesigning
#   3. Notary credentials. Easiest is a stored notarytool keychain profile:
#        xcrun notarytool store-credentials agent-whip-notary \
#          --apple-id "you@example.com" --team-id "TEAMID" \
#          --password "app-specific-password"
#
# Configure via environment (all optional — the script auto-detects/degrades):
#   DEVID           Developer ID Application identity string. Auto-detected from
#                   the keychain if unset.
#   NOTARY_PROFILE  notarytool keychain profile name (default: agent-whip-notary).
#   NOTARY_APPLE_ID / NOTARY_TEAM_ID / NOTARY_PASSWORD
#                   Alternative to a profile (app-specific password).
#
# With no Developer ID present, it still produces an ad-hoc-signed DMG (mountable,
# but recipients must right-click → Open the first time) and skips notarization.
set -euo pipefail
cd "$(dirname "$0")/.."

APP="dist/AgentWhip.app"
VERSION="$(grep -m1 '^version' Cargo.toml | sed -E 's/.*"(.*)".*/\1/')"
DMG="dist/AgentWhip-${VERSION}.dmg"
VOLNAME="AgentWhip"

# --- 1. assemble the .app bundle (reuses the packaging script) ----------------
echo "==> assembling app bundle"
scripts/pack-app.sh

# --- 2. resolve the signing identity ------------------------------------------
if [[ -z "${DEVID:-}" ]]; then
  DEVID="$(security find-identity -v -p codesigning 2>/dev/null \
            | grep -m1 'Developer ID Application' \
            | sed -E 's/.*"(.*)"/\1/' || true)"
fi

if [[ -n "${DEVID:-}" ]]; then
  SIGNED=1
  echo "==> signing with: $DEVID"
  # Hardened runtime + secure timestamp are required for notarization. The
  # bundle embeds Sparkle.framework, so sign inside-out (nested helpers, the
  # framework, then the app) — re-signing the framework with our own identity is
  # what lets it pass library validation under the hardened runtime.
  scripts/sign-bundle.sh "$APP" "$DEVID" runtime
else
  SIGNED=0
  echo "==> no Developer ID found — ad-hoc signing (DMG will need right-click → Open)"
  echo "    Get one via the Apple Developer Program, then re-run. See the header of this script."
  scripts/sign-bundle.sh "$APP" -
fi

echo "==> verifying app signature"
codesign --verify --strict --verbose=2 "$APP"

# --- 3. build the DMG ---------------------------------------------------------
echo "==> building $DMG"
STAGE="$(mktemp -d)"
trap 'rm -rf "$STAGE"' EXIT
cp -R "$APP" "$STAGE/"
ln -s /Applications "$STAGE/Applications"   # drag-to-install target
rm -f "$DMG"
hdiutil create -volname "$VOLNAME" -srcfolder "$STAGE" -ov -format UDZO "$DMG" >/dev/null

if [[ "$SIGNED" -eq 1 ]]; then
  echo "==> signing DMG"
  codesign --force --timestamp --sign "$DEVID" "$DMG"
fi

# --- 4. notarize + staple -----------------------------------------------------
if [[ "$SIGNED" -eq 0 ]]; then
  echo "==> skipping notarization (not signed with a Developer ID)"
  echo "==> done: $DMG (ad-hoc; recipients right-click → Open on first launch)"
  exit 0
fi

NOTARY_PROFILE="${NOTARY_PROFILE:-agent-whip-notary}"
NOTARY_ARGS=()
if xcrun notarytool history --keychain-profile "$NOTARY_PROFILE" >/dev/null 2>&1; then
  NOTARY_ARGS=(--keychain-profile "$NOTARY_PROFILE")
elif [[ -n "${NOTARY_APPLE_ID:-}" && -n "${NOTARY_TEAM_ID:-}" && -n "${NOTARY_PASSWORD:-}" ]]; then
  NOTARY_ARGS=(--apple-id "$NOTARY_APPLE_ID" --team-id "$NOTARY_TEAM_ID" --password "$NOTARY_PASSWORD")
else
  echo "==> DMG is signed but NOT notarized: no notary credentials found."
  echo "    Store some once, then re-run:"
  echo "      xcrun notarytool store-credentials $NOTARY_PROFILE \\"
  echo "        --apple-id you@example.com --team-id TEAMID --password app-specific-pw"
  echo "==> done (unnotarized): $DMG"
  exit 0
fi

echo "==> notarizing (this uploads the DMG and waits for Apple)"
xcrun notarytool submit "$DMG" "${NOTARY_ARGS[@]}" --wait

echo "==> stapling ticket"
xcrun stapler staple "$DMG"

echo "==> verifying Gatekeeper acceptance"
xcrun stapler validate "$DMG"
spctl -a -t open --context context:primary-signature -v "$DMG" || true

echo "==> done: $DMG (signed + notarized)"
