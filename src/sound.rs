//! Crack sound playback via `rodio`. Five clips are embedded in the binary so
//! the product ships self-contained; the config can point at custom files
//! instead (see `config.rs`).
//!
//! Layered on top of every crack (when enabled) is the Guanzhang "RRRRR" roar —
//! `guanzhang.mp3`, also embedded. Ported from OpenWhip's `playGuanzhang`: the
//! clip is decoded once at startup and only its loudest ~1.1 s window is played
//! per strike, so the growl lands immediately instead of after a quiet intro.
//!
//! The output sink is reopened on each crack so switching the system audio
//! output (headphones, AirPods, a display's speakers) doesn't leave playback
//! bound to a device that's since gone silent. Reopening also drops the previous
//! sink, which stops any roar still playing — the natural "retrigger" so rapid
//! whips stay punchy instead of piling up.

use crate::logging::log;
use rand::RngExt;
use rodio::{Decoder, DeviceSinkBuilder, MixerDeviceSink, Player, Source};
use std::io::Cursor;
use std::path::PathBuf;
use std::time::Duration;

const CLIPS: [&[u8]; 5] = [
    include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/assets/sounds/A.mp3")),
    include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/assets/sounds/B.mp3")),
    include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/assets/sounds/C.mp3")),
    include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/assets/sounds/D.mp3")),
    include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/assets/sounds/E.mp3")),
];

/// The Guanzhang roar, layered over each crack.
const GUANZHANG: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/assets/sounds/guanzhang.mp3"
));
/// How much of the roar clip each strike plays (its loudest slice).
const ROAR_WINDOW: Duration = Duration::from_millis(1100);

pub struct Sound {
    sink: Option<MixerDeviceSink>,
    /// The `(offset, length)` of the loudest window in the roar clip, computed
    /// once at startup. `None` if the embedded clip couldn't be analysed.
    roar_window: Option<(Duration, Duration)>,
}

impl Sound {
    pub fn new() -> Self {
        Sound {
            sink: open_sink(),
            roar_window: loudest_window(GUANZHANG, ROAR_WINDOW),
        }
    }

    /// Play one crack: a custom file if configured (and decodable), else a
    /// random embedded clip. When `roar` is set, the Guanzhang growl is layered on
    /// top. Reopens the default output device first so it follows the current
    /// system default (fixing "no sound after switching audio devices").
    pub fn play_crack(&mut self, custom: Option<PathBuf>, roar: bool) {
        // Reopen so we target the *current* default device; keep the old sink
        // as a fallback if reopening fails transiently.
        if let Some(s) = open_sink() {
            self.sink = Some(s);
        }
        let Some(sink) = &self.sink else { return };

        let mut played_custom = false;
        if let Some(path) = &custom {
            match std::fs::read(path) {
                Ok(bytes) => {
                    if play_bytes(sink, bytes) {
                        played_custom = true;
                    } else {
                        log!(
                            "agent-whip: couldn't decode {}; using a built-in crack",
                            path.display()
                        );
                    }
                }
                Err(e) => log!(
                    "agent-whip: can't read sound {} ({e}); using a built-in crack",
                    path.display()
                ),
            }
        }
        if !played_custom {
            let idx = rand::rng().random_range(0..CLIPS.len());
            play_bytes(sink, CLIPS[idx].to_vec());
        }

        if roar {
            self.play_roar(sink);
        }
    }

    /// Layer the loudest window of the Guanzhang roar over the crack. Fire-and-forget
    /// on the same sink as the crack; the next crack reopens the sink and stops
    /// it (retrigger).
    fn play_roar(&self, sink: &MixerDeviceSink) {
        let Some((offset, length)) = self.roar_window else {
            return;
        };
        match Decoder::new(Cursor::new(GUANZHANG.to_vec())) {
            Ok(decoder) => {
                let roar = decoder
                    .skip_duration(offset)
                    .take_duration(length)
                    .fade_in(Duration::from_millis(30))
                    .amplify(0.9);
                let player = Player::connect_new(sink.mixer());
                player.append(roar);
                player.detach();
            }
            Err(e) => log!("agent-whip: roar decode failed: {e}"),
        }
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

/// Slide a fixed-length window across the clip and return the `(offset, length)`
/// of the highest-energy (loudest) one — that's the RRRR we want, wherever it
/// sits in the file. Mirrors OpenWhip's `loudestWindow`, reading channel 0 only.
fn loudest_window(bytes: &[u8], max: Duration) -> Option<(Duration, Duration)> {
    let dec = Decoder::new(Cursor::new(bytes.to_vec())).ok()?;
    let sr = dec.sample_rate().get() as usize;
    let ch = (dec.channels().get() as usize).max(1);
    if sr == 0 {
        return None;
    }

    // Mono (channel-0) samples at the clip's sample rate.
    let mono: Vec<f32> = dec.step_by(ch).collect();
    let n = mono.len();
    if n == 0 {
        return None;
    }

    let win_len = ((max.as_secs_f32() * sr as f32) as usize).clamp(1, n);
    let step = ((sr as f32 * 0.02) as usize).max(1); // 20 ms hops

    let mut sum: f64 = mono[..win_len]
        .iter()
        .map(|&s| (s as f64) * (s as f64))
        .sum();
    let (mut best_start, mut best_sum) = (0usize, sum);
    let mut start = step;
    while start + win_len <= n {
        // Slide the window by `step`: drop the samples leaving on the left, add
        // the ones entering on the right.
        for &s in &mono[(start - step)..start] {
            sum -= (s as f64) * (s as f64);
        }
        for &s in &mono[(start - step + win_len)..(start + win_len)] {
            sum += (s as f64) * (s as f64);
        }
        if sum > best_sum {
            best_sum = sum;
            best_start = start;
        }
        start += step;
    }

    let offset = Duration::from_secs_f32(best_start as f32 / sr as f32);
    let length = Duration::from_secs_f32(win_len as f32 / sr as f32);
    Some((offset, length))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_roar_yields_a_sane_window() {
        // The embedded Guanzhang clip must decode and produce a loudest window of the
        // requested length (the clip is longer than the window), sitting inside
        // the clip. `length` derives from an integer sample count, so allow a
        // hair of float rounding on the requested duration.
        let (offset, length) =
            loudest_window(GUANZHANG, ROAR_WINDOW).expect("roar clip should decode");
        let slack = Duration::from_millis(5);
        assert!(length > ROAR_WINDOW - slack, "window ~ requested length");
        assert!(length < ROAR_WINDOW + slack, "window ~ requested length");
        // The clip is short; a sane offset stays well under a minute.
        assert!(offset < Duration::from_secs(60), "offset within the clip");
    }
}
