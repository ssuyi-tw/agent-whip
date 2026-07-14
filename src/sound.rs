//! Crack sound playback via `rodio`. Five clips are embedded in the binary so
//! the product ships self-contained; the config can point at custom files
//! instead (see `config.rs`).
//!
//! The output sink is reopened on each crack so switching the system audio
//! output (headphones, AirPods, a display's speakers) doesn't leave playback
//! bound to a device that's since gone silent.

use crate::logging::log;
use rand::RngExt;
use rodio::{Decoder, DeviceSinkBuilder, MixerDeviceSink, Player};
use std::io::Cursor;
use std::path::PathBuf;

const CLIPS: [&[u8]; 5] = [
    include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/assets/sounds/A.mp3")),
    include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/assets/sounds/B.mp3")),
    include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/assets/sounds/C.mp3")),
    include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/assets/sounds/D.mp3")),
    include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/assets/sounds/E.mp3")),
];

pub struct Sound {
    sink: Option<MixerDeviceSink>,
}

impl Sound {
    pub fn new() -> Self {
        Sound { sink: open_sink() }
    }

    /// Play one crack: a custom file if configured (and decodable), else a
    /// random embedded clip. Reopens the default output device first so it
    /// follows the current system default (fixing "no sound after switching
    /// audio devices").
    pub fn play_crack(&mut self, custom: Option<PathBuf>) {
        // Reopen so we target the *current* default device; keep the old sink
        // as a fallback if reopening fails transiently.
        if let Some(s) = open_sink() {
            self.sink = Some(s);
        }
        let Some(sink) = &self.sink else { return };

        if let Some(path) = &custom {
            match std::fs::read(path) {
                Ok(bytes) => {
                    if play_bytes(sink, bytes) {
                        return;
                    }
                    log!(
                        "agent-whip: couldn't decode {}; using a built-in crack",
                        path.display()
                    );
                }
                Err(e) => log!(
                    "agent-whip: can't read sound {} ({e}); using a built-in crack",
                    path.display()
                ),
            }
        }
        let idx = rand::rng().random_range(0..CLIPS.len());
        play_bytes(sink, CLIPS[idx].to_vec());
    }
}

/// Open the current default output sink, logging (once, here) if none is found.
fn open_sink() -> Option<MixerDeviceSink> {
    match DeviceSinkBuilder::open_default_sink() {
        Ok(mut s) => {
            // We reopen a sink per crack; rodio's drop warning would spam the log.
            s.log_on_drop(false);
            Some(s)
        }
        Err(e) => {
            log!("agent-whip: no audio output ({e}); running silently");
            None
        }
    }
}

/// Decode and play owned audio bytes. Fire-and-forget; the player detaches
/// itself. Returns whether decoding succeeded.
fn play_bytes(sink: &MixerDeviceSink, bytes: Vec<u8>) -> bool {
    match Decoder::new(Cursor::new(bytes)) {
        Ok(decoder) => {
            let player = Player::connect_new(sink.mixer());
            player.append(decoder);
            player.detach();
            true
        }
        Err(e) => {
            log!("agent-whip: crack decode failed: {e}");
            false
        }
    }
}
