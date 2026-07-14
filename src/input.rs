//! Global cursor + click polling via `device_query`.
//!
//! The overlay window is click-through (`set_cursor_hittest(false)`), so winit
//! never delivers pointer events to it. Instead we sample the global cursor each
//! frame — this is how the whip tracks the mouse across every app on screen.

use device_query::{DeviceQuery, DeviceState};

pub struct Input {
    ds: DeviceState,
    prev_left: bool,
}

impl Input {
    /// Returns `None` when the OS won't grant input access (on macOS/Linux this
    /// means Accessibility permission is not enabled). Constructing this also
    /// triggers the macOS Accessibility prompt, so call it lazily on first use.
    pub fn try_new() -> Option<Self> {
        DeviceState::checked_new().map(|ds| Input {
            ds,
            prev_left: false,
        })
    }

    /// Current global cursor position in screen coordinates.
    pub fn cursor(&self) -> (i32, i32) {
        self.ds.get_mouse().coords
    }

    /// Poll cursor + detect a fresh left-button press (rising edge).
    pub fn poll(&mut self) -> ((i32, i32), bool) {
        let m = self.ds.get_mouse();
        let left = m.button_pressed.get(1).copied().unwrap_or(false);
        let rising = left && !self.prev_left;
        self.prev_left = left;
        (m.coords, rising)
    }

    /// Re-sync the button state so the click that spawned the whip (e.g. on the
    /// tray) is not immediately read as a "drop" click.
    pub fn sync_button(&mut self) {
        let m = self.ds.get_mouse();
        self.prev_left = m.button_pressed.get(1).copied().unwrap_or(false);
    }
}
