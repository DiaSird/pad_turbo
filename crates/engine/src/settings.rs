//! User settings and the enums they are made of.

use serde::{Deserialize, Serialize};

/// The three functions the user can choose from. Only one runs at a time.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum Feature {
    /// Rapid-fire the selected buttons.
    Turbo,
    /// Record the physical controller into a macro.
    Record,
    /// Loop the recorded macro.
    Play,
}

impl Feature {
    /// Every feature, in GUI order.
    pub const ALL: [Self; 3] = [Self::Turbo, Self::Record, Self::Play];

    /// Translation key of the feature's display name.
    pub const fn key(self) -> &'static str {
        match self {
            Self::Turbo => "feature.turbo",
            Self::Record => "feature.record",
            Self::Play => "feature.play",
        }
    }
}

/// When turbo is applied.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum TurboMode {
    /// The selected buttons are pulsed continuously while turbo runs.
    Always,
    /// Each selected button is pulsed only while it is physically held.
    Hold,
}

/// Which output device the user wants.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum OutputPref {
    /// Virtual Xbox 360 controller through ViGEmBus.
    ViGEm,
    /// Keyboard injection.
    Keyboard,
}

/// Where the keyboard output delivers its keys.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum TargetMode {
    /// Whichever window has focus (`SendInput`).
    Foreground,
    /// The windows of the chosen process ([`Settings::target_exe`] /
    /// [`Settings::target_pid`]), even in the background.
    Process,
}

/// User settings, persisted to `settings.json` next to the executable.
///
/// `#[serde(default)]` lets older or partial files load: missing fields take
/// their [`Default`] value.
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// GUI language.
    pub language: i18n::Language,
    /// Function selected in the GUI; the toggle starts and stops this one.
    pub feature: Feature,
    /// Logical mask of the buttons to turbo (see [`input::pad::BUTTONS`]).
    pub turbo_mask: u32,
    /// Turbo period in milliseconds (one press + one release).
    pub interval_ms: u32,
    /// Turbo behavior.
    pub turbo_mode: TurboMode,
    /// Requested output device.
    pub output: OutputPref,
    /// Wait [`COUNTDOWN`](crate::COUNTDOWN) after pressing Start in the GUI.
    pub countdown: bool,
    /// Destination of the keyboard output.
    pub target: TargetMode,
    /// Executable file name used with [`TargetMode::Process`].
    pub target_exe: String,
    /// Which key each controller input presses with the keyboard output.
    pub keymap: output::KeyMap,
    /// Specific process of [`Settings::target_exe`] chosen in the GUI.
    /// Not persisted: process ids do not survive a restart, so after one every
    /// process with that executable name is targeted.
    #[serde(skip)]
    pub target_pid: Option<u32>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            language: i18n::Language::English,
            feature: Feature::Turbo,
            turbo_mask: 0,
            interval_ms: 1000,
            turbo_mode: TurboMode::Always,
            output: OutputPref::ViGEm,
            countdown: true,
            target: TargetMode::Foreground,
            target_exe: String::new(),
            keymap: output::KeyMap::default(),
            target_pid: None,
        }
    }
}

impl Settings {
    /// The executable name and optional process id keys should be posted
    /// to, or `None` for the focused window (also when no process has been
    /// chosen yet).
    pub fn target_spec(&self) -> Option<(&str, Option<u32>)> {
        (self.target == TargetMode::Process && !self.target_exe.is_empty())
            .then_some((self.target_exe.as_str(), self.target_pid))
    }
}

/// How long each turbo press is held, for a given period.
///
/// Half the period, clamped to 10–100 ms: long enough for games that poll at
/// 60 Hz to notice the press, short enough to leave a clear release gap even
/// at fast rates.
pub fn press_ms(interval_ms: u32) -> u32 {
    (interval_ms / 2).clamp(10, 100)
}

#[cfg(test)]
mod tests {
    use super::*;
    use output::{KeyMap, KeyPreset};

    #[test]
    fn keymap_survives_settings_json() {
        let mut s = Settings { keymap: KeyMap::preset(KeyPreset::Yakumo), ..Settings::default() };
        s.keymap.set(3, Some(b'T'));
        let json = serde_json::to_string_pretty(&s).unwrap();
        assert!(json.contains("\"A\": \"Space\""), "{json}");
        let back: Settings = serde_json::from_str(&json).unwrap();
        assert_eq!(back.keymap, s.keymap);
    }

    #[test]
    fn settings_without_keymap_use_the_standard_table() {
        let s: Settings = serde_json::from_str(r#"{ "output": "Keyboard" }"#).unwrap();
        assert!(s.keymap.is_preset(KeyPreset::Standard));
    }
}
