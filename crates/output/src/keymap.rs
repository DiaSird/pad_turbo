//! The keyboard output's key table: which key each controller input presses.
//!
//! Games and emulators read different keys, so the table is part of the user
//! settings. It starts from a [`KeyPreset`] and every entry can be changed
//! or left unassigned. Several inputs may share one key; the key is then held
//! while any of them is.
//!
//! In `settings.json` the table is a map from control id ([`CONTROLS`]) to a
//! key name as written in bindings (see [`input::binding`]), with `""` for
//! "no key". Missing or unknown entries take the [`KeyPreset::Standard`]
//! value, so older files keep working.

use input::binding::{key_code, key_name};
use input::pad;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Number of mappable controller inputs.
const COUNT: usize = 24;

/// Controller inputs that can be mapped to a key, as `(logical bit, control
/// id)`, in GUI order.
///
/// Ids that have a `control.<id>` entry in the translation files (stick and
/// D-pad directions) are translated in the GUI; the others (A, B, LB, ...) are
/// shown as-is.
pub const CONTROLS: [(u32, &str); COUNT] = [
    (0x1000, "A"),
    (0x2000, "B"),
    (0x4000, "X"),
    (0x8000, "Y"),
    (0x0100, "LB"),
    (0x0200, "RB"),
    (pad::LT, "LT"),
    (pad::RT, "RT"),
    (0x0010, "START"),
    (0x0020, "SELECT"),
    (0x0040, "LS"),
    (0x0080, "RS"),
    (0x0001, "UP"),
    (0x0002, "DOWN"),
    (0x0004, "LEFT"),
    (0x0008, "RIGHT"),
    (pad::LS_UP, "LS_UP"),
    (pad::LS_DOWN, "LS_DOWN"),
    (pad::LS_LEFT, "LS_LEFT"),
    (pad::LS_RIGHT, "LS_RIGHT"),
    (pad::RS_UP, "RS_UP"),
    (pad::RS_DOWN, "RS_DOWN"),
    (pad::RS_LEFT, "RS_LEFT"),
    (pad::RS_RIGHT, "RS_RIGHT"),
];

// Virtual-key codes used by the presets.
const VK_BACK: u8 = 0x08;
const VK_RETURN: u8 = 0x0D;
const VK_SHIFT: u8 = 0x10;
const VK_SPACE: u8 = 0x20;
const VK_LEFT: u8 = 0x25;
const VK_UP: u8 = 0x26;
const VK_RIGHT: u8 = 0x27;
const VK_DOWN: u8 = 0x28;

/// A ready-made key table.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum KeyPreset {
    /// PadTurbo's own layout: face buttons on Z/X/C/V, sticks on WASD/IJKL.
    Standard,
    /// The default keyboard layout of Yakumo (the MHP3rd PC port). The face
    /// buttons follow its gamepad layout with confirm on east: A → × (Space),
    /// B → ○ (F), X → □ (E). Y stays unassigned because Yakumo's △ is on the
    /// mouse; the triggers press the shoulders' keys, as its gamepad does.
    Yakumo,
}

impl KeyPreset {
    /// Every preset, in GUI order.
    pub const ALL: [Self; 2] = [Self::Standard, Self::Yakumo];

    /// Translation key of the preset's display name.
    pub const fn key(self) -> &'static str {
        match self {
            Self::Standard => "keymap.standard",
            Self::Yakumo => "keymap.yakumo",
        }
    }

    /// Keys of the preset, indexed like [`CONTROLS`].
    const fn keys(self) -> [Option<u8>; COUNT] {
        let face = match self {
            Self::Standard => [Some(b'Z'), Some(b'X'), Some(b'C'), Some(b'V')],
            Self::Yakumo => [Some(VK_SPACE), Some(b'F'), Some(b'E'), None],
        };
        let middle = match self {
            Self::Standard => [
                Some(b'Q'),
                Some(b'E'),
                Some(b'1'),
                Some(b'3'),
                Some(VK_RETURN),
                Some(VK_BACK),
                Some(b'F'),
                Some(b'G'),
            ],
            Self::Yakumo => [
                Some(b'Q'),
                Some(VK_SHIFT),
                Some(b'Q'),
                Some(VK_SHIFT),
                Some(VK_RETURN),
                Some(VK_BACK),
                None,
                None,
            ],
        };
        [
            face[0],
            face[1],
            face[2],
            face[3],
            middle[0],
            middle[1],
            middle[2],
            middle[3],
            middle[4],
            middle[5],
            middle[6],
            middle[7],
            Some(VK_UP),
            Some(VK_DOWN),
            Some(VK_LEFT),
            Some(VK_RIGHT),
            Some(b'W'),
            Some(b'S'),
            Some(b'A'),
            Some(b'D'),
            Some(b'I'),
            Some(b'K'),
            Some(b'J'),
            Some(b'L'),
        ]
    }
}

