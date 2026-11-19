//! Keyboard navigation of the program itself:
//!
//! * **Alt + letters**: hold Alt and type the beginning of a menu or tab name, then let
//!   go of Alt — `Alt+s` = the first item starting with "s" (Service), `Alt+set` =
//!   Settings, `Alt+f` = the File menu. A number first picks among several matches:
//!   `Alt+3s` = the third item starting with "s" (Slide designer); repeating the letter
//!   cycles through those items: `Alt+sss` = Service › Songs › Slide designer. Holding Alt a moment
//!   shows all items. Esc cancels.
//! * **Scrolling** the Songs and Settings tabs with ↑ ↓ PgUp PgDn Home End (in the other
//!   tabs these keys present slides or move designer objects).

use super::{App, Tab};
use crate::i18n::t;
use crate::storage::fold;
use eframe::egui::{self, Event, Key, Vec2};
use std::time::{Duration, Instant};

/// After holding Alt this long the list of items is shown.
const HINT_DELAY: Duration = Duration::from_millis(450);
const LINE: f32 = 40.0;

/// A menu or a tab of the top bar.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BarItem {
    /// Index of the menu in the bar (0 = File, 1 = Output).
    Menu(usize),
    Tab(Tab),
}

#[derive(Default)]
pub struct AltNav {
    /// Alt is held since.
    since: Option<Instant>,
    /// Typed while Alt is held.
    pub typed: String,
    /// A menu to open on the next frame.
    pub open_menu: Option<usize>,
    /// The last focus change was made with Tab: the focused widget gets a frame.
    pub keyboard_focus: bool,
    /// Popup ids of the File and Output menus (from the last frame), see
    /// [`App::menu_arrows_as_tab`].
    pub menu_ids: [Option<egui::Id>; 2],
}

/// A scroll asked for with the keyboard, applied by the tab's scroll area.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum KeyScroll {
    Lines(f32),
    Pages(f32),
    Top,
    Bottom,
}

/// The menus and tabs of the bar with their labels, in the order shown.
pub fn bar_items() -> Vec<(String, BarItem)> {
    vec![
        (t("menu.file"), BarItem::Menu(0)),
        (t("menu.output"), BarItem::Menu(1)),
        (t("tab.service"), BarItem::Tab(Tab::Service)),
        (t("tab.songs"), BarItem::Tab(Tab::Songs)),
        (t("tab.designer"), BarItem::Tab(Tab::Designer)),
        (t("tab.bible"), BarItem::Tab(Tab::Bible)),
        (t("tab.settings"), BarItem::Tab(Tab::Settings)),
    ]
}

/// What was typed, understood: the matching items and which of them is chosen
/// (an index into the matches).
///
/// * an optional number, then the beginning of a name: `set` = Settings, `3s` = the
///   third item starting with "s";
/// * one letter repeated cycles through the items starting with it: `s` Service,
///   `ss` Songs, `sss` Slide designer, … (wrapping around) — unless a name really
///   starts with those letters;
/// * names are matched from their start (case and diacritics ignored); if none
///   matches, also from the start of any of their words (`des` = Slide designer).
pub fn interpret(typed: &str, items: &[(String, BarItem)]) -> (Vec<usize>, Option<usize>) {
    let digits: String = typed.chars().take_while(|c| c.is_ascii_digit()).collect();
    let prefix = fold(&typed[digits.len()..]);
    let find = |prefix: &str| -> Vec<usize> {
        let start: Vec<usize> = (0..items.len()).filter(|&i| fold(&items[i].0).starts_with(prefix)).collect();
        if !start.is_empty() {
            return start;
        }
        (0..items.len()).filter(|&i| fold(&items[i].0).split_whitespace().any(|w| w.starts_with(prefix))).collect()
    };
    if prefix.is_empty() {
        // nothing typed, or only a number: all items (a number alone picks the n-th)
        let pos = digits.parse::<usize>().ok().filter(|n| *n >= 1 && *n <= items.len()).map(|n| n - 1);
        return ((0..items.len()).collect(), pos);
    }
    let found = find(&prefix);
    let mut letters = prefix.chars();
    let first = letters.next().unwrap_or(' ');
    let repeated = prefix.chars().count() > 1 && letters.all(|c| c == first);
    if found.is_empty() && repeated && digits.is_empty() {
        let same = find(&first.to_string());
        if same.is_empty() {
            return (same, None);
        }
        let pos = (prefix.chars().count() - 1) % same.len();
        return (same, Some(pos));
    }
    let n: usize = digits.parse().unwrap_or(1).max(1);
    let pos = (n <= found.len()).then(|| n - 1);
    (found, pos)
}

