//! The configurable start/stop toggle and `config.json`.
//!
//! The selected function is started and stopped with a single global toggle,
//! [`F5`](DEFAULT_TOGGLE) by default. The binding is read from `config.json`
//! next to the executable; the file is created with the default value on first
//! launch so that users have something to edit:
//!
//! ```json
//! { "toggle": "F5" }
//! ```
//!
//! # Binding syntax
//!
//! A binding is one or more inputs joined by `+`; the toggle fires when all of
//! them are held at the same time (on the transition to "all held"). Names are
//! case-insensitive.
//!
//! * Keyboard keys: `F1`–`F24`, `A`–`Z`, `0`–`9`, `Num0`–`Num9`, `Space`,
//!   `Enter`, `Tab`, `Esc`, `Backspace`, `Insert`, `Delete`, `Home`, `End`,
//!   `PageUp`, `PageDown`, `Up`, `Down`, `Left`, `Right`, `Pause`,
//!   `ScrollLock`, `CapsLock`, `Shift`, `Ctrl`, `Alt`.
//! * Controller buttons, prefixed with `Pad.`: `Pad.A`, `Pad.B`, `Pad.X`,
//!   `Pad.Y`, `Pad.LB`, `Pad.RB`, `Pad.LT`, `Pad.RT`, `Pad.START`,
//!   `Pad.SELECT` (alias `Pad.BACK`), `Pad.LS`, `Pad.RS`, `Pad.UP`,
//!   `Pad.DOWN`, `Pad.LEFT`, `Pad.RIGHT`. They are read from the controller
//!   selected in the GUI.
//!
//! Examples: `"F5"`, `"Ctrl+F5"`, `"Pad.BACK"`, `"Pad.LB+Pad.RB+Pad.A"`.
//!
//! Keyboard keys are polled with `GetAsyncKeyState`, so the toggle works while a
//! game has focus. The keys are not swallowed; the game receives them too.
//!
//! # Changing the binding from the GUI
//!
//! The GUI can replace the binding at run time, either from text (parsed with
//! [`Binding::parse`]) or by recording what the user presses ([`Capture`]).
//! The new binding takes effect immediately and is written back to
//! `config.json` with [`Config::save`].

use crate::pad::{self, LT, RT};
use serde::{Deserialize, Serialize};
use std::path::Path;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::GetAsyncKeyState;

/// Binding used when `config.json` is missing or invalid.
pub const DEFAULT_TOGGLE: &str = "F5";

/// Contents of `config.json` (user-edited, read once at start-up).
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    /// Start/stop toggle binding; see the [module docs](self) for the syntax.
    pub toggle: String,
}

impl Default for Config {
    fn default() -> Self {
        Self { toggle: DEFAULT_TOGGLE.into() }
    }
}

impl Config {
    /// Writes the configuration to `path` as pretty-printed JSON.
    ///
    /// # Errors
    ///
    /// Returns the I/O or serialization error text.
    pub fn save(&self, path: &Path) -> Result<(), String> {
        let json = serde_json::to_string_pretty(self).map_err(|e| e.to_string())?;
        std::fs::write(path, json).map_err(|e| e.to_string())
    }

    /// Reads `path`, creating it with default values if it does not exist.
    ///
    /// An unreadable or malformed file is left untouched (it is the user's
    /// file) and the defaults are used instead.
    ///
    /// # Errors
    ///
    /// Returns the defaults together with a description of the problem when
    /// the existing file cannot be parsed, so the caller can report it.
    pub fn load_or_create(path: &Path) -> Result<Self, (Self, String)> {
        match std::fs::read_to_string(path) {
            Ok(text) => serde_json::from_str(&text).map_err(|e| (Self::default(), e.to_string())),
            Err(_) => {
                let config = Self::default();
                let _ = config.save(path);
                Ok(config)
            }
        }
    }
}

