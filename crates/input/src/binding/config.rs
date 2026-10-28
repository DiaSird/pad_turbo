//! `config.json`: the user-editable file holding the toggle binding.

use super::DEFAULT_TOGGLE;
use serde::{Deserialize, Serialize};
use std::path::Path;

/// Contents of `config.json` (user-edited, read once at start-up).
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    /// Start/stop toggle binding; see the [module docs](super) for the syntax.
    pub toggle: String,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            toggle: DEFAULT_TOGGLE.into(),
        }
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
        if let Ok(text) = std::fs::read_to_string(path) { serde_json::from_str(&text).map_err(|e| (Self::default(), e.to_string())) } else {
            let config = Self::default();
            let _ = config.save(path);
            Ok(config)
        }
    }
}
