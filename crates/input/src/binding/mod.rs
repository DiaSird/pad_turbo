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
//!
//! # Modules
//!
//! | module    | responsibility                                           |
//! |-----------|----------------------------------------------------------|
//! | `config`  | reading and writing `config.json`                        |
//! | `spec`    | the [`Binding`] value: parsing, labels, "is it held?"    |
//! | `names`   | key and button names, keyboard state                     |
//! | `detect`  | turning keyboard/controller samples into events          |

mod config;
mod detect;
mod names;
mod spec;

pub use config::Config;
pub use detect::{Capture, Toggle};
pub use spec::Binding;

/// Binding used when `config.json` is missing or invalid.
pub const DEFAULT_TOGGLE: &str = "F5";
