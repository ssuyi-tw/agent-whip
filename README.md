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

## Permissions

The whip needs to read the global cursor and synthesize keystrokes, so it
requires OS input access:

- **macOS** — grant **Accessibility** permission. On first whip a system prompt
  appears (System Settings ▸ Privacy & Security ▸ Accessibility); enable
  `agent-whip`, then relaunch. Without it the app runs but the whip won't spawn.
- **Linux (X11)** — works out of the box. Wayland input injection is limited
  (same caveat as the original's `xdotool`).
- **Windows** — works out of the box.

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
