//! The word search index of bibles: made in the background the first time it is
//! needed, only for the languages the user chose (asked at the first word search,
//! changeable in Settings).

use super::{App, Dialog};
use crate::bible::{self, Bible, BibleMeta, SearchIndex};
use crate::i18n::{t, tf};
use eframe::egui;
use std::sync::mpsc::{Receiver, TryRecvError, channel};

/// Index building that runs in the background.
pub struct Indexing {
    rx: Receiver<Progress>,
    /// Bibles done / all, and the one being indexed now.
    pub done: usize,
    pub total: usize,
    pub current: String,
    /// Run the word search again when the index is ready.
    pub then_search: bool,
}

enum Progress {
    Started(String),
    Finished,
    Failed(String, String),
}

impl App {
    /// Languages of the installed bibles with their bibles, in name order.
    pub fn bible_languages(&mut self) -> Vec<(String, Vec<BibleMeta>)> {
        if self.bible.bibles.iter().any(|b| b.language.is_empty()) && !self.bible.languages_filled {
            bible::fill_missing_languages(&mut self.bible.bibles);
            self.bible.languages_filled = true;
        }
        let mut out: Vec<(String, Vec<BibleMeta>)> = Vec::new();
        for b in &self.bible.bibles {
            match out.iter_mut().find(|(l, _)| *l == b.language) {
                Some((_, list)) => list.push(b.clone()),
                None => out.push((b.language.clone(), vec![b.clone()])),
            }
        }
        out.sort_by_key(|(l, _)| bible::language_name(l));
        out
    }

    pub fn index_enabled(&self, language: &str) -> bool {
        self.settings.bible_index_languages.iter().any(|l| l == language)
    }

    /// Turns the index of a language on (and makes the missing ones) or off (and deletes them).
    pub fn set_index_language(&mut self, language: &str, on: bool) {
        if on {
            if !self.index_enabled(language) {
                self.settings.bible_index_languages.push(language.to_string());
            }
            self.save_settings();
            self.build_missing_indexes(false);
        } else {
            self.settings.bible_index_languages.retain(|l| l != language);
            self.save_settings();
            for b in self.bible.bibles.iter().filter(|b| b.language == language) {
                SearchIndex::delete(&b.id);
            }
            if self.bible.current.as_ref().is_some_and(|b| b.meta.language == language) {
                self.bible.index = None;
            }
        }
    }

    /// Starts making the indexes of all bibles in enabled languages that have none yet.
    pub fn build_missing_indexes(&mut self, then_search: bool) {
        let ids: Vec<(String, String)> = self
            .bible
            .bibles
            .iter()
            .filter(|b| self.index_enabled(&b.language) && !SearchIndex::exists(&b.id))
            .map(|b| (b.id.clone(), b.name.clone()))
            .collect();
        // an index file can also be stale (the bible changed): the current bible is checked when loaded
        let current_needs = self
            .bible
            .current
            .as_ref()
            .filter(|b| self.index_enabled(&b.meta.language) && self.bible.index.is_none())
            .map(|b| (b.meta.id.clone(), b.meta.name.clone()));
        let mut ids = ids;
        if let Some(c) = current_needs
            && !ids.iter().any(|(id, _)| *id == c.0)
        {
            ids.insert(0, c);
        }
        if ids.is_empty() {
            if then_search {
                self.load_current_index();
                self.run_bible_search();
            }
            return;
        }
        if let Some(running) = &mut self.bible.indexing {
            // the running job may not cover these; let it finish and check again then
            running.then_search |= then_search;
            self.bible.index_again = true;
            return;
        }
        let (tx, rx) = channel();
        let total = ids.len();
        std::thread::spawn(move || {
            for (id, name) in ids {
                let _ = tx.send(Progress::Started(name.clone()));
                match Bible::load(&id) {
                    Some(b) => {
                        let idx = SearchIndex::build(&b);
                        match idx.save(&id) {
                            Ok(()) => {
                                let _ = tx.send(Progress::Finished);
                            }
                            Err(e) => {
                                let _ = tx.send(Progress::Failed(name, e.to_string()));
                            }
                        }
                    }
                    None => {
                        let _ = tx.send(Progress::Failed(name, "cannot be read".into()));
                    }
                }
            }
        });
        self.bible.indexing = Some(Indexing { rx, done: 0, total, current: String::new(), then_search });
    }

    /// Loads the index of the current bible if its language is enabled and the index is ready.
    pub fn load_current_index(&mut self) {
        self.bible.index = None;
        if let Some(b) = &self.bible.current
            && self.index_enabled(&b.meta.language)
        {
            self.bible.index = SearchIndex::load(b).map(|i| (b.meta.id.clone(), i));
        }
    }

    pub fn poll_indexing(&mut self, ctx: &egui::Context) {
        let Some(job) = &mut self.bible.indexing else { return };
        let mut finished = false;
        loop {
            match job.rx.try_recv() {
                Ok(Progress::Started(name)) => job.current = name,
                Ok(Progress::Finished) => job.done += 1,
                Ok(Progress::Failed(name, e)) => {
                    job.done += 1;
                    self.status = Some((tf("bible.index_failed", &[("name", &name), ("err", &e)]), std::time::Instant::now()));
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    finished = true;
                    break;
                }
            }
        }
        if !finished {
            ctx.request_repaint_after(std::time::Duration::from_millis(150));
            return;
        }
        let then_search = job.then_search;
        self.bible.indexing = None;
        self.load_current_index();
        if std::mem::take(&mut self.bible.index_again) {
            self.build_missing_indexes(then_search);
        } else if then_search {
            self.run_bible_search();
        }
    }