/// The items listed for what was typed.
pub fn matches(typed: &str, items: &[(String, BarItem)]) -> Vec<usize> {
    interpret(typed, items).0
}

/// The item chosen by what was typed.
pub fn chosen(typed: &str, items: &[(String, BarItem)]) -> Option<BarItem> {
    if typed.is_empty() {
        return None;
    }
    let (found, pos) = interpret(typed, items);
    pos.map(|p| items[found[p]].1)
}

fn typed_char(key: Key) -> Option<char> {
    let name = key.name();
    let mut chars = name.chars();
    let c = chars.next()?;
    (chars.next().is_none() && c.is_ascii_alphanumeric()).then(|| c.to_ascii_lowercase())
}

impl App {
    /// While the File or Output menu is open, ↓ and ↑ move between its items like
    /// Tab and Shift+Tab (instead of stepping slides, which would also close the menu).
    /// Runs on the raw input, before egui and the shortcuts see the keys.
    pub fn menu_arrows_as_tab(&mut self, ctx: &egui::Context, raw: &mut egui::RawInput) {
        let open = self.alt_nav.menu_ids.iter().flatten().any(|id| egui::Popup::is_id_open(ctx, *id));
        if !open {
            return;
        }
        for e in &mut raw.events {
            if let Event::Key { key, modifiers, .. } = e
                && modifiers.is_none()
            {
                match key {
                    Key::ArrowDown => *key = Key::Tab,
                    Key::ArrowUp => {
                        *key = Key::Tab;
                        *modifiers = egui::Modifiers::SHIFT;
                    }
                    _ => {}
                }
            }
        }
        // the frame around the focused item, as with Tab
        self.alt_nav.keyboard_focus |= raw.events.iter().any(|e| matches!(e, Event::Key { key: Key::Tab, pressed: true, .. }));
    }

    /// Collects Alt + letters (before the UI runs, so text fields do not get them) and
    /// acts when Alt is released.
    pub fn alt_navigation(&mut self, ctx: &egui::Context) {
        if self.capture.is_some() || self.dialog.is_some() || self.file_dialog.is_some() {
            self.alt_nav = AltNav { open_menu: self.alt_nav.open_menu, ..Default::default() };
            return;
        }
        let (alt, focused, escape, typed) = ctx.input_mut(|i| {
            // Ctrl+Alt is AltGr on Windows (it types characters like @ on Czech keyboards)
            let alt = i.modifiers.alt && !i.modifiers.ctrl && !i.modifiers.command;
            let mut typed = String::new();
            let mut escape = false;
            if alt {
                i.events.retain(|e| match e {
                    Event::Key { key, pressed: true, .. } if *key == Key::Escape => {
                        escape = true;
                        false
                    }
                    Event::Key { key, pressed, .. } => match typed_char(*key) {
                        Some(c) => {
                            if *pressed {
                                typed.push(c);
                            }
                            false
                        }
                        None => true,
                    },
                    // the letters must not end up in a text field
                    Event::Text(_) => false,
                    _ => true,
                });
            }
            (alt, i.focused, escape, typed)
        });
        if !focused || escape {
            self.alt_nav.since = None;
            self.alt_nav.typed.clear();
            return;
        }
        if alt {
            self.alt_nav.since.get_or_insert_with(Instant::now);
            self.alt_nav.typed.push_str(&typed);
            // to show the list once Alt was held long enough
            ctx.request_repaint_after(Duration::from_millis(100));
            return;
        }
        // Alt released
        self.alt_nav.since = None;
        let typed = std::mem::take(&mut self.alt_nav.typed);
        match chosen(&typed, &bar_items()) {
            Some(BarItem::Tab(tab)) => {
                // an open menu (e.g. File from Alt+1) closes when a tab is chosen
                egui::Popup::close_all(ctx);
                self.tab = tab;
            }
            Some(BarItem::Menu(m)) => {
                self.alt_nav.open_menu = Some(m);
                ctx.request_repaint();
            }
            None => {}
        }
    }

