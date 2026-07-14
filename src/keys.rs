//! The crack macro: interrupt the focused app with Ctrl-C, then type an
//! encouraging phrase and press Enter — ported from the platform-specific
//! `sendMacro*` functions, collapsed onto `enigo`'s cross-platform backend.
//!
//! IMPORTANT (macOS): `enigo` calls into Text Services Manager APIs that assert
//! they run on the main thread. So every function here MUST be called from the
//! main thread (the winit event-loop thread) — never a spawned thread. The
//! 300 ms gap between interrupt and typing is handled by the event loop
//! deferring `type_phrase`, not by sleeping here.

use crate::logging::log;
use enigo::{Direction, Enigo, Key, Keyboard, Settings};

/// Create the keystroke backend. `None` if unavailable (e.g. Accessibility not
/// granted). Must be called on the main thread.
pub fn new_enigo() -> Option<Enigo> {
    match Enigo::new(&Settings::default()) {
        Ok(e) => Some(e),
        Err(e) => {
            log!("agent-whip: keyboard control unavailable ({e}); grant Accessibility.");
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
        log!("agent-whip: interrupt failed: {e}");
    }
}

/// Type the phrase, optionally pressing Enter after. Main thread only.
pub fn type_phrase(enigo: &mut Enigo, text: &str, send_enter: bool) {
    if let Err(e) = (|| -> Result<(), enigo::InputError> {
        enigo.text(text)?;
        if send_enter {
            enigo.key(Key::Return, Direction::Click)?;
        }
        Ok(())
    })() {
        log!("agent-whip: type failed: {e}");
    }
}
