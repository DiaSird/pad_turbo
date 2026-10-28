//! The macro format, recording a macro and playing it back.

use crate::shared::Shared;
use crate::{ms, secs};
use i18n::Message;
use input::pad::PadState;
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::time::Instant;

/// Recordings shorter than this (in milliseconds) are discarded as accidental.
const MIN_RECORDING_MS: u64 = 100;

/// One recorded controller state change.
#[derive(Clone, Serialize, Deserialize)]
pub struct MacroFrame {
    /// Milliseconds since the first frame of the recording.
    pub t: u64,
    /// Controller state from time `t` until the next frame.
    pub s: PadState,
}

/// A recorded macro, persisted to `macro.json` next to the executable.
///
/// Only state *changes* are stored; during playback each frame stays in effect
/// until the next one, and the whole sequence wraps after `duration_ms`.
#[derive(Clone, Serialize, Deserialize)]
pub struct Macro {
    /// Total length; playback loops with this period.
    pub duration_ms: u64,
    /// Frames sorted by ascending `t`. The first frame has `t == 0`.
    pub frames: Vec<MacroFrame>,
}

/// Collects the frames of a running recording.
///
/// The clock starts at the first frame, so idle time before the first input
/// is not recorded.
#[derive(Default)]
pub(crate) struct Recorder {
    /// Frames captured so far.
    frames: Vec<MacroFrame>,
    /// Time of the first frame.
    t0: Option<Instant>,
    /// State of the last frame, to store changes only.
    last: Option<PadState>,
}

impl Recorder {
    /// Discards everything and starts a new recording.
    pub(crate) fn start(&mut self) {
        *self = Self::default();
    }

    /// Stores `p` as a new frame if it differs from the previous one.
    pub(crate) fn sample(&mut self, p: PadState, now: Instant) {
        if self.last == Some(p) {
            return;
        }
        let t0 = *self.t0.get_or_insert(now);
        self.frames.push(MacroFrame { t: ms(now - t0), s: p });
        self.last = Some(p);
    }

    /// Number of frames captured so far.
    pub(crate) const fn frame_count(&self) -> usize {
        self.frames.len()
    }

    /// Turns the frames captured so far into [`Shared::macro_data`] and saves
    /// it to `path`.
    ///
    /// Recordings that are empty or shorter than [`MIN_RECORDING_MS`] are
    /// discarded. A failure to write `path` is logged but the macro is still
    /// kept in memory, so it can be played in this session.
    pub(crate) fn finish(&mut self, sh: &mut Shared, now: Instant, path: &Path) {
        let dur = self.t0.map_or(0, |t| ms(now - t));
        if self.frames.is_empty() || dur < MIN_RECORDING_MS {
            sh.log(Message::new("log.record_discarded"));
            return;
        }
        let m = Macro {
            duration_ms: dur,
            frames: std::mem::take(&mut self.frames),
        };
        let saved = serde_json::to_string(&m)
            .map_err(|e| e.to_string())
            .and_then(|json| std::fs::write(path, json).map_err(|e| e.to_string()));
        let msg = match saved {
            Ok(()) => Message::new("log.record_saved"),
            Err(e) => Message::new("log.record_save_failed").text("error", e),
        };
        sh.log(msg.text("frames", m.frames.len()).text("seconds", secs(dur)));
        sh.macro_data = Some(m);
    }
}

/// Playback position within a looping [`Macro`].
#[derive(Default)]
pub(crate) struct Player {
    /// Index of the frame currently in effect.
    idx: usize,
    /// Position of the previous call, to detect wrap-around.
    last_t: u64,
}

impl Player {
    /// The state in effect `elapsed_ms` after playback started, together with
    /// the position within the macro. `None` for a macro without frames.
    pub(crate) fn frame(&mut self, m: &Macro, elapsed_ms: u64) -> Option<(PadState, u64)> {
        let t = elapsed_ms % m.duration_ms.max(1);
        if t < self.last_t {
            self.idx = 0; // wrapped around
        }
        self.last_t = t;
        while m.frames.get(self.idx + 1).is_some_and(|f| f.t <= t) {
            self.idx += 1;
        }
        m.frames.get(self.idx).map(|f| (f.s, t))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(t: u64, buttons: u16) -> MacroFrame {
        MacroFrame { t, s: PadState { buttons, ..PadState::default() } }
    }

    #[test]
    fn player_follows_frames_and_wraps() {
        let m = Macro { duration_ms: 100, frames: vec![frame(0, 1), frame(50, 2)] };
        let mut p = Player::default();
        assert_eq!(p.frame(&m, 10).map(|(s, _)| s.buttons), Some(1));
        assert_eq!(p.frame(&m, 60).map(|(s, _)| s.buttons), Some(2));
        assert_eq!(p.frame(&m, 110), Some((frame(0, 1).s, 10)));
    }

    #[test]
    fn player_handles_empty_macro() {
        let m = Macro { duration_ms: 0, frames: Vec::new() };
        assert!(Player::default().frame(&m, 5).is_none());
    }

    #[test]
    fn recorder_stores_changes_only() {
        let mut r = Recorder::default();
        let now = Instant::now();
        r.sample(PadState::default(), now);
        r.sample(PadState::default(), now);
        r.sample(frame(0, 1).s, now);
        assert_eq!(r.frame_count(), 2);
    }
}
