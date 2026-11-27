//! Keyboard shortcuts of the web edition. The program's keys that the browser or the system
//! takes (Ctrl+1 … switch browser tabs, F5 reloads, Ctrl+F finds, F7 caret browsing, F11 full
//! screen, Ctrl+S saves the page, Ctrl+↑↓ scroll) are one letter here; no key uses Ctrl or Alt.
//! Shortcuts never fire while a text field has the keyboard.

use eframe::egui::{self, Key, Modifiers};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct KeyBinding {
    pub key: Key,
    pub shift: bool,
}

impl KeyBinding {
    pub const fn plain(key: Key) -> Self {
        Self { key, shift: false }
    }
    pub const fn shift(key: Key) -> Self {
        Self { key, shift: true }
    }

    /// The binding of a key press; `None` with Ctrl / Alt / Cmd (those belong to the browser).
    pub fn from_event(key: Key, m: Modifiers) -> Option<Self> {
        (!m.ctrl && !m.alt && !m.command && !m.mac_cmd).then_some(Self { key, shift: m.shift })
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s.strip_prefix("Shift+") {
            Some(k) => Some(Self::shift(Key::from_name(k)?)),
            None => Some(Self::plain(Key::from_name(s)?)),
        }
    }

    /// Keys the browser or the system use, so they cannot be shortcuts of the page.
    pub fn reserved(&self) -> bool {
        use Key::*;
        matches!(self.key, F1 | F3 | F5 | F6 | F7 | F11 | F12 | Tab | Escape) || (self.shift && self.key == Space)
    }
}

impl std::fmt::Display for KeyBinding {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.shift {
            f.write_str("Shift+")?;
        }
        f.write_str(self.key.name())
    }
}

impl Serialize for KeyBinding {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.to_string())
    }
}

impl<'de> Deserialize<'de> for KeyBinding {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        KeyBinding::parse(&s).ok_or_else(|| serde::de::Error::custom(format!("bad key binding {s:?}")))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Action {
    NextSlide,
    PrevSlide,
    FirstSlide,
    LastSlide,
    NextItem,
    PrevItem,
    GoLive,
    ToggleLive,
    Blank,
    Freeze,
    ToggleOutput,
    /// In the output window: full screen on / off.
    OutputFullscreen,
    Alert,
    Find,
    AddToService,
    RenameItem,
    RemoveItem,
    TabService,
    TabSongs,
    TabBible,
    TabSettings,
}

impl Action {
    pub const ALL: &'static [Action] = &[
        Action::NextSlide,
        Action::PrevSlide,
        Action::FirstSlide,
        Action::LastSlide,
        Action::NextItem,
        Action::PrevItem,
        Action::GoLive,
        Action::ToggleLive,
        Action::Blank,
        Action::Freeze,
        Action::ToggleOutput,
        Action::OutputFullscreen,
        Action::Alert,
        Action::Find,
        Action::AddToService,
        Action::RenameItem,
        Action::RemoveItem,
        Action::TabService,
        Action::TabSongs,
        Action::TabBible,
        Action::TabSettings,
    ];

    pub fn label_key(self) -> String {
        format!("action.{self:?}")
    }

    /// Keys that present (they work in the Service and Bible tabs).
    pub fn is_navigation(self) -> bool {
        use Action::*;
        matches!(self, NextSlide | PrevSlide | FirstSlide | LastSlide | NextItem | PrevItem | GoLive | ToggleLive | Blank | Freeze)
    }

