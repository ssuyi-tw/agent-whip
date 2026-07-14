//! Minimal append-only file logging so diagnostics survive when agent-whip runs
//! as a menu-bar `.app` (no terminal attached). Every line is also mirrored to
//! stderr, so running from a terminal is unchanged.
//!
//! Tail it while diagnosing (e.g. sound not playing after a device switch):
//!
//! ```sh
//! tail -f /tmp/agent-whip.log
//! ```

use std::fmt::Arguments;
use std::io::Write;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

/// Where the log is written. `/tmp/agent-whip.log` on unix (easy to `tail`),
/// the OS temp dir elsewhere.
pub fn path() -> PathBuf {
    #[cfg(unix)]
    {
        PathBuf::from("/tmp/agent-whip.log")
    }
    #[cfg(not(unix))]
    {
        std::env::temp_dir().join("agent-whip.log")
    }
}

/// Append one epoch-stamped line to the log file and mirror it to stderr.
/// Use via the [`log!`] macro.
pub fn write(args: Arguments<'_>) {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let line = format!("[{secs}] {args}\n");
    eprint!("{line}");
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path())
    {
        let _ = f.write_all(line.as_bytes());
    }
}

/// `println!`-style logging that goes to both stderr and the log file.
macro_rules! log {
    ($($arg:tt)*) => { $crate::logging::write(format_args!($($arg)*)) };
}
pub(crate) use log;