    /// While Alt is held (a moment, or after typing): the items and what would be chosen.
    pub fn alt_navigation_hint(&self, ctx: &egui::Context) {
        let Some(since) = self.alt_nav.since else { return };
        if self.alt_nav.typed.is_empty() && since.elapsed() < HINT_DELAY {
            return;
        }
        let items = bar_items();
        let shown = matches(&self.alt_nav.typed, &items);
        let target = chosen(&self.alt_nav.typed, &items);
        egui::Area::new(egui::Id::unique("alt_nav_hint"))
            .order(egui::Order::Tooltip)
            .anchor(egui::Align2::CENTER_TOP, Vec2::new(0.0, 36.0))
            .show(ctx, |ui| {
                egui::Frame::popup(ui.style()).show(ui, |ui| {
                    ui.label(egui::RichText::new(format!("Alt + {}", self.alt_nav.typed)).strong());
                    ui.weak(t("keynav.hint"));
                    ui.separator();
                    for (n, &i) in shown.iter().enumerate() {
                        let (label, item) = &items[i];
                        let text = format!("{}  {}", n + 1, label);
                        if Some(*item) == target {
                            ui.label(egui::RichText::new(format!("▶ {text}")).strong().color(super::ADD_COLOR));
                        } else {
                            ui.label(format!("   {text}"));
                        }
                    }
                    if shown.is_empty() {
                        ui.weak(t("songs.nothing_found"));
                    }
                });
            });
    }

    /// A clear frame around the widget focused with the keyboard (Tab / Shift+Tab);
    /// egui itself only draws such a widget a little darker. Shown from a Tab press
    /// until the mouse is clicked.
    pub fn keyboard_focus_frame(&mut self, ctx: &egui::Context) {
        ctx.input(|i| {
            if i.events.iter().any(|e| matches!(e, Event::Key { key: Key::Tab, pressed: true, .. })) {
                self.alt_nav.keyboard_focus = true;
            }
            if i.pointer.any_pressed() {
                self.alt_nav.keyboard_focus = false;
            }
        });
        if !self.alt_nav.keyboard_focus {
            return;
        }
        let Some(resp) = ctx.memory(|m| m.focused()).and_then(|id| ctx.read_response(id)) else { return };
        let rect = resp.interact_rect.expand(2.0);
        if !rect.is_positive() {
            return;
        }
        let painter = ctx.layer_painter(egui::LayerId::new(egui::Order::Tooltip, egui::Id::unique("keyboard_focus")));
        let color = ctx.global_style().visuals.selection.stroke.color;
        painter.rect_stroke(rect, 3.0, egui::Stroke::new(2.5, color), egui::StrokeKind::Outside);
    }

    /// Arrow / page keys scroll the Songs and Settings tabs (when not typing).
    pub fn keyboard_scroll(&mut self, ctx: &egui::Context) {
        self.key_scroll = None;
        if !matches!(self.tab, Tab::Songs | Tab::Settings)
            || self.capture.is_some()
            || self.dialog.is_some()
            || self.file_dialog.is_some()
            || ctx.egui_wants_keyboard_input()
        {
            return;
        }
        let none = egui::Modifiers::NONE;
        self.key_scroll = ctx.input_mut(|i| {
            if i.consume_key(none, Key::ArrowDown) {
                Some(KeyScroll::Lines(1.0))
            } else if i.consume_key(none, Key::ArrowUp) {
                Some(KeyScroll::Lines(-1.0))
            } else if i.consume_key(none, Key::PageDown) {
                Some(KeyScroll::Pages(1.0))
            } else if i.consume_key(none, Key::PageUp) {
                Some(KeyScroll::Pages(-1.0))
            } else if i.consume_key(none, Key::Home) {
                Some(KeyScroll::Top)
            } else if i.consume_key(none, Key::End) {
                Some(KeyScroll::Bottom)
            } else {
                None
            }
        });
    }

    #[cfg(test)]
    pub fn key_scroll_is_none(&self) -> bool {
        self.key_scroll.is_none()
    }