/// The key pressed by each entry of [`CONTROLS`], or `None` for no key.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(from = "BTreeMap<String, String>", into = "BTreeMap<String, String>")]
pub struct KeyMap {
    /// Virtual-key codes, indexed like [`CONTROLS`].
    keys: [Option<u8>; COUNT],
}

impl KeyMap {
    /// The table of preset `p`.
    pub const fn preset(p: KeyPreset) -> Self {
        Self { keys: p.keys() }
    }

    /// Whether the table is exactly preset `p`.
    pub fn is_preset(&self, p: KeyPreset) -> bool {
        *self == Self::preset(p)
    }

    /// Key of entry `i` of [`CONTROLS`]; `None` when unassigned or out of
    /// range.
    pub fn key(&self, i: usize) -> Option<u8> {
        self.keys.get(i).copied().flatten()
    }

    /// Assigns `vk` (or no key) to entry `i` of [`CONTROLS`]. Out-of-range
    /// indices are ignored.
    pub fn set(&mut self, i: usize, vk: Option<u8>) {
        if let Some(slot) = self.keys.get_mut(i) {
            *slot = vk;
        }
    }

    /// Keys to hold for logical mask `mask`, sorted and without duplicates.
    pub fn pressed(&self, mask: u32) -> Vec<u8> {
        let mut keys: Vec<u8> = CONTROLS
            .iter()
            .zip(self.keys)
            .filter(|((bit, _), _)| mask & bit != 0)
            .filter_map(|(_, vk)| vk)
            .collect();
        keys.sort_unstable();
        keys.dedup();
        keys
    }
}

impl Default for KeyMap {
    fn default() -> Self {
        Self::preset(KeyPreset::Standard)
    }
}

impl From<BTreeMap<String, String>> for KeyMap {
    /// Reads the `settings.json` form; see the [module docs](self).
    fn from(map: BTreeMap<String, String>) -> Self {
        let mut keymap = Self::default();
        for (i, (_, control)) in CONTROLS.iter().enumerate() {
            let Some(name) = map.get(*control) else { continue };
            if name.trim().is_empty() {
                keymap.set(i, None);
            } else if let Some(vk) = key_code(name) {
                keymap.set(i, Some(vk));
            }
        }
        keymap
    }
}

impl From<KeyMap> for BTreeMap<String, String> {
    /// Writes the `settings.json` form; see the [module docs](self).
    fn from(keymap: KeyMap) -> Self {
        CONTROLS
            .iter()
            .zip(keymap.keys)
            .map(|((_, control), vk)| ((*control).to_string(), vk.and_then(key_name).unwrap_or_default()))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn standard_preset_names_resolve() {
        let m = KeyMap::default();
        assert_eq!(m.key(0), Some(b'Z'));
        assert_eq!(m.key(9), Some(VK_BACK));
        assert_eq!(m.key(COUNT), None);
    }

    #[test]
    fn shared_keys_are_pressed_once() {
        let m = KeyMap::preset(KeyPreset::Yakumo);
        // LB and LT both press Q; A presses Space.
        assert_eq!(m.pressed(0x0100 | pad::LT | 0x1000), vec![VK_SPACE, b'Q']);
        // Y has no key in the Yakumo preset.
        assert!(m.pressed(0x8000).is_empty());
    }

    #[test]
    fn settings_form_round_trips() {
        let mut m = KeyMap::preset(KeyPreset::Yakumo);
        m.set(3, Some(b'T'));
        let map = BTreeMap::from(m.clone());
        assert_eq!(map.get("A").map(String::as_str), Some("Space"));
        assert_eq!(map.get("LS").map(String::as_str), Some(""));
        assert_eq!(KeyMap::from(map), m);
    }

    #[test]
    fn missing_or_unknown_entries_keep_the_standard_key() {
        let map = BTreeMap::from([("A".to_string(), "Nope".to_string()), ("B".to_string(), "space".to_string())]);
        let m = KeyMap::from(map);
        assert_eq!(m.key(0), Some(b'Z'));
        assert_eq!(m.key(1), Some(VK_SPACE));
        assert_eq!(m.key(2), Some(b'C'));
    }
}
