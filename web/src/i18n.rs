//! Czech and English texts of the web edition (`web/lang/*.lang`, `key = value` lines,
//! built into the program, so they work offline).

use std::cell::RefCell;
use std::collections::HashMap;

pub const LANGUAGES: &[(&str, &str)] = &[("en", "English"), ("cs", "Čeština")];

const EN: &str = include_str!("../lang/en.lang");
const CS: &str = include_str!("../lang/cs.lang");

pub fn parse(text: &str) -> HashMap<String, String> {
    text.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .filter_map(|l| l.split_once('='))
        .map(|(k, v)| (k.trim().to_string(), v.trim().replace("\\n", "\n")))
        .collect()
}

struct State {
    code: String,
    texts: HashMap<String, String>,
    english: HashMap<String, String>,
}

thread_local! {
    static STATE: RefCell<State> = RefCell::new(State { code: "en".into(), texts: parse(EN), english: parse(EN) });
}

pub fn source(code: &str) -> &'static str {
    if code == "cs" { CS } else { EN }
}

pub fn set_language(code: &str) {
    let code = if LANGUAGES.iter().any(|(c, _)| *c == code) { code } else { "en" };
    STATE.with(|s| {
        let mut s = s.borrow_mut();
        s.code = code.to_string();
        s.texts = parse(source(code));
    });
}

pub fn current_language() -> String {
    STATE.with(|s| s.borrow().code.clone())
}

/// The browser's language when it is one of ours, else English.
pub fn browser_language() -> String {
    let code = crate::js::browser_language();
    if LANGUAGES.iter().any(|(c, _)| *c == code) { code } else { "en".into() }
}

/// The text of `key` (English when the language lacks it, the key when English does too).
pub fn t(key: &str) -> String {
    STATE.with(|s| {
        let s = s.borrow();
        s.texts.get(key).or_else(|| s.english.get(key)).cloned().unwrap_or_else(|| key.to_string())
    })
}

/// [`t`] with `{name}` replaced by values.
pub fn tf(key: &str, args: &[(&str, &str)]) -> String {
    let mut s = t(key);
    for (k, v) in args {
        s = s.replace(&format!("{{{k}}}"), v);
    }
    s
}
