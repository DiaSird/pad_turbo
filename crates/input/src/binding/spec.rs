//! The [`Binding`] value: parsing, display label and the "held" test.

use super::DEFAULT_TOGGLE;
use super::names::{MODIFIERS, key_down, pad_bit, pad_names, vk_from_name, vk_name};

/// Virtual-key code of `F5`, the key of [`DEFAULT_TOGGLE`].
const VK_F5: u8 = 0x74;

/// A parsed toggle binding: a set of keys and controller buttons that must be
/// held together.
#[derive(Clone, Debug)]
pub struct Binding {
    /// Virtual-key codes that must all be down.
    keys: Vec<u8>,
    /// Logical controller mask (see [`crate::pad`]) whose bits must all be set.
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
        let mut pad = 0_u32;
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
        Ok(Self {
            keys,
            pad,
            label: labels.join("+"),
        })
    }

    /// The binding as written by the user, for display.
    pub fn label(&self) -> &str {
        &self.label
    }

    /// Controller buttons that are part of the binding (logical mask).
    pub const fn pad_mask(&self) -> u32 {
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
    /// The binding for [`DEFAULT_TOGGLE`], built directly so that it cannot
    /// fail; `default_matches_parsed_text` checks it against the parser.
    fn default() -> Self {
        Self {
            keys: vec![VK_F5],
            pad: 0,
            label: DEFAULT_TOGGLE.into(),
        }
    }
}

impl Binding {
    /// Builds a binding from captured inputs (see [`Capture`](super::Capture)).
    ///
    /// Keys come first, modifiers (`Ctrl`, `Shift`, `Alt`) before other keys,
    /// followed by controller buttons in [`crate::pad::BUTTONS`] order, so that the
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
    fn default_matches_parsed_text() {
        let parsed = Binding::parse(DEFAULT_TOGGLE).unwrap();
        let default = Binding::default();
        assert_eq!(default.keys, parsed.keys);
        assert_eq!(default.pad, parsed.pad);
        assert_eq!(default.label(), parsed.label());
    }

    #[test]
    fn pad_only_binding_needs_all_buttons() {
        let b = Binding::parse("Pad.LB+Pad.RB").unwrap();
        assert!(!b.is_held(0x0100));
        assert!(b.is_held(0x0100 | 0x0200 | 0x1000));
    }
}
