//! The worker loop; see the [crate docs](crate) for what one tick does.
//!
//! | module     | responsibility                                              |
//! |------------|-------------------------------------------------------------|
//! | `device`   | the output device: creation, keyboard target, sending       |
//! | `inputs`   | sampling the controller, the toggle and binding capture     |
//! | `features` | feature transitions and composing the output state          |

mod device;
mod features;
mod inputs;

use crate::settings::OutputPref;
use crate::shared::{Shared, lock};
use device::Device;
use features::Features;
use i18n::Message;
use input::pad::{PadState, Pads, SELECTABLE};
use inputs::{Inputs, Sample};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use windows_sys::Win32::Media::{timeBeginPeriod, timeEndPeriod};

/// Worker-thread entry point. Returns when [`Shared::quit`] is set.
///
/// `macro_path` is where finished recordings are saved. The toggle binding is
/// taken from [`Shared::binding`] and re-read whenever the GUI changes it.
///
/// Before returning, the output is shut down (all keys/buttons released, the
/// virtual controller unplugged) and an unfinished recording is saved.
pub fn run(shared: Arc<Mutex<Shared>>, macro_path: PathBuf) {
    let pads = match Pads::new() {
        Ok(p) => p,
        Err(e) => {
            lock(&shared).log(Message::new("log.input_failed").text("error", e));
            return;
        }
    };
    let _timer = TimerPeriod::begin();
    let mut worker = Worker::new(&shared, pads, macro_path);
    while worker.tick(&shared) {
        std::thread::sleep(Duration::from_millis(1));
    }
    worker.device.shutdown();
}

/// Raises the system timer resolution to 1 ms while it is alive, so that the
/// 1 ms sleep of the loop is not rounded up to the default 15.6 ms.
struct TimerPeriod;

impl TimerPeriod {
    /// Calls `timeBeginPeriod(1)`; dropping the value balances it.
    fn begin() -> Self {
        // SAFETY: plain WinMM call; balanced by `Drop`.
        unsafe { timeBeginPeriod(1) };
        Self
    }
}

impl Drop for TimerPeriod {
    fn drop(&mut self) {
        // SAFETY: balances the timeBeginPeriod in `begin`.
        unsafe { timeEndPeriod(1) };
    }
}

/// Everything the worker keeps between ticks.
struct Worker {
    /// Controller, toggle and capture state.
    inputs: Inputs,
    /// The active output device.
    device: Device,
    /// Feature state (turbo phase, recording, playback).
    features: Features,
    /// Whether a controller has been pre-selected for the user yet.
    auto_selected: bool,
}

impl Worker {
    /// Opens the output device chosen in the settings.
    fn new(shared: &Mutex<Shared>, mut pads: Pads, macro_path: PathBuf) -> Self {
        let (pref, binding) = {
            let sh = lock(shared);
            (sh.settings.output, sh.binding.clone())
        };
        let device = open_device(shared, &mut pads, pref);
        Self {
            inputs: Inputs::new(pads, binding),
            device,
            features: Features::new(macro_path),
            auto_selected: false,
        }
    }

    /// Runs one iteration. Returns `false` once the GUI asked to quit.
    fn tick(&mut self, shared: &Mutex<Shared>) -> bool {
        let now = Instant::now();
        self.ensure_device(shared);
        self.device.sync(&mut lock(shared));
        let mut sample = self.inputs.sample(shared, self.device.virtual_id(), now);

        let mut guard = lock(shared);
        let sh = &mut *guard;
        self.publish_input(sh, &mut sample);
        if sh.quit {
            self.features.stop(sh, now);
            return false;
        }
        apply_requests(sh, &mut sample, now);
        self.features.transition(sh, now);
        // Pass-through for ViGEm, neutral for the keyboard.
        let base = if self.device.is_vigem() { sample.state() } else { PadState::default() };
        let out = self.features.compose(sh, &sample, base, now);
        drop(guard);

        self.device.send(out);
        true
    }

    /// Replaces the output device when the user switched the preference.
    ///
    /// Keys held on the old device are released before the new one is
    /// created, and the controller list is rescanned right away so that a new
    /// virtual controller is hidden from it.
    fn ensure_device(&mut self, shared: &Mutex<Shared>) {
        let want = lock(shared).settings.output;
        if self.device.pref() == want {
            return;
        }
        self.device.release();
        let new = open_device(shared, self.inputs.pads(), want);
        std::mem::replace(&mut self.device, new).shutdown();
        self.inputs.rescan();
    }

    /// Publishes the controller list and live input for the GUI.
    ///
    /// The first controller ever detected is pre-selected once; afterwards
    /// the user's choice in the GUI is left alone.
    fn publish_input(&mut self, sh: &mut Shared, sample: &mut Sample) {
        if let Some(slots) = sample.slots.take() {
            if !self.auto_selected
                && let Some(first) = slots.first()
            {
                sh.selected.get_or_insert(first.index);
                self.auto_selected = true;
            }
            sh.slots = slots;
        }
        sh.phys = sample.state();
    }
}

/// Creates the output for `pref` and reports its status to the GUI.
fn open_device(shared: &Mutex<Shared>, pads: &mut Pads, pref: OutputPref) -> Device {
    let keymap = lock(shared).settings.keymap.clone();
    let (device, status) = Device::open(pref, pads, &keymap);
    let mut sh = lock(shared);
    sh.output_is_vigem = device.is_vigem();
    sh.log(status.clone());
    sh.output_status = status;
    device
}

/// Applies this tick's requests: a captured binding, a toggle press, an
/// expired countdown and turbo-button capture from the controller.
fn apply_requests(sh: &mut Shared, sample: &mut Sample, now: Instant) {
    if let Some(b) = sample.captured.take() {
        sh.binding = b;
        sh.binding_capture = false;
    }
    if sample.toggled {
        sh.toggle();
    }
    if let Some((f, at)) = sh.pending
        && now >= at
    {
        sh.pending = None;
        sh.active = Some(f);
    }
    if sh.capture {
        sh.settings.turbo_mask |= sample.mask() & SELECTABLE;
    }
}
