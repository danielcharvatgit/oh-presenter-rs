//! Customisable keyboard shortcuts.

use eframe::egui::{self, Key, Modifiers};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct KeyBinding {
    pub key: Key,
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
}

impl KeyBinding {
    pub const fn plain(key: Key) -> Self {
        Self { key, ctrl: false, shift: false, alt: false }
    }
    pub const fn ctrl(key: Key) -> Self {
        Self { key, ctrl: true, shift: false, alt: false }
    }
    pub const fn ctrl_shift(key: Key) -> Self {
        Self { key, ctrl: true, shift: true, alt: false }
    }

    pub fn has_modifier(&self) -> bool {
        self.ctrl || self.alt
    }

    pub fn from_event(key: Key, m: Modifiers) -> Self {
        Self { key, ctrl: m.command || m.ctrl, shift: m.shift, alt: m.alt }
    }

    /// Consumes the key press if it happened this frame. Modifiers must match exactly,
    /// so `Ctrl+S` does not fire on `Ctrl+Shift+S`.
    pub fn consume(&self, input: &mut egui::InputState) -> bool {
        let mut hit = false;
        input.events.retain(|e| {
            let m = matches!(e, egui::Event::Key { key, pressed: true, modifiers, .. }
                if KeyBinding::from_event(*key, *modifiers) == *self);
            hit |= m;
            !m
        });
        hit
    }

    pub fn parse(s: &str) -> Option<Self> {
        let mut b = Self::plain(Key::A);
        let mut key = None;
        for part in s.split('+') {
            match part.trim() {
                "Ctrl" | "Cmd" => b.ctrl = true,
                "Shift" => b.shift = true,
                "Alt" => b.alt = true,
                // a literal '+' key ("Ctrl++")
                "" => key = Some(Key::Plus),
                other => key = Some(Key::from_name(other)?),
            }
        }
        b.key = key?;
        Some(b)
    }
}

impl std::fmt::Display for KeyBinding {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.ctrl {
            f.write_str("Ctrl+")?;
        }
        if self.alt {
            f.write_str("Alt+")?;
        }
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
    NewService,
    OpenService,
    /// Saves whatever the current tab edits (service / song / design).
    Save,
    /// "Save as" for the service, "save as new" for songs and designs.
    SaveAs,
    NextSlide,
    PrevSlide,
    FirstSlide,
    LastSlide,
    NextItem,
    PrevItem,
    GoLive,
    ToggleLive,
    Blank,
    ToggleOutput,
    OutputFullscreen,
    /// Opens the alert window (a message running across the output).
    Alert,
    Find,
    TabService,
    TabSongs,
    TabDesigner,
    TabBible,
    TabSettings,
    PinPassage,
    Undo,
    Redo,
    Quit,
}

