#!/usr/bin/env bash
# Code-sign an AgentWhip.app *inside-out*: the embedded Sparkle.framework's
# nested helpers first, then the framework, then the app itself. A single
# top-level `codesign` leaves the framework's nested Mach-O code unsigned — and
# under hardened runtime + library validation those helpers must be re-signed
# with the app's own identity, or the framework won't load at all.
#
#   scripts/sign-bundle.sh <app-path> <identity|-> [runtime]
#     identity : a codesigning identity, or "-" for ad-hoc.
#     runtime  : pass "runtime" to add --options runtime --timestamp
#                (required for Developer ID signing + notarization).
set -euo pipefail

APP="${1:?usage: sign-bundle.sh <app> <identity|-> [runtime]}"
ID="${2:?missing signing identity (use - for ad-hoc)}"
MODE="${3:-}"

OPTS=(--force --sign "$ID")
[[ "$MODE" == "runtime" ]] && OPTS+=(--options runtime --timestamp)
sign() { echo "   sign: ${1#"$APP"/}"; codesign "${OPTS[@]}" "$1"; }

FW="$APP/Contents/Frameworks/Sparkle.framework"
if [[ -d "$FW" ]]; then
  V="$FW/Versions/B"
  for nested in \
    "$V/XPCServices/Downloader.xpc" \
    "$V/XPCServices/Installer.xpc" \
    "$V/Autoupdate" \
    "$V/Updater.app"; do
    [[ -e "$nested" ]] && sign "$nested"
  done
  sign "$FW"
fi
sign "$APP"
