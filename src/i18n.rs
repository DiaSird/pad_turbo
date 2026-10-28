//! User-interface translations backed by JSON files.
//!
//! Every string shown in the GUI is looked up by a dotted key such as
//! `"run.start"`. The tables for all supported languages are compiled into the
//! executable from `i18n/<code>.json`, so the program stays a single
//! self-contained `.exe`.
//!
//! # Overriding translations
//!
//! If a file named `i18n/<code>.json` exists next to the executable, its entries
//! are merged over the embedded table for that language at start-up. This lets
//! users fix or tweak wording without rebuilding. Keys missing from the override
//! keep their embedded value.
//!
//! # Placeholders
//!
//! Values may contain named placeholders written as `{name}`. They are filled in
//! by [`Tr::fmt`] or, for messages produced by the worker thread, by
//! [`Tr::msg`].
//!
//! # Fallback order
//!
//! A lookup tries the selected language first, then English, and finally
//! returns the key itself so that a missing entry is visible instead of blank.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;

/// Embedded English table (the reference set of keys).
const EN_JSON: &str = include_str!("../i18n/en.json");
/// Embedded Japanese table.
const JA_JSON: &str = include_str!("../i18n/ja.json");

/// A language the user interface can be displayed in.
///
/// The discriminant is used as an index into [`Translations`], so variants must
/// stay in the same order as [`Language::ALL`].
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
pub enum Language {
    /// English (default).
    #[default]
    English,
    /// Japanese.
    Japanese,
}

impl Language {
    /// Every supported language, in display order.
    pub const ALL: [Language; 2] = [Language::English, Language::Japanese];

    /// File-name code of the language, e.g. `"en"` for `i18n/en.json`.
    pub fn code(self) -> &'static str {
        match self {
            Language::English => "en",
            Language::Japanese => "ja",
        }
    }

    /// Translation JSON compiled into the executable for this language.
    fn embedded(self) -> &'static str {
        match self {
            Language::English => EN_JSON,
            Language::Japanese => JA_JSON,
        }
    }
}

/// A key → text table per [`Language`].
pub struct Translations {
    tables: Vec<HashMap<String, String>>,
}

impl Translations {
    /// Loads the embedded tables and merges optional override files.
    ///
    /// `override_dir` is the directory searched for `<code>.json` files,
    /// normally `<exe dir>/i18n`. Override files that are missing or contain
    /// invalid JSON are ignored silently, because the embedded table is always a
    /// complete fallback.
    ///
    /// # Panics
    ///
    /// Panics if an embedded table is not valid JSON. The tables are fixed at
    /// compile time and covered by the unit tests below, so this indicates a
    /// build mistake rather than a runtime condition.
    pub fn load(override_dir: Option<&Path>) -> Self {
        let tables = Language::ALL
            .iter()
            .map(|&lang| {
                let mut table: HashMap<String, String> = serde_json::from_str(lang.embedded())
                    .unwrap_or_else(|e| panic!("embedded {}.json is invalid: {e}", lang.code()));
                if let Some(dir) = override_dir {
                    let path = dir.join(format!("{}.json", lang.code()));
                    let extra = std::fs::read_to_string(path)
                        .ok()
                        .and_then(|text| serde_json::from_str::<HashMap<String, String>>(&text).ok());
                    if let Some(extra) = extra {
                        table.extend(extra);
                    }
                }
                table
            })
            .collect();
        Self { tables }
    }

    /// Looks up `key` in `lang`, falling back to English.
    ///
    /// Returns `None` only when neither table contains the key.
    pub fn get(&self, lang: Language, key: &str) -> Option<&str> {
        self.tables[lang as usize]
            .get(key)
            .or_else(|| self.tables[Language::English as usize].get(key))
            .map(String::as_str)
    }

    /// Creates a translator bound to `lang`.
    pub fn tr(&self, lang: Language) -> Tr<'_> {
        Tr { translations: self, lang }
    }
}

/// A translator for one language, borrowed from [`Translations`].
///
/// This is what the GUI code passes around while drawing a frame.
#[derive(Clone, Copy)]
pub struct Tr<'a> {
    translations: &'a Translations,
    /// The language this translator renders.
    pub lang: Language,
}

impl<'a> Tr<'a> {
    /// Returns the text for `key`, or the key itself when it is unknown.
    pub fn t(&self, key: &'a str) -> &'a str {
        self.translations.get(self.lang, key).unwrap_or(key)
    }

    /// Returns the text for `key`, or `None` when it is unknown.
    ///
    /// Useful for optional labels where a sensible default exists, such as the
    /// `control.*` names that only some buttons define.
    pub fn get(&self, key: &str) -> Option<&'a str> {
        self.translations.get(self.lang, key)
    }

