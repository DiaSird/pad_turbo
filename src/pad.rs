//! Gamepad input through [`gilrs`] and the button model shared by the whole app.
//!
//! # State model
//!
//! [`PadState`] mirrors the layout of the XInput `XINPUT_GAMEPAD` report:
//! a 16-bit button field, two 8-bit triggers and four signed 16-bit stick
//! axes. Because ViGEm's Xbox 360 target consumes exactly the same report, a
//! state read from the physical controller can be forwarded verbatim, and macro
//! frames can be replayed without any conversion.
//!
//! # Logical mask
//!
//! Several features need to treat triggers and stick directions like buttons
//! (turbo on `LT`, keyboard keys for stick directions, ...). For that purpose a
//! state can be collapsed into a 32-bit *logical mask* with [`PadState::mask`]:
//!
//! | bits    | meaning                                                   |
//! |---------|-----------------------------------------------------------|
//! | 0–15    | XInput buttons, same values as `XINPUT_GAMEPAD_*`          |
//! | 16–17   | [`LT`], [`RT`] (pressed past [`TRIGGER_THRESHOLD`])        |
//! | 18–21   | left stick up/down/left/right (past [`STICK_THRESHOLD`])   |
//! | 22–25   | right stick up/down/left/right                             |
//!
//! # Backend
//!
//! gilrs is built with its `xinput` backend. The default Windows Gaming Input
//! backend only delivers events while one of the process's windows has focus,
//! which would make the tool useless while a game is in the foreground.

use gilrs::{Axis, Button, GamepadId, Gilrs, PowerInfo};
use serde::{Deserialize, Serialize};
use std::time::{Duration, Instant};

/// Logical bit: left trigger pressed.
pub const LT: u32 = 0x1_0000;
/// Logical bit: right trigger pressed.
pub const RT: u32 = 0x2_0000;
/// Logical bit: left stick pushed up.
pub const LS_UP: u32 = 0x4_0000;
/// Logical bit: left stick pushed down.
pub const LS_DOWN: u32 = 0x8_0000;
/// Logical bit: left stick pushed left.
pub const LS_LEFT: u32 = 0x10_0000;
/// Logical bit: left stick pushed right.
pub const LS_RIGHT: u32 = 0x20_0000;
/// Logical bit: right stick pushed up.
pub const RS_UP: u32 = 0x40_0000;
/// Logical bit: right stick pushed down.
pub const RS_DOWN: u32 = 0x80_0000;
/// Logical bit: right stick pushed left.
pub const RS_LEFT: u32 = 0x100_0000;
/// Logical bit: right stick pushed right.
pub const RS_RIGHT: u32 = 0x200_0000;

/// Trigger value (0–255) above which a trigger counts as pressed.
///
/// Same value as XInput's `XINPUT_GAMEPAD_TRIGGER_THRESHOLD`.
pub const TRIGGER_THRESHOLD: u8 = 30;

/// Stick deflection (out of ±32767) above which a direction counts as pressed.
pub const STICK_THRESHOLD: i16 = 16000;

/// Buttons that can be selected for turbo, as `(label, logical bit)`.
///
/// The order defines the order of the check boxes and of the live input
/// display in the GUI.
pub const BUTTONS: [(&str, u32); 16] = [
    ("A", 0x1000),
    ("B", 0x2000),
    ("X", 0x4000),
    ("Y", 0x8000),
    ("LB", 0x0100),
    ("RB", 0x0200),
    ("LT", LT),
    ("RT", RT),
    ("START", 0x0010),
    ("SELECT", 0x0020),
    ("LS", 0x0040),
    ("RS", 0x0080),
    ("↑", 0x0001),
    ("↓", 0x0002),
    ("←", 0x0004),
    ("→", 0x0008),
];

/// Union of every bit listed in [`BUTTONS`].
///
/// Excludes the Guide button (`0x0400`) and the unused bit `0x0800`, as well as
/// the stick-direction bits, none of which make sense as turbo targets.
pub const SELECTABLE: u32 = 0xF3FF | LT | RT;