impl Action {
    pub const ALL: &'static [Action] = &[
        Action::NewService,
        Action::OpenService,
        Action::Save,
        Action::SaveAs,
        Action::NextSlide,
        Action::PrevSlide,
        Action::FirstSlide,
        Action::LastSlide,
        Action::NextItem,
        Action::PrevItem,
        Action::GoLive,
        Action::ToggleLive,
        Action::Blank,
        Action::ToggleOutput,
        Action::OutputFullscreen,
        Action::Alert,
        Action::Find,
        Action::TabService,
        Action::TabSongs,
        Action::TabDesigner,
        Action::TabBible,
        Action::TabSettings,
        Action::PinPassage,
        Action::Undo,
        Action::Redo,
        Action::Quit,
    ];

    /// Translation key of the action's description.
    pub fn label_key(self) -> String {
        format!("action.{self:?}")
    }

    /// Slide navigation actions are only active where slides are shown and never
    /// while typing into a text field.
    pub fn is_navigation(self) -> bool {
        matches!(
            self,
            Action::NextSlide
                | Action::PrevSlide
                | Action::FirstSlide
                | Action::LastSlide
                | Action::NextItem
                | Action::PrevItem
                | Action::GoLive
                | Action::ToggleLive
                | Action::Blank
        )
    }

    pub fn defaults(self) -> Vec<KeyBinding> {
        use Key::*;
        let p = KeyBinding::plain;
        let c = KeyBinding::ctrl;
        match self {
            Action::NewService => vec![c(N)],
            Action::OpenService => vec![c(O)],
            Action::Save => vec![c(S)],
            Action::SaveAs => vec![KeyBinding::ctrl_shift(S)],
            Action::NextSlide => vec![p(ArrowDown), p(PageDown), p(ArrowRight), p(Space)],
            Action::PrevSlide => vec![p(ArrowUp), p(PageUp), p(ArrowLeft)],
            Action::FirstSlide => vec![p(Home)],
            Action::LastSlide => vec![p(End)],
            Action::NextItem => vec![c(ArrowDown), c(PageDown)],
            Action::PrevItem => vec![c(ArrowUp), c(PageUp)],
            Action::GoLive => vec![p(Enter)],
            Action::ToggleLive => vec![p(L)],
            Action::Blank => vec![p(Period)],
            Action::ToggleOutput => vec![p(F5)],
            Action::OutputFullscreen => vec![p(F11)],
            Action::Alert => vec![p(F7)],
            Action::Find => vec![c(F)],
            Action::TabService => vec![c(Num1)],
            Action::TabSongs => vec![c(Num2)],
            Action::TabDesigner => vec![c(Num3)],
            Action::TabBible => vec![c(Num4)],
            Action::TabSettings => vec![c(Num5)],
            Action::PinPassage => vec![c(D)],
            Action::Undo => vec![c(Z)],
            Action::Redo => vec![KeyBinding::ctrl_shift(Z), c(Y)],
            Action::Quit => vec![c(Q)],
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
    pub fn bindings(&self, action: Action) -> Vec<KeyBinding> {
        self.map.get(&action).cloned().unwrap_or_else(|| action.defaults())
    }

    pub fn describe(&self, action: Action) -> String {
        self.bindings(action).iter().map(|b| b.to_string()).collect::<Vec<_>>().join(", ")
    }

    /// Other actions already bound to `binding`.
    pub fn conflicts(&self, binding: &KeyBinding, except: Action) -> Vec<Action> {
        Action::ALL.iter().copied().filter(|a| *a != except && self.bindings(*a).contains(binding)).collect()
    }

    /// Returns the triggered actions this frame. Plain keys (no Ctrl/Alt) are skipped
    /// while a text field has focus.
    /// Only actions for which `enabled` returns true are checked (and consumed).
    pub fn triggered(&self, ctx: &egui::Context, enabled: impl Fn(Action) -> bool) -> Vec<Action> {
        let typing = ctx.egui_wants_keyboard_input();
        let mut out = Vec::new();
        ctx.input_mut(|input| {
            for action in Action::ALL {
                if !enabled(*action) || (typing && matches!(action, Action::Undo | Action::Redo)) {
                    // text fields have their own undo
                    continue;
                }
                for b in self.bindings(*action) {
                    if typing && !b.has_modifier() && !is_function_key(b.key) {
                        continue;
                    }
                    if b.consume(input) {
                        out.push(*action);
                        break;
                    }
                }
            }
        });
        out
    }
}

fn is_digit(key: Key) -> bool {
    use Key::*;
    matches!(key, Num0 | Num1 | Num2 | Num3 | Num4 | Num5 | Num6 | Num7 | Num8 | Num9)
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

/// On many layouts (Czech, Slovak, French, …) the digit row types symbols or letters
/// without Shift. Letters fall back to the digit already, but a symbol does not: on the
/// Czech layout the `1` key is reported as `+`, so `Ctrl+1` became egui's `Ctrl++` zoom.
/// This turns such key events back into the digit of the physical key, so digit
/// shortcuts work on any layout. Typed text is a separate event and is not affected.
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

pub fn is_function_key(key: Key) -> bool {
    use Key::*;
    matches!(key, F1 | F2 | F3 | F4 | F5 | F6 | F7 | F8 | F9 | F10 | F11 | F12)
}

/// Returns the first key pressed this frame (for capturing a new binding).
pub fn capture_key(ctx: &egui::Context) -> Option<KeyBinding> {
    ctx.input(|i| {
        i.events.iter().find_map(|e| match e {
            egui::Event::Key { key, pressed: true, modifiers, .. } => Some(KeyBinding::from_event(*key, *modifiers)),
            _ => None,
        })
    })
}