    /// Returns the text for `key` with each `{name}` placeholder replaced.
    ///
    /// ```ignore
    /// tr.fmt("turbo.selected", &[("buttons", "A+B".into())]);
    /// // English: "Selected: A+B"
    /// ```
    pub fn fmt(&self, key: &'a str, args: &[(&str, String)]) -> String {
        fill(self.t(key), args.iter().map(|(n, v)| (*n, v.as_str())))
    }

    /// Renders a [`Message`] created by non-GUI code.
    ///
    /// [`Arg::Key`] arguments are translated before being substituted, so a
    /// message can refer to e.g. a feature name in the current language.
    pub fn msg(&self, m: &Message) -> String {
        let args: Vec<(&str, String)> = m
            .args
            .iter()
            .map(|(name, arg)| {
                let value = match arg {
                    Arg::Text(s) => s.clone(),
                    Arg::Key(k) => self.t(k).to_string(),
                    Arg::Msg(inner) => self.msg(inner),
                };
                (*name, value)
            })
            .collect();
        fill(self.t(m.key), args.iter().map(|(n, v)| (*n, v.as_str())))
    }
}

/// Replaces every `{name}` in `template` with the matching value.
fn fill<'s>(template: &str, args: impl Iterator<Item = (&'s str, &'s str)>) -> String {
    let mut out = template.to_string();
    for (name, value) in args {
        out = out.replace(&format!("{{{name}}}"), value);
    }
    out
}

/// A placeholder value inside a [`Message`].
#[derive(Clone, Debug)]
pub enum Arg {
    /// Literal text inserted as-is (numbers, file paths, OS error text, ...).
    Text(String),
    /// A translation key whose text is inserted in the display language.
    Key(&'static str),
    /// A nested message, rendered recursively.
    Msg(Box<Message>),
}

/// A language-independent message produced outside the GUI.
///
/// The worker thread cannot know which language the user will pick later, so it
/// stores the translation key plus its arguments and the GUI renders the text
/// with [`Tr::msg`] every frame. Switching the language therefore also
/// re-translates log lines that were produced earlier.
#[derive(Clone, Debug)]
pub struct Message {
    /// Translation key of the template.
    pub key: &'static str,
    /// Placeholder values, by name.
    pub args: Vec<(&'static str, Arg)>,
}

impl Message {
    /// Creates a message without arguments.
    pub fn new(key: &'static str) -> Self {
        Self { key, args: Vec::new() }
    }

    /// Adds a literal text argument.
    pub fn text(mut self, name: &'static str, value: impl ToString) -> Self {
        self.args.push((name, Arg::Text(value.to_string())));
        self
    }

    /// Adds an argument that is itself a translation key.
    pub fn key(mut self, name: &'static str, key: &'static str) -> Self {
        self.args.push((name, Arg::Key(key)));
        self
    }

    /// Adds an argument that is a nested message.
    pub fn msg(mut self, name: &'static str, inner: Message) -> Self {
        self.args.push((name, Arg::Msg(Box::new(inner))));
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn keys(json: &str) -> Vec<String> {
        let map: HashMap<String, String> = serde_json::from_str(json).unwrap();
        let mut keys: Vec<String> = map.into_keys().collect();
        keys.sort();
        keys
    }

    /// Every language must define exactly the same keys as English.
    #[test]
    fn tables_have_the_same_keys() {
        let en = keys(EN_JSON);
        for lang in Language::ALL {
            assert_eq!(keys(lang.embedded()), en, "key mismatch in {}.json", lang.code());
        }
    }

    #[test]
    fn placeholders_are_filled() {
        let t = Translations::load(None);
        let tr = t.tr(Language::English);
        assert_eq!(tr.fmt("turbo.selected", &[("buttons", "A+B".into())]), "Selected: A+B");
        let m = Message::new("log.stopped").key("feature", "feature.turbo");
        assert_eq!(tr.msg(&m), "Turbo stopped");
        assert_eq!(t.tr(Language::Japanese).msg(&m), "連打 停止");
    }

    #[test]
    fn unknown_key_falls_back_to_key() {
        let t = Translations::load(None);
        assert_eq!(t.tr(Language::Japanese).t("no.such.key"), "no.such.key");
    }
}
