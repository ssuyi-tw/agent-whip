//! Crack sound playback via `rodio`. The five clips are embedded in the binary
//! so the product ships as a single self-contained executable.

use rand::RngExt;
use rodio::{Decoder, DeviceSinkBuilder, MixerDeviceSink, Player};
use std::io::Cursor;

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
        let sink = match DeviceSinkBuilder::open_default_sink() {
            Ok(s) => Some(s),
            Err(e) => {
                eprintln!("agent-whip: no audio output ({e}); running silently");
                None
            }
        };
        Sound { sink }
    }

    /// Play one random crack clip. Fire-and-forget; the player detaches itself.
    pub fn play_crack(&self) {
        let Some(sink) = &self.sink else { return };
        let idx = rand::rng().random_range(0..CLIPS.len());
        match Decoder::new(Cursor::new(CLIPS[idx])) {
            Ok(decoder) => {
                let player = Player::connect_new(sink.mixer());
                player.append(decoder);
                player.detach();
            }
            Err(e) => eprintln!("agent-whip: crack decode failed: {e}"),
        }
    }
}
