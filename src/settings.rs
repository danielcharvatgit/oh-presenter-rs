//! User settings, persisted as JSON in the config directory.

use crate::model::TextStyle;
use crate::shortcuts::{KeyBinding, Shortcuts};
use eframe::egui::Key;
use serde::{Deserialize, Serialize};

/// A way of marking song slides (verse, chorus/refrain, bridge, custom...).
/// Pressing `key` while presenting jumps to the first slide with this mark in
/// the current song (pressing again cycles through them). Verses are also
/// reachable with the number keys 1–9.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SlideMark {
    /// Code stored in songs, e.g. `V`, `C`, `B`.
    pub code: String,
    /// Display name. Empty for built-in marks = use the translation `mark.<code>`.
    #[serde(default)]
    pub name: String,
    pub key: Option<KeyBinding>,
    #[serde(default)]
    pub builtin: bool,
}

impl SlideMark {
    pub fn display_name(&self) -> String {
        if self.name.is_empty() { crate::i18n::t(&format!("mark.{}", self.code)) } else { self.name.clone() }
    }

    pub fn defaults() -> Vec<SlideMark> {
        let m =
            |code: &str, key: Key| SlideMark { code: code.into(), name: String::new(), key: Some(KeyBinding::plain(key)), builtin: true };
        vec![m("V", Key::V), m("C", Key::C), m("P", Key::P), m("B", Key::B), m("I", Key::I), m("E", Key::E), m("O", Key::O)]
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub language: String,
    pub shortcuts: Shortcuts,
    pub marks: Vec<SlideMark>,
    /// Number keys 1–9 jump to verse N.
    pub number_keys_jump_to_verse: bool,
    pub text_style: TextStyle,
    /// Library design used behind song / bible / text slides.
    pub text_background_design: Option<String>,
    pub verses_per_slide: usize,
    pub bible: Option<String>,
    pub network_enabled: bool,
    pub network_port: u16,
    pub live_follows_selection: bool,
    pub recent_services: Vec<String>,
    pub dark_mode: bool,
    /// The offer to import files of the old oh-presenter was answered (imported or "don't ask again").
    pub legacy_import_answered: bool,
    /// Languages (bible language codes) whose bibles have a word search index.
    /// Empty = not chosen yet: the first word search asks.
    pub bible_index_languages: Vec<String>,
    /// Keep the output (projector) window above all other windows.
    pub output_always_on_top: bool,
    /// Look of alerts (messages running across the output).
    pub alert_style: crate::model::AlertStyle,
    /// Text the alert window (F7) starts with.
    pub alert_default_message: String,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            language: "cs".into(),
            shortcuts: Shortcuts::default(),
            marks: SlideMark::defaults(),
            number_keys_jump_to_verse: true,
            text_style: TextStyle::default(),
            text_background_design: None,
            verses_per_slide: 1,
            bible: None,
            // off until asked for: the presenter then listens on the network
            network_enabled: false,
            network_port: crate::net::DEFAULT_PORT,
            live_follows_selection: true,
            recent_services: Vec::new(),
            dark_mode: true,
            legacy_import_answered: false,
            bible_index_languages: Vec::new(),
            output_always_on_top: false,
            alert_style: crate::model::AlertStyle::default(),
            alert_default_message: String::new(),
        }
    }
}

impl Settings {
    pub fn load() -> Self {
        let path = crate::storage::paths().settings_file();
        let mut s: Settings = std::fs::read(&path).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default();
        // make sure new built-in marks / actions appear after upgrades
        for d in SlideMark::defaults() {
            if !s.marks.iter().any(|m| m.code == d.code) {
                s.marks.push(d);
            }
        }
        for a in crate::shortcuts::Action::ALL {
            s.shortcuts.map.entry(*a).or_insert_with(|| a.defaults());
        }
        s
    }

    pub fn save(&self) -> std::io::Result<()> {
        let path = crate::storage::paths().settings_file();
        crate::storage::write_atomic(&path, &serde_json::to_vec_pretty(self).unwrap(), false)
    }

    pub fn mark(&self, code: &str) -> Option<&SlideMark> {
        self.marks.iter().find(|m| m.code == code)
    }

    pub fn mark_name(&self, code: &str) -> String {
        self.mark(code).map(|m| m.display_name()).unwrap_or_else(|| code.to_string())
    }

    pub fn add_recent(&mut self, path: &str) {
        self.recent_services.retain(|p| p != path);
        self.recent_services.insert(0, path.to_string());
        self.recent_services.truncate(10);
    }
}