/// A parsed toggle binding: a set of keys and controller buttons that must be
/// held together.
#[derive(Clone, Debug)]
pub struct Binding {
    /// Virtual-key codes that must all be down.
    keys: Vec<u8>,
    /// Logical controller mask (see [`pad`]) whose bits must all be set.
    pad: u32,
    /// Normalized text for display, e.g. `"Ctrl+F5"`.
    label: String,
}

impl Binding {
    /// Parses a binding such as `"Ctrl+F5"` or `"Pad.LB+Pad.RB"`.
    ///
    /// # Errors
    ///
    /// Returns the first token that is not a known key or button name, or an
    /// empty string if `spec` contains no tokens at all.
    pub fn parse(spec: &str) -> Result<Self, String> {
        let mut keys = Vec::new();
        let mut pad = 0u32;
        let mut labels = Vec::new();
        for token in spec.split('+').map(str::trim).filter(|t| !t.is_empty()) {
            let upper = token.to_ascii_uppercase();
            if let Some(name) = upper.strip_prefix("PAD.") {
                pad |= pad_bit(name).ok_or_else(|| token.to_string())?;
            } else {
                keys.push(vk_from_name(&upper).ok_or_else(|| token.to_string())?);
            }
            labels.push(token.to_string());
        }
        if labels.is_empty() {
            return Err(String::new());
        }
        Ok(Self { keys, pad, label: labels.join("+") })
    }

    /// The binding as written by the user, for display.
    pub fn label(&self) -> &str {
        &self.label
    }

    /// Controller buttons that are part of the binding (logical mask).
    pub fn pad_mask(&self) -> u32 {
        self.pad
    }

    /// Whether every input of the binding is currently held.
    ///
    /// `pad_mask` is the logical mask of the selected controller (0 if none).
    pub fn is_held(&self, pad_mask: u32) -> bool {
        self.keys.iter().all(|&vk| key_down(vk)) && pad_mask & self.pad == self.pad
    }
}

impl Default for Binding {
    fn default() -> Self {
        Self::parse(DEFAULT_TOGGLE).expect("default binding is valid")
    }
}

impl Binding {
    /// Builds a binding from captured inputs (see [`Capture`]).
    ///
    /// Keys come first, modifiers (`Ctrl`, `Shift`, `Alt`) before other keys,
    /// followed by controller buttons in [`pad::BUTTONS`] order, so that the
    /// label reads naturally, e.g. `"Ctrl+F5"` or `"Pad.LB+Pad.RB"`. Returns
    /// `None` if nothing was captured.
    pub fn from_inputs(keys: &[u8], pad: u32) -> Option<Self> {
        let mut keys = keys.to_vec();
        keys.sort_by_key(|vk| !MODIFIERS.contains(vk));
        let mut names: Vec<String> = keys.iter().filter_map(|&vk| vk_name(vk)).collect();
        names.extend(pad_names(pad).into_iter().map(|n| format!("Pad.{n}")));
        Self::parse(&names.join("+")).ok()
    }
}

/// Edge detector that turns a held [`Binding`] into single toggle events.
pub struct Toggle {
    binding: Binding,
    was_held: bool,
}

impl Toggle {
    /// Creates a detector.
    ///
    /// The binding is considered *held* initially, so it only fires after it
    /// has been released once. This prevents an immediate toggle when the
    /// binding is replaced while its inputs are still down.
    pub fn new(binding: Binding) -> Self {
        Self { binding, was_held: true }
    }

    /// The watched binding.
    pub fn binding(&self) -> &Binding {
        &self.binding
    }

    /// Returns `true` exactly once each time the binding becomes fully held.
    pub fn poll(&mut self, pad_mask: u32) -> bool {
        let held = self.binding.is_held(pad_mask);
        let fired = held && !self.was_held;
        self.was_held = held;
        fired
    }
}

/// Records a new binding from what the user presses.
///
/// Every input held at the same time is accumulated; when everything has been
/// released again, the union becomes the new binding. Pressing `Ctrl`, then
/// `F5`, then releasing both therefore yields `"Ctrl+F5"`.
#[derive(Default)]
pub struct Capture {
    keys: Vec<u8>,
    pad: u32,
}

