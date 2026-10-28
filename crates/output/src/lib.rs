//! Output devices: a ViGEm virtual Xbox 360 controller or the keyboard.
//!
//! The worker thread produces one [`PadState`] per tick and hands it to an
//! [`Output`]. Two implementations exist:
//!
//! * [`Output::ViGEm`] — a virtual Xbox 360 controller created through the
//!   ViGEmBus kernel driver with [`vigem_client`]. Games see it as a real
//!   XInput pad, so turbo and macros work in any controller-driven game. The
//!   client library talks to the driver directly; no DLL is required, only the
//!   driver itself must be installed.
//! * [`Output::Keyboard`] — injects key presses with `SendInput`, using
//!   hardware scan codes so that DirectInput/raw-input games also accept them.
//!   The mapping is fixed in [`KEYMAP`]. Keys go to the focused window, or,
//!   when a target executable is set, are posted to that program's windows
//!   so it can be driven in the background (see [`target`]).

pub mod target;

use i18n::Message;
use input::pad::{self, PadState};
use target::WindowTarget;
use vigem_client::{Client, TargetId, XButtons, XGamepad, Xbox360Wired};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::*;

/// Keyboard output mapping: `(logical bit, virtual-key code, control id, key label)`.
///
/// The control id is shown in the GUI; ids that have a `control.<id>` entry in
/// the translation files (stick and D-pad directions) are translated, the
/// others (A, B, LB, ...) are shown as-is. The key label is the name of the
/// keyboard key and is not translated.
pub const KEYMAP: [(u32, u16, &str, &str); 24] = [
    (0x1000, 0x5A, "A", "Z"),
    (0x2000, 0x58, "B", "X"),
    (0x4000, 0x43, "X", "C"),
    (0x8000, 0x56, "Y", "V"),
    (0x0100, 0x51, "LB", "Q"),
    (0x0200, 0x45, "RB", "E"),
    (pad::LT, 0x31, "LT", "1"),
    (pad::RT, 0x33, "RT", "3"),
    (0x0010, 0x0D, "START", "Enter"),
    (0x0020, 0x08, "SELECT", "BackSpace"),
    (0x0040, 0x46, "LS", "F"),
    (0x0080, 0x47, "RS", "G"),
    (0x0001, 0x26, "UP", "↑"),
    (0x0002, 0x28, "DOWN", "↓"),
    (0x0004, 0x25, "LEFT", "←"),
    (0x0008, 0x27, "RIGHT", "→"),
    (pad::LS_UP, 0x57, "LS_UP", "W"),
    (pad::LS_DOWN, 0x53, "LS_DOWN", "S"),
    (pad::LS_LEFT, 0x41, "LS_LEFT", "A"),
    (pad::LS_RIGHT, 0x44, "LS_RIGHT", "D"),
    (pad::RS_UP, 0x49, "RS_UP", "I"),
    (pad::RS_DOWN, 0x4B, "RS_DOWN", "K"),
    (pad::RS_LEFT, 0x4A, "RS_LEFT", "J"),
    (pad::RS_RIGHT, 0x4C, "RS_RIGHT", "L"),
];

/// Where generated input is delivered.
pub enum Output {
    /// Virtual Xbox 360 controller plugged into ViGEmBus.
    ViGEm(Xbox360Wired<Client>),
    /// Keyboard injection. `held` is the logical mask whose keys are currently
    /// held down, used to send only the key transitions that changed.
    Keyboard {
        /// Logical bits whose keys are currently pressed.
        held: u32,
        /// Program the keys are posted to; `None` sends to the focused window.
        target: Option<WindowTarget>,
    },
}

impl Output {
    /// Connects to ViGEmBus and plugs in a wired Xbox 360 controller.
    ///
    /// Blocks until the driver reports the device as ready.
    ///
    /// # Errors
    ///
    /// Returns a translatable [`Message`] if the driver is not installed or
    /// the device cannot be created.
    pub fn vigem() -> Result<Self, Message> {
        let err = |key, e: vigem_client::Error| Message::new(key).text("error", format!("{e:?}"));
        let client = Client::connect().map_err(|e| err("output.error.connect", e))?;
        let mut target = Xbox360Wired::new(client, TargetId::XBOX360_WIRED);
        target.plugin().map_err(|e| err("output.error.plugin", e))?;
        target
            .wait_ready()
            .map_err(|e| err("output.error.ready", e))?;
        Ok(Self::ViGEm(target))
    }

