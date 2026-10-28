//! The worker thread: polls input, runs turbo / macro logic and drives the output.
//!
//! | module        | responsibility                                          |
//! |---------------|---------------------------------------------------------|
//! | [`settings`]  | user settings persisted to `settings.json`              |
//! | [`shared`]    | state shared with the GUI, start/stop requests          |
//! | [`recording`] | the macro format, recording and playback                |
//! | [`worker`]    | the 1 ms loop ([`run`])                                 |
//!
//! # Threading model
//!
//! The GUI thread and the worker share one [`Shared`] value behind a
//! [`Mutex`](std::sync::Mutex). The GUI edits settings and requests start/stop; the worker reads
//! those requests once per tick and publishes status (controller list, live
//! input, progress, log) back into the same struct. Each lock is held only for
//! a few microseconds, so neither side blocks the other noticeably.
//!
//! # Tick
//!
//! [`run`] loops roughly once per millisecond (the system timer resolution is
//! raised to 1 ms with `timeBeginPeriod`). Each tick:
//!
//! 1. (re)creates the [`Output`](output::Output) if the user switched the output device, and
//!    points the keyboard output at the focused window or the target
//!    executable;
//! 2. reads the controller selected in the GUI;
//! 3. polls the start/stop [`Toggle`](input::binding::Toggle) (keyboard and/or controller buttons);
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

pub mod recording;
pub mod settings;
pub mod shared;
pub mod worker;

pub use recording::{Macro, MacroFrame};
pub use settings::{Feature, OutputPref, Settings, TargetMode, TurboMode, press_ms};
pub use shared::{COUNTDOWN, Shared, SlotInfo, lock};
pub use worker::run;

use std::time::Duration;

/// Whole milliseconds in `d`.
pub(crate) const fn ms(d: Duration) -> u64 {
    d.as_millis() as u64
}

/// Seconds with one decimal, for log messages.
pub(crate) fn secs(ms: u64) -> String {
    format!("{:.1}", ms as f64 / 1000.0)
}
