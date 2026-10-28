//! The output device the worker drives.

use crate::settings::OutputPref;
use crate::shared::Shared;
use gilrs::GamepadId;
use i18n::Message;
use input::pad::{PadState, Pads};
use output::Output;
use std::time::Duration;

/// How long to wait for the virtual controller to appear as an input device.
const VIRTUAL_DETECT_TIMEOUT: Duration = Duration::from_secs(2);

/// An [`Output`] plus what the worker needs to know about it.
pub(super) struct Device {
    /// The device itself.
    output: Output,
    /// Preference it was created for (it may have fallen back to keyboard).
    pref: OutputPref,
    /// The virtual controller as seen by gilrs, hidden from the controller list.
    virtual_id: Option<GamepadId>,
    /// Last state sent, to send changes only.
    last_sent: Option<PadState>,
}

impl Device {
    /// Creates the output for `pref`, falling back to the keyboard if ViGEm is
    /// unavailable. Returns the device and a status message for the GUI.
    pub(super) fn open(pref: OutputPref, pads: &mut Pads) -> (Self, Message) {
        pads.pump();
        let before = pads.ids();
        let (output, status, virtual_id) = match pref {
            OutputPref::ViGEm => match Output::vigem() {
                Ok(o) => {
                    let vid = pads.wait_new(&before, VIRTUAL_DETECT_TIMEOUT);
                    (o, Message::new("output.status.vigem"), vid)
                }
                Err(e) => (
                    Output::keyboard(),
                    Message::new("output.status.fallback").msg("detail", e),
                    None,
                ),
            },
            OutputPref::Keyboard => (Output::keyboard(), Message::new("output.status.keyboard"), None),
        };
        let device = Self { output, pref, virtual_id, last_sent: None };
        (device, status)
    }

    /// Preference the device was created for.
    pub(super) const fn pref(&self) -> OutputPref {
        self.pref
    }

    /// gilrs id of the virtual controller, if this is one.
    pub(super) const fn virtual_id(&self) -> Option<GamepadId> {
        self.virtual_id
    }

    /// Whether this is the virtual controller.
    pub(super) const fn is_vigem(&self) -> bool {
        self.output.is_vigem()
    }

    /// Points the keyboard output at the process chosen in the settings (or
    /// back at the focused window) and publishes the target status.
    pub(super) fn sync_target(&mut self, sh: &mut Shared) {
        let spec = if self.is_vigem() { None } else { sh.settings.target_spec() };
        if self.output.target_spec() != spec {
            self.output.set_target(spec);
            self.last_sent = None;
        }
        let status = self.output.target_status();
        let current = sh.target_status.as_ref().map(|(p, n)| (p.as_slice(), *n));
        if current != status {
            sh.target_status = status.map(|(p, n)| (p.to_vec(), n));
        }
    }

    /// Sends `s` if it differs from the previously sent state.
    pub(super) fn send(&mut self, s: PadState) {
        if self.last_sent != Some(s) {
            self.output.send(&s);
            self.last_sent = Some(s);
        }
    }

    /// Releases every key and button.
    pub(super) fn release(&mut self) {
        self.send(PadState::default());
    }

    /// Releases everything and tears the device down.
    pub(super) fn shutdown(self) {
        self.output.shutdown();
    }
}
