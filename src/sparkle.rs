//! Bridge to the embedded Sparkle.framework (macOS auto-updates).
//!
//! We `dlopen` the framework at runtime and drive `SPUStandardUpdaterController`
//! through the Objective-C runtime, so the Rust build has **no** link-time
//! dependency on Sparkle — the framework only has to be present in the app
//! bundle at `Contents/Frameworks/Sparkle.framework` (embedded by
//! `scripts/pack-app.sh`). Running from a plain `cargo` build (no bundle) simply
//! finds no framework and disables updates.
//!
//! The update feed and signing key come from Info.plist (`SUFeedURL`,
//! `SUPublicEDKey`); Sparkle owns the download, EdDSA verification, install,
//! relaunch, and its own UI. Every call here must run on the main (winit)
//! thread — Sparkle asserts this.

use crate::logging::log;
use objc2::msg_send;
use objc2::rc::{Allocated, Retained};
use objc2::runtime::{AnyClass, AnyObject};
use std::ffi::{CStr, CString, c_char, c_int, c_void};
use std::path::PathBuf;
use std::ptr;

/// Owns the Sparkle updater controller for the app's lifetime.
pub struct Updater {
    controller: Retained<AnyObject>,
}

impl Updater {
    /// Load Sparkle and start its updater. Returns `None` if the framework isn't
    /// embedded or its class can't be found (e.g. a bare `cargo run`). Must be
    /// called on the main thread.
    pub fn start() -> Option<Updater> {
        load_framework()?;
        let cls = AnyClass::get(c"SPUStandardUpdaterController").or_else(|| {
            log!("agent-whip: Sparkle class not found after load");
            None
        })?;
        // SAFETY: on the main thread; the selector and argument types match
        // `-[SPUStandardUpdaterController initWithStartingUpdater:updaterDelegate:userDriverDelegate:]`.
        let controller: Retained<AnyObject> = unsafe {
            let alloc: Allocated<AnyObject> = msg_send![cls, alloc];
            msg_send![
                alloc,
                initWithStartingUpdater: true,
                updaterDelegate: ptr::null::<AnyObject>(),
                userDriverDelegate: ptr::null::<AnyObject>(),
            ]
        };
        log!("agent-whip: Sparkle updater started");
        Some(Updater { controller })
    }

    /// User-initiated update check (tray "Check for Update"). Shows Sparkle's UI
    /// whatever the result. Main thread only.
    pub fn check_for_updates(&self) {
        // SAFETY: main thread; `-checkForUpdates:` takes a nullable sender.
        unsafe {
            let _: () = msg_send![&*self.controller, checkForUpdates: ptr::null::<AnyObject>()];
        }
    }

    /// Silent background check (run once at launch). Sparkle only surfaces UI if
    /// an update is actually available. Main thread only.
    pub fn check_in_background(&self) {
        // SAFETY: main thread; `-[SPUStandardUpdaterController updater]` returns
        // the SPUUpdater, whose `-checkForUpdatesInBackground` takes no args.
        unsafe {
            let updater: *mut AnyObject = msg_send![&*self.controller, updater];
            if !updater.is_null() {
                let _: () = msg_send![updater, checkForUpdatesInBackground];
            }
        }
    }
}

/// Load the embedded Sparkle binary so its Objective-C classes register. Returns
/// `Some(())` on success.
fn load_framework() -> Option<()> {
    let path = framework_binary_path().or_else(|| {
        log!("agent-whip: Sparkle.framework not found in the app bundle; updates disabled");
        None
    })?;
    let cpath = CString::new(path.into_os_string().into_encoded_bytes()).ok()?;
    // SAFETY: `cpath` is a valid NUL-terminated C string for the duration of the call.
    let handle = unsafe { dlopen(cpath.as_ptr(), RTLD_NOW | RTLD_GLOBAL) };
    if handle.is_null() {
        // SAFETY: dlerror returns a static/thread-local C string or null.
        let err = unsafe {
            let e = dlerror();
            if e.is_null() {
                String::new()
            } else {
                CStr::from_ptr(e).to_string_lossy().into_owned()
            }
        };
        log!("agent-whip: Sparkle.framework failed to load ({err})");
        return None;
    }
    Some(())
}

/// Absolute path to the Sparkle binary inside this app bundle, if present.
fn framework_binary_path() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    // Resolve symlinks (e.g. Homebrew's `bin/agent-whip` -> the real bundle) so
    // we look beside the actual executable, not beside the symlink.
    let exe = std::fs::canonicalize(&exe).unwrap_or(exe);
    // exe: <App>.app/Contents/MacOS/agent-whip -> Contents is two levels up.
    let contents = exe.parent()?.parent()?;
    let fw = contents.join("Frameworks/Sparkle.framework/Versions/B/Sparkle");
    fw.exists().then_some(fw)
}

unsafe extern "C" {
    fn dlopen(path: *const c_char, mode: c_int) -> *mut c_void;
    fn dlerror() -> *const c_char;
}
const RTLD_NOW: c_int = 0x2;
const RTLD_GLOBAL: c_int = 0x8;
