//! Feature transitions (start/stop logging, saving recordings) and composing
//! the output state: base → macro frame → turbo overlay.

use super::inputs::Sample;
use crate::ms;
use crate::recording::{Player, Recorder};
use crate::settings::{Feature, Settings, TurboMode, press_ms};
use crate::shared::Shared;
use i18n::Message;
use input::pad::{self, PadState};
use std::path::PathBuf;
use std::time::Instant;

/// Turbo phase.
struct Turbo {
    /// Start of the current pulse train.
    t0: Instant,
    /// Buttons pulsed in the previous tick.
    last_target: u32,
}

impl Turbo {
    /// Restarts the phase.
    const fn start(&mut self, now: Instant) {
        self.t0 = now;
        self.last_target = 0;
    }

    /// Forces the turbo buttons of `s` on for the first `press_ms` of every
    /// period and off for the rest.
    fn apply(&mut self, s: &Settings, out: PadState, phys_mask: u32, now: Instant) -> PadState {
        let target = match s.turbo_mode {
            TurboMode::Always => s.turbo_mask,
            TurboMode::Hold => s.turbo_mask & phys_mask,
        };
        // Restart the phase when a held button is first pressed, so the first
        // pulse fires immediately.
        if target != 0 && self.last_target == 0 {
            self.t0 = now;
        }
        self.last_target = target;
        if target == 0 {
            return out;
        }
        let period = u64::from(s.interval_ms.max(1));
        let on = ms(now - self.t0) % period < u64::from(press_ms(s.interval_ms));
        out.with_mask(target, on)
    }
}

/// State of the three features across ticks.
pub(super) struct Features {
    /// Where finished recordings are saved.
    macro_path: PathBuf,
    /// The feature that was active in the previous tick.
    prev: Option<Feature>,
    /// When the active feature started.
    t0: Instant,
    /// Turbo phase.
    turbo: Turbo,
    /// Running recording.
    recorder: Recorder,
    /// Playback position.
    player: Player,
}

impl Features {
    /// Nothing running yet.
    pub(super) fn new(macro_path: PathBuf) -> Self {
        let now = Instant::now();
        Self {
            macro_path,
            prev: None,
            t0: now,
            turbo: Turbo { t0: now, last_target: 0 },
            recorder: Recorder::default(),
            player: Player::default(),
        }
    }

    /// Saves the recording if one is running.
    pub(super) fn stop(&mut self, sh: &mut Shared, now: Instant) {
        if self.prev == Some(Feature::Record) {
            self.recorder.finish(sh, now, &self.macro_path);
        }
    }

    /// Reacts to [`Shared::active`] having changed since the previous tick:
    /// ends a recording, resets the new feature and logs the change. Also
    /// publishes the running time.
    pub(super) fn transition(&mut self, sh: &mut Shared, now: Instant) {
        if sh.active != self.prev {
            self.stop(sh, now);
            if let Some(msg) = self.start(sh, now) {
                sh.log(msg);
            }
            self.t0 = now;
            self.prev = sh.active;
        }
        sh.run_ms = ms(now - self.t0);
    }

    /// Resets the state of the newly active feature and returns its log line.
    fn start(&mut self, sh: &mut Shared, now: Instant) -> Option<Message> {
        match sh.active {
            Some(Feature::Turbo) => {
                self.turbo.start(now);
                let s = &sh.settings;
                let msg = Message::new("log.turbo_started")
                    .text("buttons", pad::format_mask(s.turbo_mask))
                    .text("interval", s.interval_ms);
                Some(msg)
            }
            Some(Feature::Record) => {
                self.recorder.start();
                sh.rec_frames = 0;
                Some(Message::new("log.record_started"))
            }
            Some(Feature::Play) => {
                self.player = Player::default();
                Some(Message::new("log.play_started"))
            }
            None => self.prev.map(|p| Message::new("log.stopped").key("feature", p.key())),
        }
    }

    /// The state to send this tick: `base`, replaced by the macro frame while
    /// playing, with turbo applied on top while turbo runs. While recording,
    /// the controller state is stored.
    pub(super) fn compose(&mut self, sh: &mut Shared, sample: &Sample, base: PadState, now: Instant) -> PadState {
        match sh.active {
            Some(Feature::Turbo) => self.turbo.apply(&sh.settings, base, sample.mask(), now),
            Some(Feature::Record) => {
                if let Some(p) = sample.phys {
                    self.recorder.sample(p, now);
                }
                sh.rec_frames = self.recorder.frame_count();
                base
            }
            Some(Feature::Play) => {
                let frame = sh.macro_data.as_ref().and_then(|m| self.player.frame(m, ms(now - self.t0)));
                match frame {
                    Some((s, pos)) => {
                        sh.play_pos_ms = pos;
                        s
                    }
                    None => base,
                }
            }
            None => base,
        }
    }
}