    /// A word search in the current bible: uses the index, makes it first, or asks.
    pub fn bible_word_search(&mut self, query: &str) {
        let Some(b) = &self.bible.current else { return };
        let language = b.meta.language.clone();
        let id = b.meta.id.clone();
        if self.settings.bible_index_languages.is_empty() {
            // first word search ever: ask which language to make the index for
            self.dialog = Some(Dialog::BibleIndexLanguage(language));
            return;
        }
        if !self.index_enabled(&language) {
            self.bible.info = tf("bible.index_off", &[("lang", &bible::language_name(&language))]);
            self.bible.offer_index = Some(language);
            return;
        }
        if self.bible.index.as_ref().is_none_or(|(i, _)| *i != id) {
            self.load_current_index();
        }
        match &self.bible.index {
            Some((_, idx)) => {
                let filter = self.bible.filter;
                let books = &self.bible.current.as_ref().map(|b| b.books.iter().map(|b| b.number).collect::<Vec<_>>()).unwrap_or_default();
                let hits = idx.search_where(query, 500, |h| books.get(h.book).is_some_and(|n| filter.accepts(*n)));
                self.bible.info = if hits.len() >= 500 {
                    tf("bible.found_text_many", &[("n", "500")])
                } else {
                    tf("bible.found_text", &[("n", &hits.len().to_string())])
                };
                self.bible.selected = vec![false; hits.len()];
                self.bible.results = hits;
            }
            None => {
                self.bible.info = t("bible.indexing");
                self.build_missing_indexes(true);
            }
        }
    }

    pub fn bible_index_dialog(&mut self, ui: &mut egui::Ui, suggested: &str, close: &mut bool) {
        let languages = self.bible_languages();
        if self.bible.index_choice.is_none() {
            self.bible.index_choice = Some(suggested.to_string());
        }
        ui.heading(t("bible.index_title"));
        ui.label(t("bible.index_text"));
        ui.add_space(6.0);
        let choice = self.bible.index_choice.get_or_insert_with(String::new);
        for (lang, list) in &languages {
            let names: Vec<&str> = list.iter().map(|b| b.name.as_str()).collect();
            ui.radio_value(choice, lang.clone(), format!("{} — {}", bible::language_name(lang), names.join(", ")));
        }
        ui.weak(t("bible.index_more"));
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            if ui.button(t("bible.index_create")).clicked() {
                let lang = self.bible.index_choice.take().unwrap_or_default();
                *close = true;
                self.settings.bible_index_languages.push(lang);
                self.save_settings();
                self.bible.info = t("bible.indexing");
                self.build_missing_indexes(true);
            }
            if ui.button(t("cancel")).clicked() {
                self.bible.index_choice = None;
                *close = true;
            }
        });
    }

    /// Settings → Bible: languages with a word search index, and each bible's language.
    pub fn settings_bible(&mut self, ui: &mut egui::Ui) {
        ui.heading(t("settings.bible"));
        let languages = self.bible_languages();
        if languages.is_empty() {
            ui.weak(t("bible.no_bible"));
            return;
        }
        ui.label(t("settings.bible_index"));
        if let Some(job) = &self.bible.indexing {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label(tf(
                    "bible.indexing_progress",
                    &[("done", &job.done.to_string()), ("total", &job.total.to_string()), ("name", &job.current)],
                ));
            });
        }
        let mut toggle = None;
        let mut relabel = None;
        egui::Grid::new("bible_langs").num_columns(3).spacing([12.0, 6.0]).show(ui, |ui| {
            for (lang, list) in &languages {
                let mut on = self.index_enabled(lang);
                if ui.checkbox(&mut on, bible::language_name(lang)).on_hover_text(t("settings.bible_index_hint")).changed() {
                    toggle = Some((lang.clone(), on));
                }
                let indexed = list.iter().filter(|b| SearchIndex::exists(&b.id)).count();
                ui.weak(if self.index_enabled(lang) {
                    tf("settings.bible_indexed", &[("n", &indexed.to_string()), ("all", &list.len().to_string())])
                } else {
                    t("settings.bible_index_none")
                });
                ui.vertical(|ui| {
                    for b in list {
                        ui.horizontal(|ui| {
                            ui.label(format!("{} ({})", b.name, b.abbrev));
                            egui::ComboBox::from_id_salt(("bible_lang", &b.id)).selected_text(bible::language_name(&b.language)).show_ui(
                                ui,
                                |ui| {
                                    for l in bible::known_languages() {
                                        if ui.selectable_label(b.language == l, bible::language_name(l)).clicked() && b.language != l {
                                            relabel = Some((b.id.clone(), l.to_string()));
                                        }
                                    }
                                },
                            );
                        });
                    }
                });
                ui.end_row();
            }
        });
        if let Some((lang, on)) = toggle {
            self.set_index_language(&lang, on);
        }
        if let Some((id, lang)) = relabel {
            match Bible::set_language(&id, &lang) {
                Ok(()) => {
                    // its index (if any) belongs to the old language's setting
                    if !self.index_enabled(&lang) {
                        SearchIndex::delete(&id);
                    }
                    self.bible.bibles = bible::list_bibles();
                    if let Some(c) = &mut self.bible.current
                        && c.meta.id == id
                    {
                        c.meta.language = lang;
                    }
                    self.load_current_index();
                    self.build_missing_indexes(false);
                }
                Err(e) => self.error(e.to_string()),
            }
        }
    }
}
