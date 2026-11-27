use super::{App, Dialog, FollowFocus, ItemRef};
use crate::bible::{Bible, Bookmark, Hit};
use crate::i18n::{current_language, t, tf};
use crate::model::*;
use crate::shortcuts::Action;
use eframe::egui::{self, Key, RichText};

impl App {
    pub fn bible_tab(&mut self, ui: &mut egui::Ui) {
        egui::Panel::left("bible_bookmarks").resizable(true).default_size(250.0).show(ui, |ui| self.bible_bookmarks(ui));
        egui::CentralPanel::default_margins().show(ui, |ui| self.bible_search(ui));
    }

    fn bible_bookmarks(&mut self, ui: &mut egui::Ui) {
        let bible_id = self.bible.current.as_ref().map(|b| b.meta.id.clone()).unwrap_or_default();
        let name_of =
            |id: &str, bibles: &[crate::bible::BibleMeta]| bibles.iter().find(|b| b.id == id).map(|b| b.abbrev.clone()).unwrap_or_default();
        let mut open: Option<Bookmark> = None;
        let mut unpin = None;
        let mut changed = false;
        ui.heading(format!("📌 {}", t("bible.pins")));
        egui::ScrollArea::vertical()
            .id_salt("pins")
            .max_height(ui.available_height() * 0.5)
            .auto_shrink([false, true])
            .show_following_focus(ui, |ui| {
                if self.bible.bookmarks.pins.is_empty() {
                    ui.weak(t("bible.no_pins"));
                }
                let bibles = self.bible.bibles.clone();
                for (i, p) in self.bible.bookmarks.pins.iter_mut().enumerate() {
                    ui.push_id(i, |ui| {
                        ui.horizontal(|ui| {
                            let label = if p.bible == bible_id {
                                p.reference.clone()
                            } else {
                                format!("{} ({})", p.reference, name_of(&p.bible, &bibles))
                            };
                            if ui.link(label).clicked() {
                                open = Some(p.clone());
                            }
                            if ui.small_button("✖").on_hover_text(t("bible.unpin")).clicked() {
                                unpin = Some(i);
                            }
                        });
                        let r =
                            ui.add(egui::TextEdit::singleline(&mut p.note).hint_text(t("bible.note_hint")).desired_width(f32::INFINITY));
                        if r.lost_focus() {
                            changed = true;
                        }
                    });
                }
            });
        ui.separator();
        ui.horizontal(|ui| {
            ui.heading(format!("🕘 {}", t("bible.history")));
            if !self.bible.bookmarks.history.is_empty() && ui.small_button(t("bible.clear_history")).clicked() {
                self.bible.bookmarks.history.clear();
                changed = true;
            }
        });
        egui::ScrollArea::vertical().id_salt("history").auto_shrink([false, false]).show_following_focus(ui, |ui| {
            if self.bible.bookmarks.history.is_empty() {
                ui.weak(t("bible.no_history"));
            }
            for h in &self.bible.bookmarks.history {
                let label = if h.bible == bible_id {
                    h.reference.clone()
                } else {
                    format!("{} ({})", h.reference, name_of(&h.bible, &self.bible.bibles))
                };
                if ui.link(label).clicked() {
                    open = Some(h.clone());
                }
            }
        });
        if let Some(i) = unpin {
            self.bible.bookmarks.pins.remove(i);
            changed = true;
        }
        if changed {
            self.bible.bookmarks.save();
        }
        if let Some(b) = open {
            if b.bible != bible_id && self.bible.bibles.iter().any(|x| x.id == b.bible) {
                self.switch_bible(&b.bible);
            }
            self.bible.query = b.reference;
            self.run_bible_search();
        }
    }

    pub(super) fn switch_bible(&mut self, id: &str) {
        self.bible.current = Bible::load(id);
        self.bible.offer_index = None;
        self.bible.book_hint = None;
        self.bible.info.clear();
        self.load_current_index();
        self.settings.bible = Some(id.to_string());
        self.save_settings();
        self.bible.results.clear();
        self.bible.selected.clear();
        self.bible.book = 0;
        self.bible.chapter = 0;
    }

