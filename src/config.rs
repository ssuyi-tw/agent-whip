//! Runtime config for the crack prompt, read from
//! `$XDG_CONFIG_HOME/agent-whip/config.toml` (falls back to `~/.config/...`).
//!
//! The file is created with sensible defaults on first run and re-read whenever
//! it changes on disk, so edits apply on the next crack — no restart.

use crate::logging::log;
use rand::RngExt;
use serde::Deserialize;
use std::path::PathBuf;
use std::time::SystemTime;

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct Config {
    /// Phrases typed after a crack; one is picked at random each time.
    pub phrases: Vec<String>,
    /// Send Ctrl-C before typing.
    pub send_interrupt: bool,
    /// Press Enter after typing.
    pub send_enter: bool,
    /// Custom crack sound files (absolute or `~/…`); one is picked at random
    /// per crack. Empty = use the embedded clips.
    pub sounds: Vec<String>,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            // Default nudges the agent via Claude Code's `/btw`, which queues a
            // note WITHOUT interrupting the running turn — hence send_interrupt off.
            phrases: vec!["/btw hold on".to_string()],
            send_interrupt: false,
            send_enter: true,
            sounds: Vec::new(),
        }
    }
}

/// The commented default file written on first run.
const DEFAULT_TOML: &str = r#"# agent-whip config — edit and save; changes apply on the next crack.

# Typed after the whip cracks. One line is picked at random each crack.
# The default routes through Claude Code's /btw, which queues a note to the
# agent WITHOUT interrupting the running turn — that's why send_interrupt is off.
phrases = [
  "/btw hold on",
]

send_interrupt = false  # send Ctrl-C before typing (off: don't interrupt the turn)
send_enter     = true   # press Enter after typing

# Custom crack sounds — absolute paths or ~/…, one picked at random each crack.
# Leave empty to use the five built-in clips. Any format rodio decodes
# (wav, mp3, flac, ogg, …). Missing/unreadable files fall back to the built-ins.
sounds = []
"#;

fn config_dir() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .filter(|p| !p.as_os_str().is_empty())
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))?;
    Some(base.join("agent-whip"))
}

fn config_path() -> Option<PathBuf> {
    config_dir().map(|d| d.join("config.toml"))
}

/// Path to the pidfile the running instance writes, so an external command
/// (`agent-whip whip`, a Raycast script) can signal it to summon the whip.
pub fn pid_path() -> Option<PathBuf> {
    config_dir().map(|d| d.join("agent-whip.pid"))
}

/// Path to the one-line file holding the selected whip skin id. Kept separate
/// from `config.toml` so tray edits never clobber the user's commented config.
pub fn skin_path() -> Option<PathBuf> {
    config_dir().map(|d| d.join("skin"))
}

/// Expand a leading `~/` to `$HOME`; leave other paths untouched.
fn expand_tilde(s: &str) -> PathBuf {
    if let Some(rest) = s.strip_prefix("~/") {
        if let Some(home) = std::env::var_os("HOME") {
            return PathBuf::from(home).join(rest);
        }
    }
    PathBuf::from(s)
}

fn parse(text: &str) -> Config {
    match toml::from_str::<Config>(text) {
        Ok(c) if !c.phrases.is_empty() => c,
        Ok(_) => {
            log!("agent-whip: config has no phrases; using defaults");
            Config::default()
        }
        Err(e) => {
            log!("agent-whip: config parse error ({e}); using defaults");
            Config::default()
        }
    }
}

/// Tracks the config file and reloads it when its mtime changes.
pub struct ConfigFile {
    path: Option<PathBuf>,
    mtime: Option<SystemTime>,
    current: Config,
}

impl ConfigFile {
    /// Load config, creating the file with defaults if it's missing.
    pub fn init() -> Self {
        let path = config_path();
        if let Some(p) = &path {
            if p.exists() {
                log!("agent-whip: config at {}", p.display());
            } else {
                if let Some(dir) = p.parent() {
                    let _ = std::fs::create_dir_all(dir);
                }
                match std::fs::write(p, DEFAULT_TOML) {
                    Ok(()) => log!("agent-whip: wrote default config to {}", p.display()),
                    Err(e) => log!("agent-whip: could not write config ({e}); using defaults"),
                }
            }
        }
        let mut cf = ConfigFile {
            path,
            mtime: None,
            current: Config::default(),
        };
        cf.reload_if_changed();
        cf
    }

    /// Re-read the file if it changed on disk since the last read.
    pub fn reload_if_changed(&mut self) {
        let Some(path) = &self.path else { return };
        let m = std::fs::metadata(path).and_then(|md| md.modified()).ok();
        if self.mtime.is_some() && m == self.mtime {
            return;
        }
        self.mtime = m;
        self.current = match std::fs::read_to_string(path) {
            Ok(text) => parse(&text),
            Err(_) => Config::default(),
        };
    }

    pub fn send_interrupt(&self) -> bool {
        self.current.send_interrupt
    }

    pub fn send_enter(&self) -> bool {
        self.current.send_enter
    }

    /// A random phrase from the current config.
    pub fn pick_phrase(&self) -> String {
        let p = &self.current.phrases;
        if p.is_empty() {
            return String::new();
        }
        p[rand::rng().random_range(0..p.len())].clone()
    }

    /// A random configured sound file that exists on disk, or `None` to fall
    /// back to the embedded clips.
    pub fn pick_sound(&self) -> Option<PathBuf> {
        let existing: Vec<PathBuf> = self
            .current
            .sounds
            .iter()
            .map(|s| expand_tilde(s))
            .filter(|p| p.is_file())
            .collect();
        if existing.is_empty() {
            return None;
        }
        Some(existing[rand::rng().random_range(0..existing.len())].clone())
    }
}