    /// Creates a keyboard output with no keys held.
    pub const fn keyboard() -> Self {
        Self::Keyboard {
            held: 0,
            target: None,
        }
    }

    /// Whether this is the virtual-controller output.
    pub const fn is_vigem(&self) -> bool {
        matches!(self, Self::ViGEm(_))
    }

    /// Executable name and process id the keyboard output posts to, if any
    /// (see [`WindowTarget::spec`]).
    pub fn target_spec(&self) -> Option<(&str, Option<u32>)> {
        match self {
            Self::Keyboard {
                target: Some(t), ..
            } => Some(t.spec()),
            _ => None,
        }
    }

    /// Processes and window count of the target (see
    /// [`WindowTarget::status`]), or `None` when keys go to the focused
    /// window.
    pub fn target_status(&mut self) -> Option<(&[u32], usize)> {
        match self {
            Self::Keyboard {
                target: Some(t), ..
            } => Some(t.status()),
            _ => None,
        }
    }

    /// Redirects keyboard output to the windows of `spec` (executable name and
    /// optional process id), or back to the focused window with `None`. Keys
    /// held on the old destination are released first. Does nothing for the
    /// virtual controller, which every program sees.
    pub fn set_target(&mut self, spec: Option<(&str, Option<u32>)>) {
        if self.is_vigem() {
            return;
        }
        self.send(&PadState::default());
        if let Self::Keyboard { target, .. } = self {
            *target = spec.map(|(exe, pid)| WindowTarget::new(exe, pid));
        }
    }

    /// Delivers `s` to the device.
    ///
    /// For ViGEm the whole report is submitted. For the keyboard, `s` is
    /// collapsed with [`PadState::mask`] and only keys whose state changed
    /// since the previous call are pressed or released. Errors from the
    /// driver are ignored: a dropped report is corrected by the next tick.
    pub fn send(&mut self, s: &PadState) {
        match self {
            Self::ViGEm(target) => {
                let _ = target.update(&XGamepad {
                    buttons: XButtons { raw: s.buttons },
                    left_trigger: s.lt,
                    right_trigger: s.rt,
                    thumb_lx: s.lx,
                    thumb_ly: s.ly,
                    thumb_rx: s.rx,
                    thumb_ry: s.ry,
                });
            }
            Self::Keyboard { held, target } => {
                let m = s.mask();
                let diff = m ^ *held;
                for &(bit, vk, _, _) in KEYMAP.iter() {
                    if diff & bit != 0 {
                        let down = m & bit != 0;
                        match target {
                            Some(t) => t.post_key(vk, down, is_extended(vk)),
                            None => send_key(vk, down),
                        }
                    }
                }
                *held = m;
            }
        }
    }

    /// Releases everything and tears the device down.
    ///
    /// Sends a neutral state first so that no key or button stays stuck, then
    /// unplugs the virtual controller if there is one.
    pub fn shutdown(mut self) {
        self.send(&PadState::default());
        if let Self::ViGEm(mut target) = self {
            let _ = target.unplug();
        }
    }
}

/// Whether `vk` is a navigation key (arrows, Home/End, PageUp/PageDown,
/// Insert/Delete) on the extended scan-code page.
const fn is_extended(vk: u16) -> bool {
    matches!(vk, 0x21..=0x28 | 0x2D | 0x2E)
}

/// Presses (`down`) or releases a key identified by virtual-key code `vk`.
///
/// The key is sent as a hardware scan code (`KEYEVENTF_SCANCODE`) because many
/// games read raw scan codes and ignore virtual-key-only events. Navigation
/// keys ([`is_extended`]) get `KEYEVENTF_EXTENDEDKEY`.
fn send_key(vk: u16, down: bool) {
    let mut flags = KEYEVENTF_SCANCODE;
    if !down {
        flags |= KEYEVENTF_KEYUP;
    }
    if is_extended(vk) {
        flags |= KEYEVENTF_EXTENDEDKEY;
    }
    // SAFETY: `input` is a fully initialised INPUT of type INPUT_KEYBOARD, and
    // the size passed matches the structure, as SendInput requires.
    unsafe {
        let input = INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: vk,
                    wScan: MapVirtualKeyW(vk as u32, MAPVK_VK_TO_VSC) as u16,
                    dwFlags: flags,
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        };
        SendInput(1, &input, std::mem::size_of::<INPUT>() as i32);
    }
}
