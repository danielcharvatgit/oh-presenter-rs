//! Translations.
//!
//! Languages are plain-text `.lang` files with one `key = value` per line.
//! English and Czech are compiled in; any other `<code>.lang` file placed in
//! the user `lang` directory is picked up at startup (and may also override
//! the built-in ones). The special key `_name` holds the language's display
//! name. Missing keys fall back to English, then to a readable form of the key.

use std::collections::{BTreeMap, HashMap};
use std::path::Path;
use std::sync::{OnceLock, RwLock};

pub const BUILTIN_EN: &str = include_str!("../lang/en.lang");
pub const BUILTIN_CS: &str = include_str!("../lang/cs.lang");

pub struct Language {
    pub code: String,
    pub name: String,
    strings: HashMap<String, String>,
}

pub struct I18n {
    languages: BTreeMap<String, Language>,
    current: String,
}

fn parse(text: &str) -> HashMap<String, String> {
    let mut map = HashMap::new();
    for line in text.lines() {
        let line = line.trim_start();
        // a byte order mark (files saved by Windows Notepad) must not end up in the first key
        let line = line.trim_start_matches('\u{feff}');
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some((k, v)) = line.split_once('=') {
            let v = v.trim().replace("\\n", "\n");
            map.insert(k.trim().to_string(), v);
        }
    }
    map
}

impl I18n {
    fn new() -> Self {
        let mut s = Self { languages: BTreeMap::new(), current: "en".into() };
        s.add("en", BUILTIN_EN);
        s.add("cs", BUILTIN_CS);
        s
    }

    fn add(&mut self, code: &str, text: &str) {
        let strings = parse(text);
        let lang = self.languages.entry(code.to_string()).or_insert_with(|| Language {
            code: code.to_string(),
            name: code.to_string(),
            strings: HashMap::new(),
        });
        lang.strings.extend(strings);
        if let Some(name) = lang.strings.get("_name") {
            lang.name = name.clone();
        }
    }

    fn get(&self, key: &str) -> String {
        for code in [self.current.as_str(), "en"] {
            if let Some(v) = self.languages.get(code).and_then(|l| l.strings.get(key)) {
                return v.clone();
            }
        }
        humanize(key)
    }
}

/// Last resort for a key missing in every language: never show the raw key
/// (`tab.songs`), show something readable instead (`Songs`).
fn humanize(key: &str) -> String {
    let last = key.rsplit('.').next().unwrap_or(key).replace('_', " ");
    let mut c = last.chars();
    match c.next() {
        Some(f) => f.to_uppercase().chain(c).collect(),
        None => key.to_string(),
    }
}

fn global() -> &'static RwLock<I18n> {
    static I18N: OnceLock<RwLock<I18n>> = OnceLock::new();
    I18N.get_or_init(|| RwLock::new(I18n::new()))
}

/// Loads additional `*.lang` files from `dir`. The file stem is the language code.
pub fn load_dir(dir: &Path) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    let mut i18n = global().write().unwrap();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("lang") {
            continue;
        }
        let (Some(code), Ok(text)) = (path.file_stem().and_then(|s| s.to_str()), std::fs::read_to_string(&path)) else {
            continue;
        };
        i18n.add(code, &text);
    }
}

pub fn set_language(code: &str) {
    let mut i18n = global().write().unwrap();
    if i18n.languages.contains_key(code) {
        i18n.current = code.to_string();
    }
}

pub fn current_language() -> String {
    global().read().unwrap().current.clone()
}

/// (code, display name) of every available language.
pub fn languages() -> Vec<(String, String)> {
    global().read().unwrap().languages.values().map(|l| (l.code.clone(), l.name.clone())).collect()
}

/// Translates `key`.
pub fn t(key: &str) -> String {
    global().read().unwrap().get(key)
}

/// Translates `key` and replaces `{name}` placeholders.
pub fn tf(key: &str, args: &[(&str, &str)]) -> String {
    let mut s = t(key);
    for (name, value) in args {
        s = s.replace(&format!("{{{name}}}"), value);
    }
    s
}
