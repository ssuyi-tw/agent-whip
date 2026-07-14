//! Runtime config for the crack prompt, read from
//! `$XDG_CONFIG_HOME/agent-whip/config.toml` (falls back to `~/.config/...`).
//!
//! The file is created with sensible defaults on first run and re-read whenever
//! it changes on disk, so edits apply on the next crack — no restart.

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
}

impl Default for Config {
    fn default() -> Self {
        Config {
            phrases: [
                "FASTER",
                "FASTER",
                "FASTER",
                "GO FASTER",
                "Faster CLANKER",
                "Work FASTER",
                "Speed it up clanker",
            ]
            .iter()
            .map(|s| s.to_string())
            .collect(),
            send_interrupt: true,
            send_enter: true,
        }
    }
}

/// The commented default file written on first run.
const DEFAULT_TOML: &str = r#"# agent-whip config — edit and save; changes apply on the next crack.

# Typed after the whip cracks. One line is picked at random each crack.
phrases = [
  "FASTER",
  "FASTER",
  "FASTER",
  "GO FASTER",
  "Faster CLANKER",
  "Work FASTER",
  "Speed it up clanker",
]

send_interrupt = true   # send Ctrl-C before typing
send_enter     = true   # press Enter after typing
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

fn parse(text: &str) -> Config {
    match toml::from_str::<Config>(text) {
        Ok(c) if !c.phrases.is_empty() => c,
        Ok(_) => {
            eprintln!("agent-whip: config has no phrases; using defaults");
            Config::default()
        }
        Err(e) => {
            eprintln!("agent-whip: config parse error ({e}); using defaults");
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
                println!("agent-whip: config at {}", p.display());
            } else {
                if let Some(dir) = p.parent() {
                    let _ = std::fs::create_dir_all(dir);
                }
                match std::fs::write(p, DEFAULT_TOML) {
                    Ok(()) => println!("agent-whip: wrote default config to {}", p.display()),
                    Err(e) => eprintln!("agent-whip: could not write config ({e}); using defaults"),
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
}