impl Capture {
    /// Starts an empty capture.
    pub fn new() -> Self {
        Self::default()
    }

    /// Samples the keyboard and `pad_mask` (logical mask of the selected
    /// controller). Returns the finished binding once all inputs are released
    /// after at least one was pressed.
    pub fn poll(&mut self, pad_mask: u32) -> Option<Binding> {
        let held_keys: Vec<u8> = capturable_keys().filter(|&vk| key_down(vk)).collect();
        let held_pad = pad_mask & pad::SELECTABLE;
        if !held_keys.is_empty() || held_pad != 0 {
            for vk in held_keys {
                if !self.keys.contains(&vk) {
                    self.keys.push(vk);
                }
            }
            self.pad |= held_pad;
            return None;
        }
        if self.keys.is_empty() && self.pad == 0 {
            return None;
        }
        let result = Binding::from_inputs(&self.keys, self.pad);
        *self = Self::default();
        result
    }
}

/// Modifier keys, listed first in generated labels.
const MODIFIERS: [u8; 3] = [0x11, 0x10, 0x12];

/// Named keys other than letters, digits, `F1`–`F24` and `Num0`–`Num9`,
/// as `(name, virtual-key code)`. The first name of each code is used when
/// generating labels.
const NAMED_KEYS: [(&str, u8); 24] = [
    ("Ctrl", 0x11),
    ("Shift", 0x10),
    ("Alt", 0x12),
    ("Space", 0x20),
    ("Enter", 0x0D),
    ("Tab", 0x09),
    ("Esc", 0x1B),
    ("Backspace", 0x08),
    ("Insert", 0x2D),
    ("Delete", 0x2E),
    ("Home", 0x24),
    ("End", 0x23),
    ("PageUp", 0x21),
    ("PageDown", 0x22),
    ("Up", 0x26),
    ("Down", 0x28),
    ("Left", 0x25),
    ("Right", 0x27),
    ("Pause", 0x13),
    ("ScrollLock", 0x91),
    ("CapsLock", 0x14),
    // Aliases accepted when parsing only.
    ("Return", 0x0D),
    ("Escape", 0x1B),
    ("Control", 0x11),
];

/// Every virtual-key code that has a name, i.e. that a binding can contain.
///
/// Mouse buttons and the left/right-specific modifier codes are excluded on
/// purpose: clicking the GUI must not end up in a captured binding, and the
/// generic `Ctrl`/`Shift`/`Alt` codes already cover both sides.
fn capturable_keys() -> impl Iterator<Item = u8> {
    let letters_digits = (b'0'..=b'9').chain(b'A'..=b'Z');
    let function = 0x70..=0x87u8;
    let numpad = 0x60..=0x69u8;
    let named = NAMED_KEYS[..21].iter().map(|&(_, vk)| vk);
    letters_digits.chain(function).chain(numpad).chain(named)
}

/// Whether virtual key `vk` is currently down.
fn key_down(vk: u8) -> bool {
    // SAFETY: GetAsyncKeyState has no preconditions.
    let state = unsafe { GetAsyncKeyState(vk as i32) };
    state as u16 & 0x8000 != 0
}

/// Display name of virtual key `vk`, the inverse of [`vk_from_name`].
fn vk_name(vk: u8) -> Option<String> {
    match vk {
        b'0'..=b'9' | b'A'..=b'Z' => Some((vk as char).to_string()),
        0x70..=0x87 => Some(format!("F{}", vk - 0x70 + 1)),
        0x60..=0x69 => Some(format!("Num{}", vk - 0x60)),
        _ => NAMED_KEYS.iter().find(|&&(_, v)| v == vk).map(|(n, _)| n.to_string()),
    }
}

