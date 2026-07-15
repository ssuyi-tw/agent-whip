//! In-app updater: checks GitHub Releases for a newer version, then installs the
//! notarized DMG in place and relaunches. Network and disk work run off the main
//! thread (see `run_update_flow` in `main.rs`); this module stays winit-free.
//!
//! We shell out to `curl`, `shasum`, `hdiutil`, `ditto`, and `osascript` — all
//! present on macOS — rather than pulling in an HTTP/TLS stack. Installing the
//! Developer-ID-notarized DMG (same signing identity every release) keeps the
//! macOS Accessibility grant stable across updates.

use crate::logging::log;
use serde::Deserialize;
use std::path::{Path, PathBuf};
use std::process::Command;

const REPO: &str = "ssuyi-tw/agent-whip";

/// A release and its `.dmg` asset.
pub struct Release {
    /// Version without a leading `v` (e.g. `0.3.0`).
    pub version: String,
    pub dmg_url: String,
    /// Expected SHA-256 of the DMG, parsed from the release notes if present.
    pub sha256: Option<String>,
}

#[derive(Deserialize)]
struct ApiRelease {
    tag_name: String,
    #[serde(default)]
    body: String,
    #[serde(default)]
    assets: Vec<ApiAsset>,
}

#[derive(Deserialize)]
struct ApiAsset {
    name: String,
    browser_download_url: String,
}

