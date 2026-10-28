//! Turning sampled inputs into events: the start/stop [`Toggle`] edge
//! detector and the [`Capture`] that records a new binding.

use super::Binding;
use super::names::{capturable_keys, key_down};
use crate::pad;

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
    pub const fn new(binding: Binding) -> Self {
        Self {
            binding,
            was_held: true,
        }
    }

    /// The watched binding.
    pub const fn binding(&self) -> &Binding {
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