/// `Pad.<name>` names of the controller buttons in `mask`, in
/// [`pad::BUTTONS`] order. D-pad arrows are spelled `UP`/`DOWN`/`LEFT`/`RIGHT`.
fn pad_names(mask: u32) -> Vec<&'static str> {
    pad::BUTTONS
        .iter()
        .filter(|(_, bit)| mask & bit != 0)
        .map(|&(name, bit)| match bit {
            0x0001 => "UP",
            0x0002 => "DOWN",
            0x0004 => "LEFT",
            0x0008 => "RIGHT",
            _ => name,
        })
        .collect()
}

/// Logical controller bit for a `Pad.<name>` token (name already upper-case).
fn pad_bit(name: &str) -> Option<u32> {
    let bit = match name {
        "LT" => LT,
        "RT" => RT,
        "BACK" | "SELECT" => 0x0020,
        "UP" => 0x0001,
        "DOWN" => 0x0002,
        "LEFT" => 0x0004,
        "RIGHT" => 0x0008,
        _ => return pad::BUTTONS.iter().find(|(n, _)| *n == name).map(|(_, b)| *b),
    };
    Some(bit)
}

/// Virtual-key code for a keyboard key name (already upper-case).
fn vk_from_name(name: &str) -> Option<u8> {
    let bytes = name.as_bytes();
    // Single letters and digits map directly to their ASCII code.
    if bytes.len() == 1 && (bytes[0].is_ascii_uppercase() || bytes[0].is_ascii_digit()) {
        return Some(bytes[0]);
    }
    if let Some(n) = name.strip_prefix('F').and_then(|n| n.parse::<u8>().ok()) {
        return (1..=24).contains(&n).then_some(0x70 + n - 1);
    }
    if let Some(n) = name.strip_prefix("NUM").and_then(|n| n.parse::<u8>().ok()) {
        return (n <= 9).then_some(0x60 + n);
    }
    NAMED_KEYS.iter().find(|(n, _)| n.eq_ignore_ascii_case(name)).map(|&(_, vk)| vk)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_keys_and_pad_buttons() {
        let b = Binding::parse("ctrl + F5").unwrap();
        assert_eq!(b.keys, vec![0x11, 0x74]);
        assert_eq!(b.pad, 0);

        let b = Binding::parse("Pad.LB+Pad.rb+Pad.Back").unwrap();
        assert!(b.keys.is_empty());
        assert_eq!(b.pad, 0x0100 | 0x0200 | 0x0020);
        assert_eq!(b.label(), "Pad.LB+Pad.rb+Pad.Back");
    }

    #[test]
    fn rejects_unknown_names() {
        assert_eq!(Binding::parse("F5+Foo").unwrap_err(), "Foo");
        assert_eq!(Binding::parse("Pad.Z").unwrap_err(), "Pad.Z");
        assert!(Binding::parse(" + ").is_err());
        assert!(Binding::parse("F25").is_err());
    }

    #[test]
    fn captured_inputs_produce_readable_labels() {
        // F5 pressed before Ctrl still yields the modifier first.
        let b = Binding::from_inputs(&[0x74, 0x11], 0).unwrap();
        assert_eq!(b.label(), "Ctrl+F5");
        let b = Binding::from_inputs(&[], 0x0200 | 0x0100 | 0x0001).unwrap();
        assert_eq!(b.label(), "Pad.LB+Pad.RB+Pad.UP");
        assert!(Binding::from_inputs(&[], 0).is_none());
    }

    #[test]
    fn every_capturable_key_round_trips() {
        for vk in capturable_keys() {
            let name = vk_name(vk).unwrap();
            assert_eq!(vk_from_name(&name.to_ascii_uppercase()), Some(vk), "{name}");
        }
    }

    #[test]
    fn pad_only_binding_needs_all_buttons() {
        let b = Binding::parse("Pad.LB+Pad.RB").unwrap();
        assert!(!b.is_held(0x0100));
        assert!(b.is_held(0x0100 | 0x0200 | 0x1000));
    }
}
