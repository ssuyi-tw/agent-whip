//! The crack macro: interrupt the focused app with Ctrl-C, then type an
//! encouraging phrase and press Enter — ported from the platform-specific
//! `sendMacro*` functions, collapsed onto `enigo`'s cross-platform backend.
//!
//! IMPORTANT (macOS): `enigo` calls into Text Services Manager APIs that assert
//! they run on the main thread. So every function here MUST be called from the
//! main thread (the winit event-loop thread) — never a spawned thread. The
//! 300 ms gap between interrupt and typing is handled by the event loop
//! deferring `type_phrase`, not by sleeping here.

use enigo::{Direction, Enigo, Key, Keyboard, Settings};
use rand::RngExt;

/// The five encouraging phrases (weighted toward "FASTER", as in the original).
pub const PHRASES: [&str; 7] = [
    "FASTER",
    "FASTER",
    "FASTER",
    "GO FASTER",
    "Faster CLANKER",
    "Work FASTER",
    "Speed it up clanker",
];

pub fn pick_phrase() -> &'static str {
    PHRASES[rand::rng().random_range(0..PHRASES.len())]
}

/// Create the keystroke backend. `None` if unavailable (e.g. Accessibility not
/// granted). Must be called on the main thread.
pub fn new_enigo() -> Option<Enigo> {
    match Enigo::new(&Settings::default()) {
        Ok(e) => Some(e),
        Err(e) => {
            eprintln!("agent-whip: keyboard control unavailable ({e}); grant Accessibility.");
            None
        }
    }
}

/// Ctrl-C — interrupt whatever the whipped app is doing. Main thread only.
pub fn interrupt(enigo: &mut Enigo) {
    if let Err(e) = (|| -> Result<(), enigo::InputError> {
        enigo.key(Key::Control, Direction::Press)?;
        enigo.key(Key::Unicode('c'), Direction::Click)?;
        enigo.key(Key::Control, Direction::Release)
    })() {
        eprintln!("agent-whip: interrupt failed: {e}");
    }
}

/// Type the phrase and press Enter. Main thread only.
pub fn type_phrase(enigo: &mut Enigo, text: &str) {
    if let Err(e) = (|| -> Result<(), enigo::InputError> {
        enigo.text(text)?;
        enigo.key(Key::Return, Direction::Click)
    })() {
        eprintln!("agent-whip: type failed: {e}");
    }
}
