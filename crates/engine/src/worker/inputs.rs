//! Sampling the inputs once per tick: the selected controller, the start/stop
//! toggle and a binding capture requested by the GUI.

use crate::shared::{Shared, SlotInfo, lock};
use gilrs::GamepadId;
use input::binding::{Binding, Capture, Toggle};
use input::pad::{PadState, Pads};
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// How often the list of connected controllers is refreshed.
const SLOT_REFRESH: Duration = Duration::from_secs(1);

/// What the inputs looked like in one tick.
pub(super) struct Sample {
    /// The selected controller, with the toggle's buttons hidden while the
    /// whole toggle binding is held; `None` if no controller is selected or
    /// connected.
    pub(super) phys: Option<PadState>,
    /// Fresh controller list, when it was rescanned this tick.
    pub(super) slots: Option<Vec<SlotInfo>>,
    /// A binding the capture finished recording this tick.
    pub(super) captured: Option<Binding>,
    /// Whether the toggle fired this tick.
    pub(super) toggled: bool,
}

impl Sample {
    /// The controller state, neutral without a controller.
    pub(super) fn state(&self) -> PadState {
        self.phys.unwrap_or_default()
    }

    /// Logical mask of the controller, 0 without a controller.
    pub(super) fn mask(&self) -> u32 {
        self.phys.map_or(0, |p| p.mask())
    }
}

/// Controller reader plus toggle and capture state.
pub(super) struct Inputs {
    /// gilrs wrapper.
    pads: Pads,
    /// Edge detector for the start/stop binding.
    toggle: Toggle,
    /// Running binding capture, while the GUI requests one.
    capture: Option<Capture>,
    /// When the controller list is rescanned next.
    next_scan: Instant,
}

impl Inputs {
    /// Starts watching `binding`.
    pub(super) fn new(pads: Pads, binding: Binding) -> Self {
        Self { pads, toggle: Toggle::new(binding), capture: None, next_scan: Instant::now() }
    }

    /// The controller reader, e.g. to detect a new virtual controller.
    pub(super) const fn pads(&mut self) -> &mut Pads {
        &mut self.pads
    }

    /// Rescans the controller list on the next tick.
    pub(super) fn rescan(&mut self) {
        self.next_scan = Instant::now();
    }

    /// Reads everything for this tick. `virtual_id` (our own virtual
    /// controller) is never read and never listed.
    pub(super) fn sample(
        &mut self,
        shared: &Mutex<Shared>,
        virtual_id: Option<GamepadId>,
        now: Instant,
    ) -> Sample {
        self.pads.pump();
        let (selected, capturing) = {
            let sh = lock(shared);
            if sh.binding.label() != self.toggle.binding().label() {
                self.toggle = Toggle::new(sh.binding.clone());
            }
            (sh.selected, sh.binding_capture)
        };
        let phys = selected
            .and_then(|i| self.pads.find(i))
            .filter(|&id| Some(id) != virtual_id)
            .and_then(|id| self.pads.state(id));
        let raw_mask = phys.map_or(0, |p| p.mask());
        let slots = self.scan_slots(virtual_id, now);
        // A capture requested by the GUI replaces the toggle.
        let captured = self.poll_capture(capturing, raw_mask);
        let toggled = !capturing && self.toggle.poll(raw_mask);
        Sample { phys: phys.map(|p| self.hide_binding(p, raw_mask)), slots, captured, toggled }
    }

    /// The connected controllers, if a rescan is due.
    fn scan_slots(&mut self, virtual_id: Option<GamepadId>, now: Instant) -> Option<Vec<SlotInfo>> {
        if now < self.next_scan {
            return None;
        }
        self.next_scan = now + SLOT_REFRESH;
        let slots = self
            .pads
            .ids()
            .into_iter()
            .filter(|&id| Some(id) != virtual_id)
            .filter_map(|id| {
                self.pads.info(id).map(|(name, power)| SlotInfo { index: usize::from(id), name, power })
            })
            .collect();
        Some(slots)
    }

    /// Advances a binding capture while `capturing`; returns the binding once
    /// it is complete.
    fn poll_capture(&mut self, capturing: bool, raw_mask: u32) -> Option<Binding> {
        if !capturing {
            self.capture = None;
            return None;
        }
        let captured = self.capture.get_or_insert_with(Capture::new).poll(raw_mask);
        if captured.is_some() {
            self.capture = None;
        }
        captured
    }

    /// Removes the toggle's controller buttons from `p` while the whole
    /// binding is held, so the toggle is neither forwarded nor recorded.
    const fn hide_binding(&self, p: PadState, raw_mask: u32) -> PadState {
        let pad_binding = self.toggle.binding().pad_mask();
        if pad_binding != 0 && raw_mask & pad_binding == pad_binding {
            p.with_mask(pad_binding, false)
        } else {
            p
        }
    }
}
