//! The worker thread: polls input, runs turbo / macro logic and drives the output.
//!
//! # Threading model
//!
//! The GUI thread and the worker share one [`Shared`] value behind a
//! [`Mutex`]. The GUI edits settings and requests start/stop; the worker reads
//! those requests once per tick and publishes status (controller list, live
//! input, progress, log) back into the same struct. Each lock is held only for
//! a few microseconds, so neither side blocks the other noticeably.
//!
//! # Tick
//!
//! [`run`] loops roughly once per millisecond (the system timer resolution is
//! raised to 1 ms with `timeBeginPeriod`). Each tick:
//!
//! 1. (re)creates the [`Output`] if the user switched the output device;
//! 2. reads the controller selected in the GUI;
//! 3. polls the start/stop [`Toggle`] (keyboard and/or controller buttons);
//! 4. applies state transitions (start/stop, countdown expiry, recording end);
//! 5. composes the output state: pass-through → macro frame → turbo overlay;
//! 6. sends the state to the output if it changed since the previous tick.
//!
//! # Composition
//!
//! With the ViGEm output the physical controller is passed through so that the
//! player keeps full control of the virtual pad. With the keyboard output
//! nothing is passed through, because the game already receives the physical
//! controller directly. A playing macro replaces the base state, and turbo
//! finally forces its buttons on or off according to the current phase.
//!
//! Controller buttons that belong to the toggle binding are removed from the
//! physical state while the whole binding is held, so pressing the toggle on
//! the controller is neither forwarded to the game nor recorded into a macro.

use crate::binding::{Binding, Capture, Toggle};
use crate::i18n::Message;
use crate::output::Output;
use crate::pad::{self, PadState, Pads, SELECTABLE};
use gilrs::{GamepadId, PowerInfo};
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use windows_sys::Win32::Media::{timeBeginPeriod, timeEndPeriod};

/// Delay between pressing Start in the GUI and the function actually running,
/// when [`Settings::countdown`] is enabled. Gives the user time to switch to
/// the game window. The toggle binding always starts immediately, because it
/// is pressed while the game is already in front.
pub const COUNTDOWN: Duration = Duration::from_secs(3);

/// Maximum number of log lines kept in [`Shared::log`].
const LOG_LINES: usize = 8;

/// Recordings shorter than this (in milliseconds) are discarded as accidental.
const MIN_RECORDING_MS: u64 = 100;

/// How often the list of connected controllers is refreshed.
const SLOT_REFRESH: Duration = Duration::from_secs(1);

/// How long to wait for the virtual controller to appear as an input device.
const VIRTUAL_DETECT_TIMEOUT: Duration = Duration::from_secs(2);

/// The three functions the user can choose from. Only one runs at a time.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum Feature {
    /// Rapid-fire the selected buttons.
    Turbo,
    /// Record the physical controller into a macro.
    Record,
    /// Loop the recorded macro.
    Play,
}

impl Feature {
    /// Every feature, in GUI order.
    pub const ALL: [Feature; 3] = [Feature::Turbo, Feature::Record, Feature::Play];

    /// Translation key of the feature's display name.
    pub fn key(self) -> &'static str {
        match self {
            Feature::Turbo => "feature.turbo",
            Feature::Record => "feature.record",
            Feature::Play => "feature.play",
        }
    }
}

/// When turbo is applied.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum TurboMode {
    /// The selected buttons are pulsed continuously while turbo runs.
    Always,
    /// Each selected button is pulsed only while it is physically held.
    Hold,
}

/// Which output device the user wants.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum OutputPref {
    /// Virtual Xbox 360 controller through ViGEmBus.
    ViGEm,
    /// Keyboard injection.
    Keyboard,
}

