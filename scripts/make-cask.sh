#!/usr/bin/env bash
# Stamp the current version + DMG sha256 into the Homebrew cask (in the tap repo),
# so each release ships an accurate cask. Run after the DMG is built/published.
#
#   scripts/make-dmg.sh      # build the notarized DMG first
#   scripts/make-cask.sh     # then update the cask
#
# Hashes dist/AgentWhip-<version>.dmg if present (the same file you upload to the
# release), else downloads the published v<version> release asset. Updates the
# cask at $TAP/Casks/agent-whip.rb in place — only the version + sha256 lines, so
# manual edits (desc, zap, livecheck) survive. TAP defaults to ../homebrew-tap.
# Commit + push the tap afterwards.
set -euo pipefail
cd "$(dirname "$0")/.."

REPO="ssuyi-tw/agent-whip"
VERSION="$(grep -m1 '^version' Cargo.toml | sed -E 's/.*"(.*)".*/\1/')"
TAP="${TAP:-../homebrew-tap}"
CASK="$TAP/Casks/agent-whip.rb"
DMG="dist/AgentWhip-${VERSION}.dmg"

[[ -f "$CASK" ]] || { echo "!! cask not found at $CASK (set TAP=/path/to/homebrew-tap)"; exit 1; }

if [[ -f "$DMG" ]]; then
  echo "==> hashing local $DMG"
else
  TMPD="$(mktemp -d)"; trap 'rm -rf "$TMPD"' EXIT
  DMG="$TMPD/AgentWhip-${VERSION}.dmg"
  URL="https://github.com/${REPO}/releases/download/v${VERSION}/AgentWhip-${VERSION}.dmg"
  echo "==> downloading published $URL"
  curl -fsSL -o "$DMG" "$URL" \
    || { echo "!! no local dist DMG and download failed — is v${VERSION} released?"; exit 1; }
fi
SHA="$(shasum -a 256 "$DMG" | awk '{print $1}')"
echo "==> version ${VERSION}  sha256 ${SHA}"

# Stamp version + sha256 in place (BSD sed; leaves the rest of the cask untouched).
sed -i '' -E \
  -e "s/^  version \".*\"/  version \"${VERSION}\"/" \
  -e "s/^  sha256 \".*\"/  sha256 \"${SHA}\"/" \
  "$CASK"

echo "==> updated $CASK"
echo "    Next: cd $TAP && git commit -am \"agent-whip ${VERSION}\" && git push"
