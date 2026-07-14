<img src="assets/icon/agent_whip_icon_v2.png" width="120">

# agent-whip

Sometimes your coding agent is going too shlow, and you must whip it into shape.

A **native Rust** port of [OpenWhip](https://github.com/GitFrog1111/OpenWhip) —
no Electron, no browser engine, one self-contained binary. It lives in the
system tray; click it to summon a bullwhip that follows your cursor. Flick the
whip and *crack* — it plays a sound and types a phrase into whatever app you
were just using. By default it types `/btw hold on`, which nudges Claude Code
*without* interrupting the running turn; Ctrl-C is opt-in (see below).

## What it does

- **Tray icon** — left-click to spawn the whip, left-click again (or click
  anywhere) to drop it. Right-click for a Quit menu.
- **Transparent overlay** — a full-screen, always-on-top, click-through window
  that draws the whip. It never steals focus, so keystrokes land in your
  terminal / editor, not the overlay.
- **Whip physics** — Verlet rope simulation with distance constraints, elastic
  bend limits, and screen-edge slaps, rendered as an anti-aliased Catmull-Rom
  spline. Ported verbatim from OpenWhip; the tunables live in `src/whip.rs` and
  `src/render.rs`.
- **The crack** — flick the tip past the speed threshold and it plays a crack
  sound and types a phrase + `Enter` into the focused app. The default phrase is
  `/btw hold on` (a non-interrupting Claude Code nudge); the phrase set, the
  optional `Ctrl-C`, and the sounds are all configurable (see below).

## Build & run

Requires a Rust toolchain (`rustup`, stable).

```bash
cargo run --release
# or, after building:
./target/release/agent-whip
```

Flags:

- `--dry-run` — do everything except inject keystrokes; the macro is logged
  instead. Handy for trying it out without interrupting anything.

## Install as a macOS app (Finder / Spotlight / Raycast)

To launch it from Raycast or Spotlight, package it as a `.app` bundle (a
menu-bar agent — `LSUIElement`, so no dock icon):

```bash
scripts/pack-app.sh --install   # builds dist/AgentWhip.app and copies it to /Applications
```

Then find **AgentWhip** in Raycast/Spotlight.

**Keep the Accessibility grant across rebuilds.** Ad-hoc signing pins the app's
identity to the binary hash, which changes every build — so macOS re-asks for
Accessibility each time. Sign with a real certificate for a stable identity and
the grant sticks. `pack-app.sh` auto-uses a **Developer ID Application** cert or a
self-signed **agent-whip** code-signing cert if it finds one (override with
`SIGN_ID=…`), else falls back to ad-hoc. To make a free self-signed one: Keychain
Access → Certificate Assistant → *Create a Certificate* → name `agent-whip`,
Identity Type *Self Signed Root*, Certificate Type *Code Signing*.

## Distribute a signed DMG

To hand the app to someone else's Mac without a Gatekeeper wall, build a signed,
notarized DMG:

```bash
scripts/make-dmg.sh    # → dist/AgentWhip-<version>.dmg
```

The full path needs an Apple Developer Program membership, a **Developer ID
Application** certificate in your keychain, and notary credentials stored once:

```bash
xcrun notarytool store-credentials agent-whip-notary \
  --apple-id "you@example.com" --team-id "TEAMID" --password "app-specific-pw"
```

With those present the script signs (hardened runtime), builds the DMG,
notarizes, and staples it. **Without** a Developer ID it still emits a mountable,
ad-hoc-signed DMG — recipients just right-click → *Open* the first time. See the
script header for the env vars (`DEVID`, `NOTARY_PROFILE`, …).

## Summon it without the tray icon

The running app listens for a toggle signal, so you can crack the whip from
anywhere — handy when the menu-bar icon is buried among other apps.

**Command line** (put `agent-whip` on your PATH with `cargo install --path .`):

```bash
agent-whip whip                                     # summon / drop
# …or with no PATH install:
kill -USR1 "$(cat ~/.config/agent-whip/agent-whip.pid)"
```

**Raycast** — add this repo's `raycast/` folder as a script-command directory
(Raycast → Settings → Extensions → Script Commands → *Add Script Directory*),
then run **“Whip the agent.”** Assign it a Raycast hotkey/alias and you've got a
global keyboard shortcut — no extra permissions, nothing baked into the app. If
the app isn't running, the script launches it (run it again to whip).

## Permissions

The whip needs to read the global cursor and synthesize keystrokes, so it
requires OS input access:

- **macOS** — grant **Accessibility** permission. On first whip a system prompt
  appears (System Settings ▸ Privacy & Security ▸ Accessibility); enable
  `agent-whip`, then relaunch. Without it the app runs but the whip won't spawn.
- **Linux (X11)** — works out of the box. Wayland input injection is limited
  (same caveat as the original's `xdotool`).
- **Windows** — works out of the box.

## Configure the crack

What the whip types (and the sounds it plays) is read from a config file,
created with defaults on first run and **re-read on every crack** — so edit,
save, and the next crack uses it (no restart, no rebuild):

```
~/.config/agent-whip/config.toml      # or $XDG_CONFIG_HOME/agent-whip/config.toml
```
```toml
# One line is picked at random each crack. The default routes through Claude
# Code's /btw, which queues a note to the agent WITHOUT interrupting the
# running turn — that's why send_interrupt is off.
phrases = [
  "/btw hold on",
]

send_interrupt = false  # send Ctrl-C before typing (off: don't interrupt the turn)
send_enter     = true   # press Enter after typing

# Custom crack sounds — absolute paths or ~/…, one picked at random each crack.
# Leave empty to use the five built-in clips. Any format rodio decodes
# (wav, mp3, flac, ogg, …). Missing/unreadable files fall back to the built-ins.
sounds = [
  # "~/Sounds/my-crack.wav",
]
```

Want the old "whip it faster" behavior? Set `send_interrupt = true` and put your
own phrases (`"FASTER"`, `"GO FASTER"`, …) in `phrases`.

If the file is missing or invalid, the built-in defaults are used.

## Troubleshooting

Running as a menu-bar `.app` there's no terminal, so agent-whip logs to a file
you can tail:

```bash
tail -f /tmp/agent-whip.log
```

- **No sound after switching audio output** — fixed: the output device is
  reopened on each crack, so it follows the current system default (headphones,
  AirPods, a display's speakers). If it's still silent, the log will say why
  (e.g. `no audio output`, or a decode failure for a custom `sounds` file).
- **Whip won't spawn / nothing types** — almost always a missing Accessibility
  grant; the log says so. See Permissions above.

## How this differs from OpenWhip

| | OpenWhip | agent-whip |
|---|---|---|
| Runtime | Electron + Node | single native binary |
| UI / physics | HTML `<canvas>` + JS | `tiny-skia` + `wgpu`, all Rust |
| Keystrokes | `keybd_event` FFI / `osascript` / `xdotool` | `enigo` (one backend) |
| Cursor input | overlay captures mouse | global polling (`device_query`) |
| Sound | Electron `Audio` | `rodio`, clips embedded (or your own files) |

## Credits

Port of [OpenWhip](https://github.com/GitFrog1111/OpenWhip) by GitFrog1111. Whip
sounds and icons are carried over from that project. MIT licensed — see
[`LICENSE`](LICENSE).
