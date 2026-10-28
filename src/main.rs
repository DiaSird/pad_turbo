//! # PadTurbo
//!
//! A single-executable gamepad turbo and macro tool for Windows.
//!
//! * **Turbo** — rapid-fires selected buttons at a configurable interval,
//!   either always or only while the button is held.
//! * **Record macro** — records the selected controller (buttons, triggers and
//!   sticks) with timing.
//! * **Play macro** — loops the recorded macro.
//!
//! The selected function is started and stopped with a global toggle (F5 by
//! default, configurable in `config.json`; see [`input::binding`]).
//!
//! With the keyboard output, keys go either to the focused window or to a
//! chosen process, which then also works in the background (see
//! [`output::target`]).
//!
//! ## Architecture
//!
//! The program is a Cargo workspace. This crate is only the GUI and the
//! start-up code; everything else lives in library crates under `crates/`,
//! imported under the short names in the first column:
//!
//! | crate (package)                | modules             | responsibility                                          |
//! |--------------------------------|---------------------|---------------------------------------------------------|
//! | [`i18n`] (`pad_turbo_i18n`)     |                     | JSON translation tables (English / Japanese)            |
//! | [`input`] (`pad_turbo_input`)   | [`input::pad`]      | reading controllers with gilrs, the button/mask model   |
//! |                                | [`input::binding`]  | `config.json` and the start/stop toggle                 |
//! | [`output`] (`pad_turbo_output`) | [`output`]          | ViGEm virtual Xbox 360 controller, keyboard injection   |
//! |                                | [`output::target`]  | posting keys to a chosen process's windows (background) |
//! | [`engine`] (`pad_turbo_engine`) |                     | the 1 ms worker loop: turbo, recording, playback        |
//! | `pad_turbo` (this crate)       | [`ui`]              | the egui window                                         |
//!
//! Dependencies only point downwards in this table: `input` and `i18n` are
//! independent, `output` uses both, `engine` uses all three, and the GUI uses
//! everything.
//!
//! ## Files next to the executable
//!
//! | file            | written by | content                                     |
//! |-----------------|------------|---------------------------------------------|
//! | `config.json`   | user / GUI | toggle binding (created on first launch)    |
//! | `settings.json` | GUI        | language, feature, turbo, output, key table, target |
//! | `macro.json`    | worker     | last recorded macro                         |
//! | `i18n/*.json`   | user       | optional translation overrides              |
//!
//! None of them is required: everything has built-in defaults, and the icon and
//! translations are embedded in the executable.

#![windows_subsystem = "windows"]

mod ui;

use eframe::egui;
use engine::{Macro, Settings, Shared};
use i18n::{Message, Translations};
use input::binding::{Binding, Config, DEFAULT_TOGGLE};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

/// Raw 64×64 RGBA window icon, generated alongside `assets/icon.ico`.
const WINDOW_ICON: &[u8] = include_bytes!("../assets/icon_64.rgba");

/// Directory containing the executable; data files are stored there.
///
/// Falls back to the current directory if the path cannot be determined.
fn exe_dir() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(Path::to_path_buf))
        .unwrap_or_else(|| PathBuf::from("."))
}

/// Reads and deserializes a JSON file, returning `None` on any error.
fn load_json<T: serde::de::DeserializeOwned>(path: &Path) -> Option<T> {
    serde_json::from_str(&std::fs::read_to_string(path).ok()?).ok()
}

/// Loads `config.json` and parses the toggle binding.
///
/// Problems are returned as log messages and replaced by [`DEFAULT_TOGGLE`],
/// so a typo in the file never prevents the program from starting.
fn load_toggle(path: &Path) -> (Binding, Vec<Message>) {
    let mut problems = Vec::new();
    let config = Config::load_or_create(path).unwrap_or_else(|(defaults, error)| {
        problems.push(
            Message::new("log.bad_config")
                .text("error", error)
                .text("key", DEFAULT_TOGGLE),
        );
        defaults
    });
    let binding = Binding::parse(&config.toggle).unwrap_or_else(|token| {
        problems.push(
            Message::new("log.bad_binding")
                .text("token", token)
                .text("key", DEFAULT_TOGGLE),
        );
        Binding::default()
    });
    (binding, problems)
}

fn main() -> eframe::Result {
    let dir = exe_dir();
    let settings_path = dir.join("settings.json");
    let macro_path = dir.join("macro.json");
    let settings: Settings = load_json(&settings_path).unwrap_or_default();
    let macro_data: Option<Macro> = load_json(&macro_path);
    let config_path = dir.join("config.json");
    let (binding, problems) = load_toggle(&config_path);
    let translations = Translations::load(Some(&dir.join("i18n")));

    let mut state = Shared::new(settings, macro_data, binding);
    for p in problems {
        state.log(p);
    }
    let shared = Arc::new(Mutex::new(state));
    let worker = {
        let shared = Arc::clone(&shared);
        std::thread::spawn(move || engine::run(shared, macro_path))
    };

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("PadTurbo")
            .with_inner_size([560.0, 760.0])
            .with_min_inner_size([420.0, 400.0])
            .with_icon(egui::IconData {
                rgba: WINDOW_ICON.to_vec(),
                width: 64,
                height: 64,
            }),
        ..Default::default()
    };
    eframe::run_native(
        "PadTurbo",
        options,
        Box::new(move |cc| {
            Ok(Box::new(ui::App::new(
                cc,
                shared,
                worker,
                settings_path,
                config_path,
                translations,
            )))
        }),
    )
}