/// Mapping from gilrs buttons to XInput button bits.
///
/// gilrs uses positional names: `South` is the bottom face button (A on an Xbox
/// pad), `LeftTrigger` is the bumper (LB) and `LeftTrigger2` the analog trigger
/// (LT), which is read separately in [`Pads::state`].
const BUTTON_MAP: [(Button, u16); 15] = [
    (Button::South, 0x1000),
    (Button::East, 0x2000),
    (Button::West, 0x4000),
    (Button::North, 0x8000),
    (Button::LeftTrigger, 0x0100),
    (Button::RightTrigger, 0x0200),
    (Button::Start, 0x0010),
    (Button::Select, 0x0020),
    (Button::LeftThumb, 0x0040),
    (Button::RightThumb, 0x0080),
    (Button::DPadUp, 0x0001),
    (Button::DPadDown, 0x0002),
    (Button::DPadLeft, 0x0004),
    (Button::DPadRight, 0x0008),
    (Button::Mode, 0x0400),
];

/// A complete snapshot of a gamepad, laid out like `XINPUT_GAMEPAD`.
///
/// This is the unit that is read from the physical controller, recorded into
/// macros, and sent to the output device.
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct PadState {
    /// XInput button bits (`XINPUT_GAMEPAD_*`).
    pub buttons: u16,
    /// Left trigger, 0 (released) – 255 (fully pressed).
    pub lt: u8,
    /// Right trigger, 0 (released) – 255 (fully pressed).
    pub rt: u8,
    /// Left stick X, −32768 (left) – 32767 (right).
    pub lx: i16,
    /// Left stick Y, −32768 (down) – 32767 (up).
    pub ly: i16,
    /// Right stick X, −32768 (left) – 32767 (right).
    pub rx: i16,
    /// Right stick Y, −32768 (down) – 32767 (up).
    pub ry: i16,
}

impl PadState {
    /// Collapses the state into a logical mask (see the [module docs](self)).
    ///
    /// Analog inputs are converted with [`TRIGGER_THRESHOLD`] and
    /// [`STICK_THRESHOLD`].
    pub fn mask(&self) -> u32 {
        let t = STICK_THRESHOLD;
        let mut m = self.buttons as u32;
        if self.lt > TRIGGER_THRESHOLD {
            m |= LT;
        }
        if self.rt > TRIGGER_THRESHOLD {
            m |= RT;
        }
        if self.ly > t {
            m |= LS_UP;
        }
        if self.ly < -t {
            m |= LS_DOWN;
        }
        if self.lx < -t {
            m |= LS_LEFT;
        }
        if self.lx > t {
            m |= LS_RIGHT;
        }
        if self.ry > t {
            m |= RS_UP;
        }
        if self.ry < -t {
            m |= RS_DOWN;
        }
        if self.rx < -t {
            m |= RS_LEFT;
        }
        if self.rx > t {
            m |= RS_RIGHT;
        }
        m
    }

    /// Returns a copy with the buttons and triggers in `mask` forced on or off.
    ///
    /// Button bits are set or cleared; [`LT`] / [`RT`] set the trigger to 255
    /// (`on`) or 0 (`!on`). Stick-direction bits are ignored, so sticks are
    /// never modified by turbo.
    ///
    /// ```ignore
    /// let s = PadState::default().with_mask(0x1000 | LT, true);
    /// assert_eq!((s.buttons, s.lt), (0x1000, 255));
    /// ```
    pub fn with_mask(mut self, mask: u32, on: bool) -> Self {
        let b = (mask & 0xFFFF) as u16;
        self.buttons = if on { self.buttons | b } else { self.buttons & !b };
        if mask & LT != 0 {
            self.lt = if on { 255 } else { 0 };
        }
        if mask & RT != 0 {
            self.rt = if on { 255 } else { 0 };
        }
        self
    }
}

