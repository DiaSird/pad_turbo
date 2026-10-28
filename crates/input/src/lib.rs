//! Controller input for PadTurbo.
//!
//! * [`pad`] — reading controllers with gilrs and the button/mask model
//!   shared by every other crate.
//! * [`binding`] — `config.json` and the start/stop toggle (keyboard and
//!   controller buttons).

pub mod binding;
pub mod pad;