    pub fn defaults(self) -> Vec<KeyBinding> {
        use Key::*;
        let p = KeyBinding::plain;
        let s = KeyBinding::shift;
        match self {
            Action::NextSlide => vec![p(ArrowDown), p(PageDown), p(ArrowRight)],
            Action::PrevSlide => vec![p(ArrowUp), p(PageUp), p(ArrowLeft)],
            Action::FirstSlide => vec![p(Home)],
            Action::LastSlide => vec![p(End)],
            // the program: Ctrl+↓ / Ctrl+↑ (the browser scrolls with them)
            Action::NextItem => vec![p(N)],
            Action::PrevItem => vec![s(N)],
            Action::GoLive => vec![p(Enter), p(G)],
            Action::ToggleLive => vec![p(L)],
            Action::Blank => vec![p(Period)],
            Action::Freeze => vec![p(Comma)],
            // the program: F5 (reloads the page) / F11 (the browser's full screen)
            Action::ToggleOutput => vec![p(W)],
            Action::OutputFullscreen => vec![p(F)],
            // the program: F7 (caret browsing in the browser)
            Action::Alert => vec![p(T)],
            // the program: Ctrl+F (the browser's find)
            Action::Find => vec![p(S)],
            // the program: Ctrl+Enter
            Action::AddToService => vec![p(A)],
            Action::RenameItem => vec![p(F2)],
            Action::RemoveItem => vec![p(Delete)],
            // the program: Ctrl+1 … (they switch the browser's tabs); in the order of the tabs
            Action::TabService => vec![s(Num1)],
            Action::TabSongs => vec![s(Num2)],
            Action::TabBible => vec![s(Num3)],
            Action::TabSettings => vec![s(Num4)],
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Shortcuts {
    pub map: BTreeMap<Action, Vec<KeyBinding>>,
}

impl Default for Shortcuts {
    fn default() -> Self {
        Self { map: Action::ALL.iter().map(|a| (*a, a.defaults())).collect() }
    }
}

impl Shortcuts {
    pub fn bindings(&self, a: Action) -> Vec<KeyBinding> {
        self.map.get(&a).cloned().unwrap_or_else(|| a.defaults())
    }

    pub fn describe(&self, a: Action) -> String {
        self.bindings(a).iter().map(|b| b.to_string()).collect::<Vec<_>>().join(", ")
    }

    /// Other actions using `binding`, and the slide marks / verse numbers (`marks`) it would hide.
    pub fn conflicts(&self, binding: &KeyBinding, except: Action) -> Vec<Action> {
        Action::ALL.iter().copied().filter(|a| *a != except && self.bindings(*a).contains(binding)).collect()
    }

    /// The actions whose key was pressed this frame (the key events are used up).
    pub fn triggered(&self, ctx: &egui::Context, enabled: impl Fn(Action) -> bool) -> Vec<Action> {
        let mut out = Vec::new();
        ctx.input_mut(|input| {
            for a in Action::ALL {
                if !enabled(*a) {
                    continue;
                }
                let keys = self.bindings(*a);
                let mut hit = false;
                input.events.retain(|e| {
                    let m = matches!(e, egui::Event::Key { key, pressed: true, modifiers, .. }
                        if KeyBinding::from_event(*key, *modifiers).is_some_and(|b| keys.contains(&b)));
                    hit |= m;
                    !m
                });
                if hit {
                    out.push(*a);
                }
            }
        });
        out
    }
}

/// The first key pressed this frame (to bind it). Ctrl / Alt combinations are not offered.
pub fn capture_key(ctx: &egui::Context) -> Option<KeyBinding> {
    ctx.input(|i| {
        i.events.iter().find_map(|e| match e {
            egui::Event::Key { key, pressed: true, modifiers, .. } if *key != Key::Escape => KeyBinding::from_event(*key, *modifiers),
            _ => None,
        })
    })
}

fn is_digit(key: Key) -> bool {
    use Key::*;
    matches!(key, Num0 | Num1 | Num2 | Num3 | Num4 | Num5 | Num6 | Num7 | Num8 | Num9)
}

/// On many layouts (Czech, French, …) the digit row types symbols or letters: the key's
/// position decides, so `Shift+1` and the verse numbers work on any layout (as in the program).
pub fn digits_by_position(events: &mut [egui::Event]) {
    for e in events {
        if let egui::Event::Key { key, physical_key: Some(p), .. } = e
            && is_digit(*p)
            && is_symbol(*key)
        {
            *key = *p;
        }
    }
}

fn is_symbol(key: Key) -> bool {
    use Key::*;
    matches!(
        key,
        Colon
            | Comma
            | Backslash
            | Slash
            | Pipe
            | Questionmark
            | Exclamationmark
            | OpenBracket
            | CloseBracket
            | OpenCurlyBracket
            | CloseCurlyBracket
            | Backtick
            | Minus
            | Period
            | Plus
            | Equals
            | Semicolon
            | Quote
    )
}

/// Number key → its digit.
pub fn digit_of(key: Key) -> Option<u32> {
    use Key::*;
    Some(match key {
        Num1 => 1,
        Num2 => 2,
        Num3 => 3,
        Num4 => 4,
        Num5 => 5,
        Num6 => 6,
        Num7 => 7,
        Num8 => 8,
        Num9 => 9,
        _ => return None,
    })
}