/// Formats the [`BUTTONS`] contained in `mask` as `"A+B+LT"`, or `"-"` if none.
pub fn format_mask(mask: u32) -> String {
    let names: Vec<&str> = BUTTONS.iter().filter(|(_, b)| mask & b != 0).map(|(n, _)| *n).collect();
    if names.is_empty() { "-".into() } else { names.join("+") }
}

/// Owner of the gilrs context; reads the connected gamepads.
///
/// gilrs updates its cached gamepad state only while events are drained, so
/// [`Pads::pump`] must be called before each round of [`Pads::state`] calls.
/// The value is not `Send` on every platform and is therefore created and used
/// on the worker thread only.
pub struct Pads {
    gilrs: Gilrs,
}

impl Pads {
    /// Initializes gilrs.
    ///
    /// # Errors
    ///
    /// Returns gilrs' error text if the input backend cannot be initialized.
    pub fn new() -> Result<Self, String> {
        Gilrs::new().map(|gilrs| Self { gilrs }).map_err(|e| e.to_string())
    }

    /// Drains pending events so that the cached gamepad state is current.
    ///
    /// Also advances gilrs' internal frame counter, which keeps its
    /// "changed since last frame" bookkeeping consistent.
    pub fn pump(&mut self) {
        while self.gilrs.next_event().is_some() {}
        self.gilrs.inc();
    }

    /// Identifiers of all currently connected gamepads.
    pub fn ids(&self) -> Vec<GamepadId> {
        self.gilrs.gamepads().map(|(id, _)| id).collect()
    }

    /// Resolves the numeric index shown in the GUI back to a connected
    /// [`GamepadId`]. Returns `None` if that gamepad is not connected.
    pub fn find(&self, index: usize) -> Option<GamepadId> {
        self.gilrs.gamepads().map(|(id, _)| id).find(|&id| usize::from(id) == index)
    }

    /// Waits until a gamepad that is not in `before` appears, up to `timeout`.
    ///
    /// Used right after plugging in the ViGEm virtual controller: the virtual
    /// pad shows up as a regular XInput device, and it must be identified so
    /// that it is never offered as an input source (which would create a
    /// feedback loop).
    pub fn wait_new(&mut self, before: &[GamepadId], timeout: Duration) -> Option<GamepadId> {
        let end = Instant::now() + timeout;
        while Instant::now() < end {
            self.pump();
            if let Some(id) = self.ids().into_iter().find(|id| !before.contains(id)) {
                return Some(id);
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        None
    }

    /// Reads the current state of gamepad `id`.
    ///
    /// Analog values reported by gilrs as floats are scaled back to XInput
    /// ranges (triggers 0–255, sticks ±32767). Returns `None` if the gamepad
    /// is not connected.
    pub fn state(&self, id: GamepadId) -> Option<PadState> {
        let g = self.gilrs.connected_gamepad(id)?;
        let mut buttons = 0u16;
        for (btn, bit) in BUTTON_MAP {
            if g.is_pressed(btn) {
                buttons |= bit;
            }
        }
        let trigger = |btn| {
            g.button_data(btn).map(|d| (d.value().clamp(0.0, 1.0) * 255.0).round() as u8).unwrap_or(0)
        };
        let axis = |a| (g.value(a).clamp(-1.0, 1.0) * 32767.0) as i16;
        Some(PadState {
            buttons,
            lt: trigger(Button::LeftTrigger2),
            rt: trigger(Button::RightTrigger2),
            lx: axis(Axis::LeftStickX),
            ly: axis(Axis::LeftStickY),
            rx: axis(Axis::RightStickX),
            ry: axis(Axis::RightStickY),
        })
    }

    /// Display name and power status of gamepad `id`, or `None` if it is not
    /// connected.
    pub fn info(&self, id: GamepadId) -> Option<(String, PowerInfo)> {
        self.gilrs.connected_gamepad(id).map(|g| (g.name().to_string(), g.power_info()))
    }
}