/// Fetch the latest published release from GitHub.
pub fn latest() -> Result<Release, String> {
    let url = format!("https://api.github.com/repos/{REPO}/releases/latest");
    let out = Command::new("curl")
        .arg("-fsSL")
        .arg("-H")
        .arg("Accept: application/vnd.github+json")
        .arg("-H")
        .arg(format!("User-Agent: agent-whip/{}", env!("CARGO_PKG_VERSION")))
        .arg(&url)
        .output()
        .map_err(|e| format!("running curl: {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "GitHub API request failed ({})",
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    let rel: ApiRelease =
        serde_json::from_slice(&out.stdout).map_err(|e| format!("parsing release JSON: {e}"))?;
    let asset = rel
        .assets
        .iter()
        .find(|a| a.name.to_lowercase().ends_with(".dmg"))
        .ok_or("latest release has no .dmg asset")?;
    Ok(Release {
        version: rel.tag_name.trim_start_matches(['v', 'V']).to_string(),
        dmg_url: asset.browser_download_url.clone(),
        sha256: extract_sha256(&rel.body),
    })
}

/// Whether `latest` is a strictly higher version than `current` (dotted numeric).
pub fn is_newer(latest: &str, current: &str) -> bool {
    fn parts(v: &str) -> Vec<u64> {
        v.trim_start_matches(['v', 'V'])
            .split('.')
            .map(|p| p.trim().parse().unwrap_or(0))
            .collect()
    }
    let (l, c) = (parts(latest), parts(current));
    for i in 0..l.len().max(c.len()) {
        let a = l.get(i).copied().unwrap_or(0);
        let b = c.get(i).copied().unwrap_or(0);
        if a != b {
            return a > b;
        }
    }
    false
}

/// Pull the 64-hex SHA-256 that follows a `SHA-256` label in the release notes.
fn extract_sha256(body: &str) -> Option<String> {
    const LABEL: &str = "SHA-256";
    let idx = body.find(LABEL)?;
    let hex: String = body[idx + LABEL.len()..]
        .chars()
        .filter(|c| c.is_ascii_hexdigit())
        .take(64)
        .collect();
    (hex.len() == 64).then(|| hex.to_lowercase())
}

/// Download the release DMG to a temp file, verifying its checksum when the
/// release publishes one. Returns the downloaded path.
pub fn download(rel: &Release) -> Result<PathBuf, String> {
    let dmg = std::env::temp_dir().join("AgentWhip-update.dmg");
    let out = Command::new("curl")
        .arg("-fSL")
        .arg("-o")
        .arg(&dmg)
        .arg(&rel.dmg_url)
        .output()
        .map_err(|e| format!("running curl: {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "download failed ({})",
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    match &rel.sha256 {
        Some(want) => {
            let got = sha256_file(&dmg)?;
            if !got.eq_ignore_ascii_case(want) {
                let _ = std::fs::remove_file(&dmg);
                return Err(format!("checksum mismatch (expected {want}, got {got})"));
            }
            log!("agent-whip: update DMG checksum verified");
        }
        None => log!("agent-whip: release has no published checksum; skipping verification"),
    }
    Ok(dmg)
}

fn sha256_file(path: &Path) -> Result<String, String> {
    let out = Command::new("shasum")
        .arg("-a")
        .arg("256")
        .arg(path)
        .output()
        .map_err(|e| format!("running shasum: {e}"))?;
    if !out.status.success() {
        return Err("shasum failed".into());
    }
    String::from_utf8_lossy(&out.stdout)
        .split_whitespace()
        .next()
        .map(|h| h.to_lowercase())
        .ok_or_else(|| "empty shasum output".into())
}

/// Write a detached helper that waits for this process to quit, swaps the app
/// bundle out of the DMG, and relaunches — then spawn it. We can't replace our
/// own running bundle and relaunch from within, hence the helper.
pub fn spawn_installer(dmg: &Path) -> Result<(), String> {
    let dest = app_bundle_path();
    let script_path = std::env::temp_dir().join("agent-whip-installer.sh");
    let script = format!(
        r#"#!/bin/bash
# agent-whip self-updater — waits for the old app to quit, installs the new
# bundle from the downloaded DMG, then relaunches.
set -u
PID={pid}
DMG={dmg}
DEST={dest}
for _ in $(seq 1 150); do
  kill -0 "$PID" 2>/dev/null || break
  sleep 0.2
done
MP=$(hdiutil attach "$DMG" -nobrowse -readonly | grep -o '/Volumes/.*' | tail -1)
if [ -n "${{MP:-}}" ] && [ -d "$MP/AgentWhip.app" ]; then
  rm -rf "$DEST"
  ditto "$MP/AgentWhip.app" "$DEST"
fi
[ -n "${{MP:-}}" ] && hdiutil detach "$MP" -quiet 2>/dev/null || true
rm -f "$DMG"
open "$DEST"
rm -f "$0"
"#,
        pid = std::process::id(),
        dmg = shell_quote(dmg),
        dest = shell_quote(&dest),
    );
    std::fs::write(&script_path, script).map_err(|e| format!("writing installer: {e}"))?;
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&script_path, std::fs::Permissions::from_mode(0o755))
        .map_err(|e| format!("chmod installer: {e}"))?;
    // Spawned detached: on this GUI (no controlling terminal) it is reparented to
    // launchd and survives our exit, which it waits for before installing.
    Command::new("/bin/bash")
        .arg(&script_path)
        .spawn()
        .map_err(|e| format!("spawning installer: {e}"))?;
    Ok(())
}

/// The `.app` bundle this binary runs from, or `/Applications/AgentWhip.app`.
fn app_bundle_path() -> PathBuf {
    if let Ok(exe) = std::env::current_exe() {
        for anc in exe.ancestors() {
            if anc.extension().is_some_and(|e| e == "app") {
                return anc.to_path_buf();
            }
        }
    }
    PathBuf::from("/Applications/AgentWhip.app")
}

/// Single-quote a path for safe interpolation into the helper script.
fn shell_quote(p: &Path) -> String {
    format!("'{}'", p.to_string_lossy().replace('\'', r"'\''"))
}

/// Ask the user whether to install; `true` only if they pick Install.
pub fn confirm(message: &str) -> bool {
    let script = format!(
        "display dialog {msg} with title \"AgentWhip\" \
         buttons {{\"Later\", \"Install\"}} default button \"Install\" with icon note",
        msg = applescript_string(message)
    );
    match Command::new("osascript").arg("-e").arg(script).output() {
        Ok(o) => String::from_utf8_lossy(&o.stdout).contains("button returned:Install"),
        Err(e) => {
            log!("agent-whip: osascript failed: {e}");
            false
        }
    }
}

/// A non-blocking Notification Center banner.
pub fn notify(message: &str) {
    let script = format!(
        "display notification {msg} with title \"AgentWhip\"",
        msg = applescript_string(message)
    );
    let _ = Command::new("osascript").arg("-e").arg(script).status();
}

/// A blocking one-button alert (used for errors).
pub fn alert(message: &str) {
    let script = format!(
        "display dialog {msg} with title \"AgentWhip\" \
         buttons {{\"OK\"}} default button \"OK\" with icon caution",
        msg = applescript_string(message)
    );
    let _ = Command::new("osascript").arg("-e").arg(script).status();
}

/// Quote a string as an AppleScript string literal.
fn applescript_string(s: &str) -> String {
    format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn newer_versions() {
        assert!(is_newer("0.4.0", "0.3.0"));
        assert!(is_newer("v0.3.1", "0.3.0"));
        assert!(is_newer("1.0.0", "0.9.9"));
        assert!(!is_newer("0.3.0", "0.3.0"));
        assert!(!is_newer("0.2.0", "0.3.0"));
        assert!(!is_newer("v0.3.0", "0.3.0"));
    }

    #[test]
    fn sha_from_release_notes() {
        let hex = "14c532f17022f52be1e10cfc6f58af0843083f6321a436a79507149ae3181b3e";
        let body = format!("## macOS download\n\nSHA-256: `{hex}`\n");
        assert_eq!(extract_sha256(&body).as_deref(), Some(hex));
        assert_eq!(extract_sha256("no checksum here"), None);
    }
}
