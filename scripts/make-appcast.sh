#!/usr/bin/env bash
# Generate + EdDSA-sign appcast.xml for the current release from the built DMG,
# using Sparkle's generate_appcast tool. Run this on the machine holding the
# Sparkle private EdDSA key (in its login Keychain) — i.e. the release signer.
#
#   scripts/make-dmg.sh        # build the notarized DMG first
#   scripts/make-appcast.sh    # then generate/sign the appcast
#
# Produces ./appcast.xml with enclosure URLs pointing at this version's GitHub
# Releases download. Commit appcast.xml to main — that's the SUFeedURL the app
# polls.
set -euo pipefail
cd "$(dirname "$0")/.."

SPARKLE_VERSION="2.9.4"
SPARKLE_SHA256="ce89daf967db1e1893ed3ebd67575ed82d3902563e3191ca92aaec9164fbdef9"
REPO="ssuyi-tw/agent-whip"
VERSION="$(grep -m1 '^version' Cargo.toml | sed -E 's/.*"(.*)".*/\1/')"
DMG="dist/AgentWhip-${VERSION}.dmg"
[[ -f "$DMG" ]] || { echo "!! $DMG not found — run scripts/make-dmg.sh first"; exit 1; }

# Fetch the Sparkle tools (checksum-pinned) and locate generate_appcast.
TOOLS="dist/.sparkle-tools-${SPARKLE_VERSION}"
GA="$TOOLS/bin/generate_appcast"
if [[ ! -x "$GA" ]]; then
  echo "==> downloading Sparkle ${SPARKLE_VERSION} tools"
  mkdir -p "$TOOLS"
  TARBALL="$TOOLS/Sparkle.tar.xz"
  curl -fsSL -o "$TARBALL" \
    "https://github.com/sparkle-project/Sparkle/releases/download/${SPARKLE_VERSION}/Sparkle-${SPARKLE_VERSION}.tar.xz"
  echo "${SPARKLE_SHA256}  ${TARBALL}" | shasum -a 256 -c - \
    || { echo "!! Sparkle checksum mismatch; aborting"; exit 1; }
  tar -xJf "$TARBALL" -C "$TOOLS" bin
  rm -f "$TARBALL"
fi

# Stage just this version's DMG so the per-release download URL prefix is right.
# Seed the existing appcast so prior entries are preserved.
STAGE="$(mktemp -d)"; trap 'rm -rf "$STAGE"' EXIT
cp "$DMG" "$STAGE/"
[[ -f appcast.xml ]] && cp appcast.xml "$STAGE/appcast.xml"

echo "==> generating + signing appcast for v${VERSION}"
# Reads the private EdDSA key from your login Keychain (set up once with
# bin/generate_keys). Use --ed-key-file to point at a key file instead.
"$GA" \
  --download-url-prefix "https://github.com/${REPO}/releases/download/v${VERSION}/" \
  -o "$STAGE/appcast.xml" \
  "$STAGE"

cp "$STAGE/appcast.xml" appcast.xml
echo "==> wrote appcast.xml"
echo "    Next: create the v${VERSION} GitHub release, upload ${DMG##*/},"
echo "    then commit appcast.xml to main so the SUFeedURL serves it."
