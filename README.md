<img src="assets/icon/agent_whip_icon_v2.png" width="120" alt="agent-whip icon">

# agent-whip

Sometimes your coding agent is going too slow, and you must whip it into shape.

agent-whip is a native Rust port of
[OpenWhip](https://github.com/GitFrog1111/OpenWhip). It lives in your system
tray and summons a bullwhip that follows your cursor. Flick it fast enough and
it cracks, plays a sound, and types a prompt into the app you were using.

By default, the prompt is `/btw hold on`. This nudges Claude Code without
interrupting its current turn. The prompt, sound, and optional `Ctrl-C` are all
configurable.

Unlike the original, agent-whip has no Electron or browser runtime: it is one
self-contained native binary.

## Install on macOS

Homebrew installs the notarized app and adds the `agent-whip` CLI to your
`PATH`:

```bash
brew install --cask ssuyi-tw/tap/agent-whip
```

Launch **AgentWhip** from Spotlight, Raycast, or Finder. Then:

1. Grant Accessibility access when macOS asks.
2. Click the menu-bar icon to summon the whip.
3. Move the cursor sharply to crack it.

You can also toggle the whip from a terminal while the app is running:

```bash
agent-whip whip
```

The app updates itself through Sparkle. To remove the app, CLI, configuration,
and caches:

```bash
brew uninstall --zap --cask agent-whip
```

## How it works

- **Menu-bar controls** — left-click the icon to summon or dismiss the whip.
  Right-click to toggle prompt injection, sound, and the roar, pick a **Skin**,
  check for updates, or quit.
- **Focus-safe overlay** — the transparent, always-on-top overlay never takes
  focus, so the generated keystrokes go to your terminal or editor.
- **Whip physics** — a Verlet rope simulation with distance constraints,
  elastic bend limits, and screen-edge slaps, rendered as an anti-aliased
  Catmull-Rom spline.
- **Configurable crack** — crossing the tip-speed threshold plays a sound and
  types a randomly selected prompt. Prompts, `Enter`, `Ctrl-C`, and sounds are
  configurable.
- **Guanzhang RRRRR roar** — layered over every crack, the signature roar from
  Notorious Whip. The embedded clip is auto-trimmed to its loudest ~1.1 s so the
  growl lands instantly, and rapid whips retrigger it instead of piling up.
  Toggle it from the tray.
- **Skins** — pick the whip's material from the tray "Skin" submenu: Classic
  (black & white), Notorious (braided leather + red glow), Chrome, Gold, or
  Neon. The choice is remembered across restarts.

## Configure the crack

On first launch, agent-whip creates:

```text
~/.config/agent-whip/config.toml
```

If `XDG_CONFIG_HOME` is set, the file lives at
`$XDG_CONFIG_HOME/agent-whip/config.toml` instead. The file is re-read on every
crack, so changes apply without a restart.

```toml
# One phrase is selected at random for each crack.
phrases = [
  "/btw hold on",
]

send_interrupt = false # send Ctrl-C before typing
send_enter = true      # press Enter after typing

# One sound is selected at random for each crack. Paths may be absolute or
# start with ~/. Leave the list empty to use the five built-in clips.
sounds = [
  # "~/Sounds/my-crack.wav",
]
```

Supported custom sound formats include WAV, MP3, FLAC, and OGG. Missing or
unreadable files fall back to the built-in clips. If the config file is invalid
or contains no phrases, agent-whip uses its defaults.

To restore the original “whip it faster” behavior, set
`send_interrupt = true` and replace `phrases` with prompts such as `"FASTER"`
or `"GO FASTER"`.

## Other ways to summon the whip

### Command line

`agent-whip whip` toggles an already-running app. If you built from source and
the CLI is not on your `PATH`, send the same signal directly:

```bash
kill -USR1 "$(cat ~/.config/agent-whip/agent-whip.pid)"
```

### Raycast

Add the repository's `raycast/` directory in **Raycast → Settings → Extensions
→ Script Commands → Add Script Directory**, then run **Whip the agent**. Assign
it a hotkey or alias for a global shortcut.

If AgentWhip is not running, the command launches it. Run the command again to
summon the whip.

## Permissions and platform support

agent-whip reads the global cursor position and synthesizes keystrokes, so its
input-access requirements vary by platform:

- **macOS** — requires Accessibility access. Open **System Settings → Privacy &
  Security → Accessibility**, enable AgentWhip, and relaunch it. Without this
  permission, the app runs but the whip does not appear.
- **Linux (X11)** — works without additional setup. Input injection under
  Wayland is limited, as it is for the original project's `xdotool` backend.
- **Windows** — works without additional setup.

The packaged app, Homebrew cask, Sparkle updates, and Raycast command are
macOS-specific. The Rust binary itself is cross-platform.

## Build from source

Install a stable Rust toolchain with [rustup](https://rustup.rs/), then run:

```bash
cargo run --release
```

To build the binary without launching it:

```bash
cargo build --release
./target/release/agent-whip
```

Useful commands:

```bash
./target/release/agent-whip --version
./target/release/agent-whip --dry-run  # log prompts instead of typing them
./target/release/agent-whip --selftest # synthetic cursor auto-cracks, exits (~5s)
./target/release/agent-whip whip       # toggle an already-running instance

# Preview a skin as a PNG without launching the app:
./target/release/agent-whip render-skin notorious out.png # classic/notorious/chrome/gold/neon
```

To install a source-built CLI on your `PATH`:

```bash
cargo install --path .
```

## Package a macOS app

Build a menu-bar `.app` bundle for Finder, Spotlight, or Raycast:

```bash
scripts/pack-app.sh           # build dist/AgentWhip.app
scripts/pack-app.sh --install # also copy it to /Applications
```

### Keep Accessibility access across rebuilds

Ad-hoc signing ties the app's identity to its binary hash, which changes on
every rebuild. macOS therefore asks for Accessibility access again.

For a stable identity, `pack-app.sh` automatically uses either a **Developer ID
Application** certificate or a self-signed **agent-whip** code-signing
certificate when one is available. Set `SIGN_ID` to override the detected
identity. Otherwise, the script falls back to ad-hoc signing.

To create a free self-signed certificate, open **Keychain Access → Certificate
Assistant → Create a Certificate** and use:

- Name: `agent-whip`
- Identity Type: **Self Signed Root**
- Certificate Type: **Code Signing**

## Build a distributable DMG

Create `dist/AgentWhip-<version>.dmg`:

```bash
scripts/make-dmg.sh
```

For a signed and notarized DMG, you need an Apple Developer Program membership,
a **Developer ID Application** certificate in your keychain, and stored notary
credentials:

```bash
xcrun notarytool store-credentials agent-whip-notary \
  --apple-id "you@example.com" \
  --team-id "TEAMID" \
  --password "app-specific-password"
```

With those credentials, the script signs the app with the hardened runtime,
builds the DMG, notarizes it, and staples the ticket. Without a Developer ID,
the script still creates an ad-hoc-signed DMG; recipients must right-click the
app and select **Open** the first time.

See the script header for overrides such as `DEVID` and `NOTARY_PROFILE`.

## Troubleshooting

The menu-bar app has no terminal, so agent-whip writes diagnostics to a log:

```bash
tail -f /tmp/agent-whip.log
```

- **The whip does not appear or nothing is typed** — Accessibility access is
  usually missing. Grant it as described above, then relaunch the app.
- **There is no sound after changing audio output** — agent-whip reopens the
  output device on every crack, so it should follow the current system default.
  Check the log for errors such as `no audio output` or a custom-sound decode
  failure.

## Compared with OpenWhip

| | OpenWhip | agent-whip |
|---|---|---|
| Runtime | Electron + Node | Single native binary |
| UI and physics | HTML `<canvas>` + JavaScript | `tiny-skia` + `wgpu`, all Rust |
| Keystrokes | `keybd_event` FFI / `osascript` / `xdotool` | `enigo` |
| Cursor input | Overlay captures the mouse | Global polling with `device_query` |
| Sound | Electron `Audio` | `rodio`, with embedded or custom clips |
| Guanzhang roar | Web Audio decode + loudest window | `rodio` decode + loudest window, embedded |
| Skins | Canvas gradients + `shadowBlur` | `tiny-skia` gradients + faked-bloom glow |
| Skin/roar glow | `ctx.shadowBlur` | Translucent over-wide passes |

## Credits

agent-whip is a port of [OpenWhip](https://github.com/GitFrog1111/OpenWhip) by
GitFrog1111. Its whip sounds and icons are carried over from that project.

Licensed under the [MIT License](LICENSE).