    pub fn run_bible_search(&mut self) {
        let lang = current_language();
        let query = self.bible.query.trim().to_string();
        let Some(bible) = self.bible.current.as_mut() else { return };
        self.bible.selected.clear();
        self.bible.last_clicked = None;
        self.bible.offer_index = None;
        self.bible.book_hint = None;
        if query.is_empty() {
            self.bible.results.clear();
            return;
        }
        let ranges = bible.parse_reference(&query, &lang);
        // a reference needs a chapter number: "Jan" or "jak" alone are words to search
        // for (offering to open the book), not the first chapter of John / James
        let has_number = query.chars().any(|c| c.is_ascii_digit());
        if has_number && let Some(ranges) = &ranges {
            let hits: Vec<Hit> = ranges.iter().flat_map(|r| bible.verses_in(r)).collect();
            if !hits.is_empty() {
                self.bible.word_search = false;
                let reference = bible.format_reference(&hits, &lang);
                self.bible.info = tf("bible.found_ref", &[("ref", &reference), ("n", &hits.len().to_string())]);
                self.bible.bookmarks.visit(Bookmark { reference, bible: bible.meta.id.clone(), note: String::new() });
                self.bible.book = hits[0].book;
                self.bible.chapter = hits[0].chapter;
                self.bible.selected = vec![true; hits.len()];
                self.bible.results = hits;
                return;
            }
        }
        if !has_number {
            self.bible.book_hint = ranges.and_then(|r| r.first().map(|r| r.book));
        }
        self.bible.results.clear();
        self.bible.word_search = true;
        self.bible_word_search(&query);
    }

    /// The selected verses (or all results when nothing is selected) as a passage.
    pub fn bible_selected_passage(&self) -> Option<Passage> {
        let bible = self.bible.current.as_ref()?;
        let lang = current_language();
        let mut hits: Vec<Hit> = self.bible.results.iter().zip(&self.bible.selected).filter(|(_, s)| **s).map(|(h, _)| h.clone()).collect();
        if hits.is_empty() {
            hits = self.bible.results.clone();
        }
        if hits.is_empty() {
            return None;
        }
        let verses = hits
            .iter()
            .filter_map(|h| {
                let (b, c, v) = bible.verse(h)?;
                Some(PassageVerse {
                    book: bible.book_name(h.book, &lang),
                    chapter: c.number,
                    verse: v.number,
                    text: v.text.clone(),
                    book_number: b.number,
                    extra: Vec::new(),
                })
            })
            .collect();
        Some(Passage {
            reference: bible.format_reference(&hits, &lang),
            version: bible.meta.abbrev.clone(),
            verses,
            bible: bible.meta.id.clone(),
            ..Default::default()
        })
    }

    pub(super) fn bible_show_live(&mut self) {
        if let Some(p) = self.bible_selected_passage() {
            let bible = self.bible.current.as_ref().map(|b| b.meta.id.clone()).unwrap_or_default();
            self.bible.bookmarks.visit(Bookmark { reference: p.reference.clone(), bible, note: String::new() });
            self.quick = Some(ServiceItem::new(ItemKind::Bible(p)));
            self.sel_item = Some(ItemRef::Quick);
            self.sel_slide = 0;
            self.go_live();
        }
    }

