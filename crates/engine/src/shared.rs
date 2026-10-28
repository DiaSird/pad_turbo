//! State shared between the GUI thread and the worker thread, and the
//! start/stop requests the GUI makes on it.

use crate::recording::Macro;
use crate::settings::{Feature, Settings, TargetMode, TurboMode};
use gilrs::PowerInfo;
use i18n::Message;
use input::binding::Binding;
use input::pad::{PadState, SELECTABLE};
use std::collections::VecDeque;
use std::sync::{Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

/// Delay between pressing Start in the GUI and the function actually running.
///
/// Used when [`Settings::countdown`] is enabled, to give the user time to
/// switch to the game window. The toggle binding always starts immediately, because it
/// is pressed while the game is already in front.
pub const COUNTDOWN: Duration = Duration::from_secs(3);

/// Maximum number of log lines kept in [`Shared::log`].
const LOG_LINES: usize = 8;

/// Locks `shared`.
///
/// A panic on another thread while it held the lock does not make the state
/// unusable (every field is valid on its own), so a poisoned lock is
/// recovered instead of propagating the panic.
pub fn lock(shared: &Mutex<Shared>) -> MutexGuard<'_, Shared> {
    shared.lock().unwrap_or_else(PoisonError::into_inner)
}

/// A connected physical controller, as listed in the GUI.
pub struct SlotInfo {
    /// gilrs gamepad index; stable while the controller stays connected.
    pub index: usize,
    /// Device name reported by the driver.
    pub name: String,
    /// Battery / cable status.
    pub power: PowerInfo,
}

/// State shared between the GUI thread and the worker thread.
///
/// Fields in the first group are written by the GUI and read by the worker;
/// fields in the second group are written by the worker for display.
pub struct Shared {
    /// User settings (GUI-owned, worker reads every tick).
    pub settings: Settings,
    /// The function currently running, if any.
    pub active: Option<Feature>,
    /// A function scheduled to start at the given instant (GUI countdown).
    pub pending: Option<(Feature, Instant)>,
    /// While `true`, buttons pressed on the controller are added to
    /// [`Settings::turbo_mask`].
    pub capture: bool,
    /// Set by the GUI on exit; the worker cleans up and returns.
    pub quit: bool,
    /// The recorded macro, if any (worker replaces it after recording).
    pub macro_data: Option<Macro>,
    /// Controller chosen in the GUI, as a gilrs index ([`SlotInfo::index`]).
    ///
    /// The worker pre-selects the first controller it detects; the user can
    /// change it at any time while nothing is running.
    pub selected: Option<usize>,

    /// Connected physical controllers (the virtual one is excluded).
    pub slots: Vec<SlotInfo>,
    /// Live state of the selected controller.
    pub phys: PadState,
    /// Start/stop toggle binding. The GUI may replace it at any time; the
    /// worker picks up the new value on its next tick.
    pub binding: Binding,
    /// While `true`, the worker records a new binding from the keyboard and
    /// the selected controller instead of reacting to the toggle. When done it
    /// stores the result in [`Shared::binding`] and clears this flag.
    pub binding_capture: bool,
    /// Human-readable status of the output device.
    pub output_status: Message,
    /// Whether the active output is the virtual controller.
    pub output_is_vigem: bool,
    /// Processes currently driven by the keyboard output and their window
    /// count, or `None` when keys go to the focused window.
    pub target_status: Option<(Vec<u32>, usize)>,
    /// Frames captured so far by the running recording.
    pub rec_frames: usize,
    /// Milliseconds since the active function started.
    pub run_ms: u64,
    /// Current playback position within the macro, in milliseconds.
    pub play_pos_ms: u64,
    /// Most recent log entries, oldest first.
    pub log: VecDeque<Message>,
}

impl Shared {
    /// Creates the initial shared state from loaded settings, macro and
    /// toggle binding.
    pub fn new(settings: Settings, macro_data: Option<Macro>, binding: Binding) -> Self {
        Self {
            settings,
            active: None,
            pending: None,
            capture: false,
            quit: false,
            macro_data,
            selected: None,
            slots: Vec::new(),
            phys: PadState::default(),
            binding,
            binding_capture: false,
            output_status: Message::new("output.status.init"),
            output_is_vigem: false,
            target_status: None,
            rec_frames: 0,
            run_ms: 0,
            play_pos_ms: 0,
            log: VecDeque::new(),
        }
    }

    /// Appends a log entry, discarding the oldest beyond `LOG_LINES`.
    pub fn log(&mut self, msg: Message) {
        self.log.push_back(msg);
        while self.log.len() > LOG_LINES {
            self.log.pop_front();
        }
    }

    /// Whether a function is running or counting down.
    pub const fn is_running(&self) -> bool {
        self.active.is_some() || self.pending.is_some()
    }

    /// Checks whether `f` can be started with the current settings.
    ///
    /// # Errors
    ///
    /// Returns the translation key of the reason it cannot start:
    /// no target executable chosen for [`TargetMode::Process`] (keyboard
    /// output only), no turbo buttons selected, no controller selected
    /// (needed to record or to turbo in [`TurboMode::Hold`]), or no macro to
    /// play.
    pub fn check(&self, f: Feature) -> Result<(), &'static str> {
        let s = &self.settings;
        let sends = f != Feature::Record && !self.output_is_vigem;
        if sends && s.target == TargetMode::Process && s.target_exe.is_empty() {
            return Err("check.no_target");
        }
        match f {
            Feature::Turbo if s.turbo_mask & SELECTABLE == 0 => Err("check.no_buttons"),
            Feature::Turbo if s.turbo_mode == TurboMode::Hold && self.selected.is_none() => {
                Err("check.no_controller")
            }
            Feature::Record if self.selected.is_none() => Err("check.no_controller"),
            Feature::Play if self.macro_data.is_none() => Err("check.no_macro"),
            _ => Ok(()),
        }
    }

    /// Start button handler: starts `f`, honoring [`Settings::countdown`].
    ///
    /// Does nothing if [`Shared::check`] fails; the GUI disables the button in
    /// that case anyway.
    pub fn request_start(&mut self, f: Feature) {
        if self.check(f).is_err() {
            return;
        }
        if self.settings.countdown {
            self.pending = Some((f, Instant::now() + COUNTDOWN));
        } else {
            self.active = Some(f);
        }
    }

    /// Stop button handler: stops the running function and cancels any
    /// countdown.
    pub const fn stop(&mut self) {
        self.active = None;
        self.pending = None;
    }

    /// Toggle-binding handler: stops whatever is running or counting down,
    /// otherwise starts the selected function immediately.
    ///
    /// If the selected function cannot start (see [`Shared::check`]), the
    /// reason is logged instead.
    pub(crate) fn toggle(&mut self) {
        if self.is_running() {
            self.stop();
            return;
        }
        let f = self.settings.feature;
        match self.check(f) {
            Ok(()) => self.active = Some(f),
            Err(key) => self.log(Message::new(key)),
        }
    }
}
