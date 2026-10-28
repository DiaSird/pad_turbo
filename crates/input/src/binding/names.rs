//! Keyboard key and controller button names used in bindings, and the
//! keyboard state they are tested against.

use crate::pad::{self, LT, RT};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::GetAsyncKeyState;

/// Modifier keys, listed first in generated labels.
pub(super) const MODIFIERS: [u8; 3] = [0x11, 0x10, 0x12];

/// Named keys other than letters, digits, `F1`–`F24` and `Num0`–`Num9`,
/// as `(name, virtual-key code)`. The first name of each code is used when
/// generating labels.
pub(super) const NAMED_KEYS: [(&str, u8); 24] = [
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
pub(super) fn capturable_keys() -> impl Iterator<Item = u8> {
    let letters_digits = (b'0'..=b'9').chain(b'A'..=b'Z');
    let function = 0x70..=0x87_u8;
    let numpad = 0x60..=0x69_u8;
    let named = NAMED_KEYS[..21].iter().map(|&(_, vk)| vk);
    letters_digits.chain(function).chain(numpad).chain(named)
}

/// Whether virtual key `vk` is currently down.
pub(super) fn key_down(vk: u8) -> bool {
    // SAFETY: GetAsyncKeyState has no preconditions.
    let state = unsafe { GetAsyncKeyState(vk as i32) };
    state as u16 & 0x8000 != 0
}

/// Display name of virtual key `vk`, the inverse of [`vk_from_name`].
pub(super) fn vk_name(vk: u8) -> Option<String> {
    match vk {
        b'0'..=b'9' | b'A'..=b'Z' => Some((vk as char).to_string()),
        0x70..=0x87 => Some(format!("F{}", vk - 0x70 + 1)),
        0x60..=0x69 => Some(format!("Num{}", vk - 0x60)),
        _ => NAMED_KEYS
            .iter()
            .find(|&&(_, v)| v == vk)
            .map(|(n, _)| n.to_string()),
    }
}

/// `Pad.<name>` names of the controller buttons in `mask`, in
/// [`pad::BUTTONS`] order. D-pad arrows are spelled `UP`/`DOWN`/`LEFT`/`RIGHT`.
pub(super) fn pad_names(mask: u32) -> Vec<&'static str> {
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
pub(super) fn pad_bit(name: &str) -> Option<u32> {
    let bit = match name {
        "LT" => LT,
        "RT" => RT,
        "BACK" | "SELECT" => 0x0020,
        "UP" => 0x0001,
        "DOWN" => 0x0002,
        "LEFT" => 0x0004,
        "RIGHT" => 0x0008,
        _ => {
            return pad::BUTTONS
                .iter()
                .find(|(n, _)| *n == name)
                .map(|(_, b)| *b);
        }
    };
    Some(bit)
}

/// Virtual-key code for a keyboard key name (already upper-case).
pub(super) fn vk_from_name(name: &str) -> Option<u8> {
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
    NAMED_KEYS
        .iter()
        .find(|(n, _)| n.eq_ignore_ascii_case(name))
        .map(|&(_, vk)| vk)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_capturable_key_round_trips() {
        for vk in capturable_keys() {
            let name = vk_name(vk).unwrap();
            assert_eq!(vk_from_name(&name.to_ascii_uppercase()), Some(vk), "{name}");
        }
    }
}