    fn bible_search(&mut self, ui: &mut egui::Ui) {
        let sc = self.settings.shortcuts.clone();
        // bible selection
        ui.horizontal_wrapped(|ui| {
            ui.label(t("bible.bible"));
            let current =
                self.bible.current.as_ref().map(|b| format!("{} ({})", b.meta.name, b.meta.abbrev)).unwrap_or_else(|| t("bible.none"));
            let mut switch = None;
            egui::ComboBox::from_id_salt("bible_select").selected_text(current).width(260.0).show_ui(ui, |ui| {
                for b in &self.bible.bibles {
                    let sel = self.bible.current.as_ref().map(|c| c.meta.id == b.id).unwrap_or(false);
                    if ui.selectable_label(sel, format!("{} ({})", b.name, b.abbrev)).clicked() && !sel {
                        switch = Some(b.id.clone());
                    }
                }
            });
            if let Some(id) = switch {
                self.switch_bible(&id);
            }
            if ui.button(t("import.bible")).on_hover_text(t("import.bible_hint")).clicked() {
                self.import_bible();
            }
            if let Some(b) = &self.bible.current {
                let id = b.meta.id.clone();
                if ui.button(t("export.bible")).on_hover_text(t("export.bible_hint")).clicked() {
                    self.export_bible_txt();
                }
                if ui.button(format!("✖ {}", t("bible.delete"))).clicked() {
                    self.dialog = Some(Dialog::DeleteBible(id));
                }
            }
        });
        if self.bible.current.is_none() {
            ui.add_space(20.0);
            ui.label(t("bible.no_bible"));
            return;
        }
        ui.separator();

        // search
        ui.horizontal(|ui| {
            let r = ui.add(
                egui::TextEdit::singleline(&mut self.bible.query)
                    .hint_text(t("bible.search_hint"))
                    .desired_width(ui.available_width() - 110.0),
            );
            if self.focus_search {
                r.request_focus();
                self.focus_search = false;
            }
            let enter = r.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
            if ui.button(format!("🔍 {}", t("search"))).clicked() || enter {
                self.run_bible_search();
            }
        });

        // what the word search keeps: a testament, a book
        let lang = current_language();
        let mut refilter = false;
        if let Some(bible) = &self.bible.current {
            use crate::bible::Testament;
            let f = &mut self.bible.filter;
            ui.horizontal_wrapped(|ui| {
                ui.label(t("bible.filter")).on_hover_text(t("bible.filter_hint"));
                for tm in [Testament::All, Testament::Old, Testament::New] {
                    if ui.selectable_label(f.testament == tm, t(tm.label_key())).clicked() && f.testament != tm {
                        f.testament = tm;
                        // a book of the other testament is no longer offered
                        if f.book.is_some_and(|b| !tm.contains(b)) {
                            f.book = None;
                        }
                        refilter = true;
                    }
                }
                let name = |n: u32| {
                    bible.books.iter().position(|b| b.number == n).map(|i| bible.book_name(i, &lang)).unwrap_or_else(|| n.to_string())
                };
                let shown = f.book.map(name).unwrap_or_else(|| t("bible.all_books"));
                egui::ComboBox::from_id_salt("filter_book").selected_text(shown).height(400.0).show_ui(ui, |ui| {
                    if ui.selectable_label(f.book.is_none(), t("bible.all_books")).clicked() && f.book.is_some() {
                        f.book = None;
                        refilter = true;
                    }
                    for (i, b) in bible.books.iter().enumerate().filter(|(_, b)| f.testament.contains(b.number)) {
                        if ui.selectable_label(f.book == Some(b.number), bible.book_name(i, &lang)).clicked() && f.book != Some(b.number) {
                            f.book = Some(b.number);
                            refilter = true;
                        }
                    }
                });
            });
        }
        if refilter && self.bible.word_search && !self.bible.query.trim().is_empty() {
            self.run_bible_search();
        }

        // browse by book / chapter
        let mut browse = false;
        if let Some(bible) = &self.bible.current {
            ui.horizontal(|ui| {
                ui.label(t("bible.browse"));
                let book = self.bible.book.min(bible.books.len().saturating_sub(1));
                egui::ComboBox::from_id_salt("book").selected_text(bible.book_name(book, &lang)).height(400.0).show_ui(ui, |ui| {
                    for i in 0..bible.books.len() {
                        if ui.selectable_label(i == book, bible.book_name(i, &lang)).clicked() {
                            self.bible.book = i;
                            self.bible.chapter = 0;
                            browse = true;
                        }
                    }
                });
                if let Some(b) = bible.books.get(self.bible.book) {
                    let chapter = self.bible.chapter.min(b.chapters.len().saturating_sub(1));
                    egui::ComboBox::from_id_salt("chapter")
                        .selected_text(b.chapters.get(chapter).map(|c| c.number.to_string()).unwrap_or_default())
                        .show_ui(ui, |ui| {
                            for (i, c) in b.chapters.iter().enumerate() {
                                if ui.selectable_label(i == chapter, c.number.to_string()).clicked() {
                                    self.bible.chapter = i;
                                    browse = true;
                                }
                            }
                        });
                    if ui.small_button("◀").clicked() && self.bible.chapter > 0 {
                        self.bible.chapter -= 1;
                        browse = true;
                    }
                    if ui.small_button("▶").clicked() && self.bible.chapter + 1 < b.chapters.len() {
                        self.bible.chapter += 1;
                        browse = true;
                    }
                }
            });
        }
        if browse && let Some(bible) = &self.bible.current {
            let book = &bible.books[self.bible.book];
            let ch = &book.chapters[self.bible.chapter.min(book.chapters.len() - 1)];
            self.bible.query = format!("{} {}", bible.book_name(self.bible.book, &lang), ch.number);
            self.run_bible_search();
            // browsing shows a whole chapter: start with nothing selected
            self.bible.selected.iter_mut().for_each(|s| *s = false);
        }

        ui.separator();
        // actions
        let any = !self.bible.results.is_empty();
        let n_sel = self.bible.selected.iter().filter(|s| **s).count();
        ui.horizontal_wrapped(|ui| {
            ui.add_enabled_ui(any, |ui| {
                if ui.button(format!("▶ {}", t("bible.show_live"))).on_hover_text(t("bible.show_live_hint")).clicked() {
                    self.bible_show_live();
                }
                if ui.button((super::plus_icon(), t("bible.add_to_service"))).on_hover_text(sc.describe(Action::AddToService)).clicked()
                    && let Some(p) = self.bible_selected_passage()
                {
                    self.add_to_service(ItemKind::Bible(p));
                }
                let bible_id = self.bible.current.as_ref().map(|b| b.meta.id.clone()).unwrap_or_default();
                let pinned =
                    self.bible_selected_passage().map(|p| self.bible.bookmarks.is_pinned(&p.reference, &bible_id)).unwrap_or(false);
                let label = if pinned { format!("📌 {}", t("bible.unpin")) } else { format!("📌 {}", t("bible.pin")) };
                if ui.button(label).on_hover_text(sc.describe(Action::PinPassage)).clicked() {
                    self.pin_current_passage();
                }
                if ui.button(t("bible.select_all")).clicked() {
                    self.bible.selected.iter_mut().for_each(|s| *s = true);
                }
                if ui.button(t("bible.select_none")).clicked() {
                    self.bible.selected.iter_mut().for_each(|s| *s = false);
                }
            });
        });
        let mut enable = None;
        let mut open_book = None;
        ui.horizontal_wrapped(|ui| {
            if self.bible.indexing.is_some() {
                ui.spinner();
            }
            ui.weak(&self.bible.info);
            if n_sel > 0 {
                ui.weak(tf("bible.selected", &[("n", &n_sel.to_string())]));
            }
            if let Some(lang) = &self.bible.offer_index
                && ui.button(tf("bible.index_enable", &[("lang", &crate::bible::language_name(lang))])).clicked()
            {
                enable = Some(lang.clone());
            }
            if let (Some(book), Some(bible)) = (self.bible.book_hint, &self.bible.current)
                && ui.link(tf("bible.open_book", &[("book", &bible.book_name(book, &current_language()))])).clicked()
            {
                open_book = Some(book);
            }
        });
        if let Some(lang) = enable {
            self.bible.offer_index = None;
            self.bible.info = t("bible.indexing");
            self.set_index_language(&lang, true);
            if let Some(job) = &mut self.bible.indexing {
                job.then_search = true;
            } else {
                self.run_bible_search();
            }
        }
        if let Some(book) = open_book
            && let Some(bible) = &self.bible.current
        {
            self.bible.query = format!("{} 1", bible.book_name(book, &current_language()));
            self.run_bible_search();
        }
        ui.separator();

        // results
        let Some(bible) = self.bible.current.as_ref() else { return };
        let mut clicked: Option<(usize, egui::Modifiers)> = None;
        let mut double = None;
        egui::ScrollArea::vertical().auto_shrink([false, false]).show_following_focus(ui, |ui| {
            for (i, h) in self.bible.results.iter().enumerate() {
                let Some((_, c, v)) = bible.verse(h) else { continue };
                let reference =
                    format!("{} {}{}{}", bible.book_name(h.book, &lang), c.number, if lang == "cs" { "," } else { ":" }, v.number);
                let sel = self.bible.selected.get(i).copied().unwrap_or(false);
                let mut job = egui::text::LayoutJob::default();
                job.append(
                    &format!("{reference}  "),
                    0.0,
                    egui::TextFormat { color: ui.visuals().strong_text_color(), ..Default::default() },
                );
                job.append(&v.text, 0.0, egui::TextFormat { color: ui.visuals().text_color(), ..Default::default() });
                job.wrap.max_width = ui.available_width() - 10.0;
                let r = ui.add(egui::Button::selectable(sel, job).wrap());
                if r.clicked() {
                    clicked = Some((i, ui.input(|i| i.modifiers)));
                }
                if r.double_clicked() {
                    double = Some(i);
                }
            }
            if self.bible.results.is_empty() && !self.bible.query.is_empty() {
                ui.weak(RichText::new(t("songs.nothing_found")));
            }
        });
        if let Some((i, m)) = clicked {
            let sel = &mut self.bible.selected;
            if m.shift {
                let from = self.bible.last_clicked.unwrap_or(i);
                let (a, b) = (from.min(i), from.max(i));
                for (j, s) in sel.iter_mut().enumerate() {
                    *s = j >= a && j <= b;
                }
            } else if m.command || m.ctrl {
                sel[i] = !sel[i];
            } else {
                sel.iter_mut().enumerate().for_each(|(j, s)| *s = j == i);
            }
            self.bible.last_clicked = Some(i);
        }
        if let Some(i) = double {
            self.bible.selected.iter_mut().enumerate().for_each(|(j, s)| *s = j == i);
            self.bible_show_live();
        }
    }
}
