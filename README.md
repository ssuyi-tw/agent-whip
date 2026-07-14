# agent-whip

Sometimes your coding agent is going too shlow, and you must whip it into shape.

A **native Rust** port of [OpenWhip](https://github.com/GitFrog1111/OpenWhip) —
no Electron, no browser engine, one self-contained binary. It lives in the
system tray; click it to summon a bullwhip that follows your cursor. Flick the
whip and *crack* — it plays a sound and fires an interrupt (`Ctrl-C`) plus an
encouraging phrase into whatever app you were just using.

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
- **The crack** — flick the tip past the speed threshold and it plays one of
  five crack sounds and sends `Ctrl-C` + a random phrase (`FASTER`,
  `GO FASTER`, `Speed it up clanker`, …) + `Enter` to the focused app.

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

Then find **AgentWhip** in Raycast/Spotlight. (The app bundle's binary is a new
identity, so macOS will ask for Accessibility again on first whip.)

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

## Configure the crack prompt

What the whip types after it cracks is read from a config file, created with
defaults on first run and **re-read on every crack** — so edit, save, and the
next crack uses it (no restart, no rebuild):

```
~/.config/agent-whip/config.toml      # or $XDG_CONFIG_HOME/agent-whip/config.toml
```
```toml
# One line is picked at random each crack.
phrases = [
  "FASTER",
  "GO FASTER",
  "Work FASTER",
  "Speed it up clanker",
]

send_interrupt = true   # send Ctrl-C before typing
send_enter     = true   # press Enter after typing
```

If the file is missing or invalid, the built-in defaults are used.

## How this differs from OpenWhip

| | OpenWhip | agent-whip |
|---|---|---|
| Runtime | Electron + Node | single native binary |
| UI / physics | HTML `<canvas>` + JS | `tiny-skia` + `wgpu`, all Rust |
| Keystrokes | `keybd_event` FFI / `osascript` / `xdotool` | `enigo` (one backend) |
| Cursor input | overlay captures mouse | global polling (`device_query`) |
| Sound | Electron `Audio` | `rodio`, clips embedded in the binary |

## Credits

Port of [OpenWhip](https://github.com/GitFrog1111/OpenWhip) by GitFrog1111. Whip
sounds and icons are carried over from that project. MIT licensed — see
[`LICENSE`](LICENSE).
