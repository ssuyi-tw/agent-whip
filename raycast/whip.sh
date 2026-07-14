#!/bin/bash

# Raycast Script Command — summon (or drop) the whip.
# Add this folder in Raycast: Settings → Extensions → Script Commands →
# "Add Script Directory" → pick this repo's `raycast/` folder. Then run "Whip
# the agent" (and optionally assign it a Raycast hotkey/alias).
#
# @raycast.schemaVersion 1
# @raycast.title Whip the agent
# @raycast.mode silent
# @raycast.icon 😩
# @raycast.packageName agent-whip
# @raycast.description Crack the whip — summon/drop the agent-whip overlay

PIDFILE="${XDG_CONFIG_HOME:-$HOME/.config}/agent-whip/agent-whip.pid"

if [ -f "$PIDFILE" ] && kill -0 "$(cat "$PIDFILE")" 2>/dev/null; then
  kill -USR1 "$(cat "$PIDFILE")"   # running → toggle the whip
else
  open -a AgentWhip                # not running → launch it (run again to whip)
fi