    /// Shows the tab's main scroll area (`name` tells the areas apart); ↑ ↓ PgUp PgDn
    /// Home End move it. The position and content height of the last frame are kept,
    /// so the new position is exact and stays within the content.
    pub fn key_scroll_area<R>(
        &mut self,
        ui: &mut egui::Ui,
        name: &'static str,
        area: egui::ScrollArea,
        add_contents: impl FnOnce(&mut App, &mut egui::Ui) -> R,
    ) -> R {
        let (offset, max) = self.scroll_pos.get(name).copied().unwrap_or((0.0, 0.0));
        let page = (ui.available_height() - LINE).max(LINE);
        let target = self.key_scroll.take().map(|s| match s {
            KeyScroll::Lines(n) => offset + n * LINE,
            KeyScroll::Pages(n) => offset + n * page,
            KeyScroll::Top => 0.0,
            KeyScroll::Bottom => max,
        });
        let area = match target {
            Some(y) => area.vertical_scroll_offset(y.clamp(0.0, max.max(0.0))),
            None => area,
        };
        let out = area.id_salt(name).show(ui, |ui| add_contents(self, ui));
        let max = (out.content_size.y - out.inner_rect.height()).max(0.0);
        self.scroll_pos.insert(name, (out.state.clamped_offset().y, max));
        out.inner
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn items() -> Vec<(String, BarItem)> {
        [
            ("File", BarItem::Menu(0)),
            ("Output", BarItem::Menu(1)),
            ("Service", BarItem::Tab(Tab::Service)),
            ("Songs", BarItem::Tab(Tab::Songs)),
            ("Slide designer", BarItem::Tab(Tab::Designer)),
            ("Bible", BarItem::Tab(Tab::Bible)),
            ("Settings", BarItem::Tab(Tab::Settings)),
        ]
        .into_iter()
        .map(|(l, i)| (l.to_string(), i))
        .collect()
    }

    #[test]
    fn choosing_by_typed_letters() {
        let it = items();
        assert_eq!(chosen("s", &it), Some(BarItem::Tab(Tab::Service)));
        assert_eq!(chosen("f", &it), Some(BarItem::Menu(0)));
        assert_eq!(chosen("3s", &it), Some(BarItem::Tab(Tab::Designer)));
        assert_eq!(chosen("set", &it), Some(BarItem::Tab(Tab::Settings)));
        assert_eq!(chosen("so", &it), Some(BarItem::Tab(Tab::Songs)));
        assert_eq!(chosen("sl", &it), Some(BarItem::Tab(Tab::Designer)));
        assert_eq!(chosen("2s", &it), Some(BarItem::Tab(Tab::Songs)));
        assert_eq!(chosen("des", &it), Some(BarItem::Tab(Tab::Designer)), "start of a word");
        // a repeated letter cycles through the items starting with it, wrapping around
        assert_eq!(chosen("ss", &it), Some(BarItem::Tab(Tab::Songs)));
        assert_eq!(chosen("sss", &it), Some(BarItem::Tab(Tab::Designer)));
        assert_eq!(chosen("ssss", &it), Some(BarItem::Tab(Tab::Settings)));
        assert_eq!(chosen("sssss", &it), Some(BarItem::Tab(Tab::Service)));
        assert_eq!(chosen("ff", &it), Some(BarItem::Menu(0)), "only one item: stays on it");
        assert_eq!(matches("sss", &it).len(), 4, "the list shows all items with that letter");
        assert_eq!(chosen("xx", &it), None);
        // ...unless a name really starts with the repeated letters
        let ab = vec![("Aardvark".to_string(), BarItem::Tab(Tab::Bible)), ("Apple".to_string(), BarItem::Tab(Tab::Songs))];
        assert_eq!(chosen("aa", &ab), Some(BarItem::Tab(Tab::Bible)));
        // a number alone picks the n-th item of the bar
        assert_eq!(chosen("3", &it), Some(BarItem::Tab(Tab::Service)));
        assert_eq!(chosen("9s", &it), None);
        assert_eq!(chosen("x", &it), None);
        assert_eq!(chosen("", &it), None);
        // diacritics do not matter (Czech names)
        let cs = vec![("Shromáždění".to_string(), BarItem::Tab(Tab::Service)), ("Písně".to_string(), BarItem::Tab(Tab::Songs))];
        assert_eq!(chosen("pi", &cs), Some(BarItem::Tab(Tab::Songs)));
        assert_eq!(chosen("shr", &cs), Some(BarItem::Tab(Tab::Service)));
    }
}