/// User settings, persisted to `settings.json` next to the executable.
///
/// `#[serde(default)]` lets older or partial files load: missing fields take
/// their [`Default`] value.
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// GUI language.
    pub language: crate::i18n::Language,
    /// Function selected in the GUI; the toggle starts and stops this one.
    pub feature: Feature,
    /// Logical mask of the buttons to turbo (see [`pad::BUTTONS`]).
    pub turbo_mask: u32,
    /// Turbo period in milliseconds (one press + one release).
    pub interval_ms: u32,
    /// Turbo behavior.
    pub turbo_mode: TurboMode,
    /// Requested output device.
    pub output: OutputPref,
    /// Wait [`COUNTDOWN`] after pressing Start in the GUI.
    pub countdown: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            language: crate::i18n::Language::English,
            feature: Feature::Turbo,
            turbo_mask: 0,
            interval_ms: 1000,
            turbo_mode: TurboMode::Always,
            output: OutputPref::ViGEm,
            countdown: true,
        }
    }
}

/// How long each turbo press is held, for a given period.
///
/// Half the period, clamped to 10–100 ms: long enough for games that poll at
/// 60 Hz to notice the press, short enough to leave a clear release gap even
/// at fast rates.
pub fn press_ms(interval_ms: u32) -> u32 {
    (interval_ms / 2).clamp(10, 100)
}

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
            rec_frames: 0,
            run_ms: 0,
            play_pos_ms: 0,
            log: VecDeque::new(),
        }
    }

    /// Appends a log entry, discarding the oldest beyond [`LOG_LINES`].
    pub fn log(&mut self, msg: Message) {
        self.log.push_back(msg);
        while self.log.len() > LOG_LINES {
            self.log.pop_front();
        }
    }

    /// Whether a function is running or counting down.
    pub fn is_running(&self) -> bool {
        self.active.is_some() || self.pending.is_some()
    }

    /// Checks whether `f` can be started with the current settings.
    ///
    /// # Errors
    ///
    /// Returns the translation key of the reason it cannot start:
    /// no turbo buttons selected, no controller selected (needed to record or
    /// to turbo in [`TurboMode::Hold`]), or no macro to play.
    pub fn check(&self, f: Feature) -> Result<(), &'static str> {
        let s = &self.settings;
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
    pub fn stop(&mut self) {
        self.active = None;
        self.pending = None;
    }

    /// Toggle-binding handler: stops whatever is running or counting down,
    /// otherwise starts the selected function immediately.
    ///
    /// If the selected function cannot start (see [`Shared::check`]), the
    /// reason is logged instead.
    fn toggle(&mut self) {
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

/// Whole milliseconds in `d`.
fn ms(d: Duration) -> u64 {
    d.as_millis() as u64
}

/// Seconds with one decimal, for log messages.
fn secs(ms: u64) -> String {
    format!("{:.1}", ms as f64 / 1000.0)
}

/// Turns the frames captured so far into [`Shared::macro_data`] and saves it.
///
/// Recordings that are empty or shorter than [`MIN_RECORDING_MS`] are
/// discarded. A failure to write `path` is logged but the macro is still kept
/// in memory, so it can be played in this session.
fn finish_recording(
    sh: &mut Shared,
    rec: &mut Vec<MacroFrame>,
    rec_t0: Option<Instant>,
    now: Instant,
    path: &Path,
) {
    let dur = rec_t0.map(|t| ms(now - t)).unwrap_or(0);
    if rec.is_empty() || dur < MIN_RECORDING_MS {
        sh.log(Message::new("log.record_discarded"));
        return;
    }
    let m = Macro { duration_ms: dur, frames: std::mem::take(rec) };
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

/// Worker-thread entry point. Returns when [`Shared::quit`] is set.
///
/// `macro_path` is where finished recordings are saved. The toggle binding is
/// taken from [`Shared::binding`] and re-read whenever the GUI changes it.
///
/// Before returning, the output is shut down (all keys/buttons released, the
/// virtual controller unplugged) and an unfinished recording is saved.
pub fn run(shared: Arc<Mutex<Shared>>, macro_path: PathBuf) {
    let mut pads = match Pads::new() {
        Ok(p) => p,
        Err(e) => {
            shared.lock().unwrap().log(Message::new("log.input_failed").text("error", e));
            return;
        }
    };
    // SAFETY: plain WinMM call; balanced by timeEndPeriod below.
    unsafe { timeBeginPeriod(1) };

    // Output device state.
    let mut output: Option<Output> = None;
    let mut out_pref: Option<OutputPref> = None;
    let mut virtual_id: Option<GamepadId> = None;
    let mut last_out: Option<PadState> = None;
    let mut next_scan = Instant::now();
    let mut auto_selected = false;

    // Start/stop binding state.
    let mut toggle = Toggle::new(shared.lock().unwrap().binding.clone());
    let mut capture: Option<Capture> = None;

    // Feature state.
    let mut prev_active: Option<Feature> = None;
    let mut t0 = Instant::now();
    let mut turbo_t0 = Instant::now();
    let mut last_target = 0u32;
    let mut rec: Vec<MacroFrame> = Vec::new();
    let mut rec_t0: Option<Instant> = None;
    let mut rec_last: Option<PadState> = None;
    let mut play_idx = 0usize;
    let mut last_play_t = 0u64;

    loop {
        let now = Instant::now();

        // ---- (Re)build the output device when the preference changes.
        let want = shared.lock().unwrap().settings.output;
        if out_pref != Some(want) {
            if let Some(o) = output.take() {
                o.shutdown();
            }
            pads.pump();
            let before = pads.ids();
            let (o, status, vid) = match want {
                OutputPref::ViGEm => match Output::vigem() {
                    Ok(o) => {
                        let vid = pads.wait_new(&before, VIRTUAL_DETECT_TIMEOUT);
                        (o, Message::new("output.status.vigem"), vid)
                    }
                    Err(e) => (Output::keyboard(), Message::new("output.status.fallback").msg("detail", e), None),
                },
                OutputPref::Keyboard => (Output::keyboard(), Message::new("output.status.keyboard"), None),
            };
            let mut sh = shared.lock().unwrap();
            sh.output_is_vigem = o.is_vigem();
            sh.log(status.clone());
            sh.output_status = status;
            output = Some(o);
            out_pref = Some(want);
            virtual_id = vid;
            last_out = None;
            next_scan = now;
        }
        let out_dev = output.as_mut().expect("output is created above");

        // ---- Read the controller selected in the GUI.
        pads.pump();
        let (selected, capturing) = {
            let sh = shared.lock().unwrap();
            if sh.binding.label() != toggle.binding().label() {
                toggle = Toggle::new(sh.binding.clone());
            }
            (sh.selected, sh.binding_capture)
        };
        let phys = selected
            .and_then(|i| pads.find(i))
            .filter(|&id| Some(id) != virtual_id)
            .and_then(|id| pads.state(id));
        let slots_update = (now >= next_scan).then(|| {
            next_scan = now + SLOT_REFRESH;
            pads.ids()
                .into_iter()
                .filter(|&id| Some(id) != virtual_id)
                .filter_map(|id| {
                    pads.info(id).map(|(name, power)| SlotInfo { index: usize::from(id), name, power })
                })
                .collect::<Vec<_>>()
        });
        let raw_mask = phys.map(|p| p.mask()).unwrap_or(0);

        // ---- Binding capture requested by the GUI replaces the toggle.
        let mut captured: Option<Binding> = None;
        if capturing {
            captured = capture.get_or_insert_with(Capture::new).poll(raw_mask);
        } else {
            capture = None;
        }

        // ---- Start/stop toggle. While a controller binding is fully held,
        // its buttons are hidden from everything below.
        let toggled = !capturing && toggle.poll(raw_mask);
        let pad_binding = toggle.binding().pad_mask();
        let hide = pad_binding != 0 && raw_mask & pad_binding == pad_binding;
        let phys = phys.map(|p| if hide { p.with_mask(pad_binding, false) } else { p });
        let phys_state = phys.unwrap_or_default();
        let phys_mask = if phys.is_some() { phys_state.mask() } else { 0 };

        // ---- Update shared state and compose the output.
        let out = {
            let mut guard = shared.lock().unwrap();
            let sh = &mut *guard;
            if let Some(s) = slots_update {
                // Default to the first controller ever detected. This happens
                // once; afterwards the user's choice in the GUI is left alone.
                if !auto_selected {
                    if let Some(first) = s.first() {
                        sh.selected.get_or_insert(first.index);
                        auto_selected = true;
                    }
                }
                sh.slots = s;
            }
            sh.phys = phys_state;

            if sh.quit {
                if prev_active == Some(Feature::Record) {
                    finish_recording(sh, &mut rec, rec_t0, now, &macro_path);
                }
                break;
            }

            if let Some(b) = captured {
                sh.binding = b;
                sh.binding_capture = false;
                capture = None;
            }
            if toggled {
                sh.toggle();
            }
            if let Some((f, at)) = sh.pending {
                if now >= at {
                    sh.pending = None;
                    sh.active = Some(f);
                }
            }
            if sh.capture {
                sh.settings.turbo_mask |= phys_mask & SELECTABLE;
            }

            // Transitions between features.
            if sh.active != prev_active {
                if prev_active == Some(Feature::Record) {
                    finish_recording(sh, &mut rec, rec_t0, now, &macro_path);
                }
                match sh.active {
                    Some(Feature::Turbo) => {
                        turbo_t0 = now;
                        last_target = 0;
                        let msg = Message::new("log.turbo_started")
                            .text("buttons", pad::format_mask(sh.settings.turbo_mask))
                            .text("interval", sh.settings.interval_ms);
                        sh.log(msg);
                    }
                    Some(Feature::Record) => {
                        rec.clear();
                        rec_t0 = None;
                        rec_last = None;
                        sh.rec_frames = 0;
                        sh.log(Message::new("log.record_started"));
                    }
                    Some(Feature::Play) => {
                        play_idx = 0;
                        last_play_t = 0;
                        sh.log(Message::new("log.play_started"));
                    }
                    None => {
                        if let Some(p) = prev_active {
                            sh.log(Message::new("log.stopped").key("feature", p.key()));
                        }
                    }
                }
                t0 = now;
                prev_active = sh.active;
            }
            sh.run_ms = ms(now - t0);

            // Recording: store a frame whenever the state changes. The clock
            // starts at the first frame, so leading idle time is not recorded.
            if sh.active == Some(Feature::Record) {
                if let Some(p) = phys {
                    if rec_last != Some(p) {
                        let t = match rec_t0 {
                            Some(r) => ms(now - r),
                            None => {
                                rec_t0 = Some(now);
                                0
                            }
                        };
                        rec.push(MacroFrame { t, s: p });
                        rec_last = Some(p);
                    }
                }
                sh.rec_frames = rec.len();
            }

            // Base state: pass-through for ViGEm, neutral for the keyboard.
            let mut out = if out_dev.is_vigem() { phys_state } else { PadState::default() };

            // Playback: advance to the last frame whose time has passed.
            if sh.active == Some(Feature::Play) {
                if let Some(m) = &sh.macro_data {
                    let t = ms(now - t0) % m.duration_ms.max(1);
                    if t < last_play_t {
                        play_idx = 0; // wrapped around
                    }
                    last_play_t = t;
                    while play_idx + 1 < m.frames.len() && m.frames[play_idx + 1].t <= t {
                        play_idx += 1;
                    }
                    out = m.frames[play_idx].s;
                    sh.play_pos_ms = t;
                }
            }

            // Turbo: pressed for the first `press_ms` of every period.
            let s = &sh.settings;
            if sh.active == Some(Feature::Turbo) && s.turbo_mask != 0 {
                let target = match s.turbo_mode {
                    TurboMode::Always => s.turbo_mask,
                    TurboMode::Hold => s.turbo_mask & phys_mask,
                };
                // Restart the phase when a held button is first pressed, so the
                // first pulse fires immediately.
                if target != 0 && last_target == 0 {
                    turbo_t0 = now;
                }
                last_target = target;
                if target != 0 {
                    let period = s.interval_ms.max(1) as u64;
                    let on = ms(now - turbo_t0) % period < press_ms(s.interval_ms) as u64;
                    out = out.with_mask(target, on);
                }
            }
            out
        };

        if last_out != Some(out) {
            out_dev.send(&out);
            last_out = Some(out);
        }

        std::thread::sleep(Duration::from_millis(1));
    }

    if let Some(o) = output.take() {
        o.shutdown();
    }
    // SAFETY: balances timeBeginPeriod above.
    unsafe { timeEndPeriod(1) };
}
