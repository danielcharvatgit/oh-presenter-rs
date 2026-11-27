//! A bible passage in the service (Service tab, middle column): the translations and
//! languages shown on the output, the verse shown now, the books of the bible and the
//! chapters of the chosen book, and the chapter's text in one language (see diagram.odg, layout A).
//!
//! A verse clicked here is shown live right away. A verse of the passage selects its slide;
//! any other verse of the book is shown without changing the service item.

use super::{App, ItemRef, LIVE_COLOR};
use crate::bible::{Bible, BibleMeta};
use crate::i18n::{current_language, t};
use crate::model::*;
use eframe::egui::{self, RichText};
use std::sync::Arc;

/// What the bible controls of the selected service item show.
#[derive(Clone, Debug, Default)]
pub struct BibleNav {
    /// The service item (its id) this is for; another item starts over.
    pub item_id: String,
    /// The language whose translations are offered and whose text the verse list shows
    /// (`None` = the passage's main translation's language).
    pub focus_lang: Option<String>,
    /// The book the lists show (`None` = the book of the selected slide).
    pub book: Option<u32>,
    /// The chapter the lists show (`None` = the chapter of the selected slide).
    pub chapter: Option<u32>,
    /// A verse outside the passage that is live now: (book number, chapter, verse).
    pub outside: Option<(u32, u32, u32)>,
}

/// A verse of a bible by canonical book number, chapter and verse number.
pub fn find_verse(bible: &Bible, book: u32, chapter: u32, verse: u32) -> Option<&crate::bible::Verse> {
    let b = bible.books.iter().find(|b| b.number == book)?;
    let c = b.chapters.iter().find(|c| c.number == chapter)?;
    c.verses.iter().find(|v| v.number == verse)
}

impl App {
    /// A bible by id, loaded once (the service's passages use several translations).
    pub fn cached_bible(&mut self, id: &str) -> Option<Arc<Bible>> {
        if id.is_empty() {
            return None;
        }
        if let Some(b) = self.bible_cache.get(id) {
            return Some(b.clone());
        }
        let b = match &self.bible.current {
            Some(cur) if cur.meta.id == id => cur.clone(),
            _ => Bible::load(id)?,
        };
        let b = Arc::new(b);
        self.bible_cache.insert(id.to_string(), b.clone());
        Some(b)
    }

    /// The installed bible a passage is from: its id, or (older files) the bible with its
    /// abbreviation.
    pub fn passage_bible_id(&self, p: &Passage) -> Option<String> {
        if !p.bible.is_empty() && self.bible.bibles.iter().any(|b| b.id == p.bible) {
            return Some(p.bible.clone());
        }
        self.bible.bibles.iter().find(|b| !p.version.is_empty() && b.abbrev == p.version).map(|b| b.id.clone())
    }

    fn passage_of(&self, r: ItemRef) -> Option<Passage> {
        match &self.item(r)?.kind {
            ItemKind::Bible(p) => Some(p.clone()),
            _ => None,
        }
    }

    fn item_id(&self, r: ItemRef) -> String {
        match r {
            ItemRef::Service(i) => self.service.items.get(i).map(|it| it.id.clone()).unwrap_or_default(),
            ItemRef::Quick => "quick".into(),
        }
    }

    /// The bible controls' state for item `r` (a new one for another item).
    fn nav_for(&mut self, r: ItemRef) -> &mut BibleNav {
        let id = self.item_id(r);
        if self.bible_nav.item_id != id {
            self.bible_nav = BibleNav { item_id: id, ..Default::default() };
        }
        &mut self.bible_nav
    }

    /// The book number of a passage's verse (older files: found by the book's name).
    fn verse_book(&self, bible: &Bible, v: &PassageVerse) -> Option<u32> {
        if v.book_number > 0 {
            return Some(v.book_number);
        }
        bible.find_book(&v.book, &current_language()).map(|i| bible.books[i].number)
    }

    /// The (book, chapter, verse) shown as "now" for item `r`: the live verse outside the
    /// passage, else the first verse of the selected slide.
    pub fn bible_now(&mut self, r: ItemRef) -> Option<(u32, u32, u32)> {
        let p = self.passage_of(r)?;
        let bible = self.cached_bible(&self.passage_bible_id(&p)?)?;
        if let Some(o) = self.nav_for(r).outside {
            return Some(o);
        }
        let per = self.settings.verses_per_slide.max(1);
        let slide = if self.sel_item == Some(r) { self.sel_slide } else { 0 };
        let v = p.verses.get(slide * per).or(p.verses.first())?;
        Some((self.verse_book(&bible, v)?, v.chapter, v.verse))
    }

    /// The verse (book, chapter, verse) of item `r` in every shown translation, ready for a slide.
    fn passage_verse(&mut self, p: &Passage, book: u32, chapter: u32, verse: u32) -> Option<PassageVerse> {
        let main = self.cached_bible(&self.passage_bible_id(p)?)?;
        let text = find_verse(&main, book, chapter, verse)?.text.clone();
        let book_index = main.books.iter().position(|b| b.number == book)?;
        let extra = p
            .extra_bibles
            .iter()
            .map(|id| self.cached_bible(id).and_then(|b| find_verse(&b, book, chapter, verse).map(|v| v.text.clone())).unwrap_or_default())
            .collect();
        Some(PassageVerse { book: main.book_name(book_index, &current_language()), chapter, verse, text, book_number: book, extra })
    }

    /// Shows verse (book, chapter, verse) of the bible item `r` live: a verse of the passage
    /// selects its slide, another verse is shown without changing the item.
    pub fn bible_item_show_verse(&mut self, r: ItemRef, book: u32, chapter: u32, verse: u32) -> bool {
        let Some(p) = self.passage_of(r) else { return false };
        let Some(bible) = self.passage_bible_id(&p).and_then(|id| self.cached_bible(&id)) else { return false };
        let per = self.settings.verses_per_slide.max(1);
        let inside = p.verses.iter().position(|v| v.chapter == chapter && v.verse == verse && self.verse_book(&bible, v) == Some(book));
        if let Some(i) = inside {
            self.sel_item = Some(r);
            self.sel_slide = i / per;
            self.scroll_pending = true;
            self.bible_nav.outside = None;
            self.bible_nav.book = None;
            self.bible_nav.chapter = None;
            self.go_live();
            return true;
        }
        let Some(pv) = self.passage_verse(&p, book, chapter, verse) else { return false };
        let background = self.item(r).and_then(|it| self.text_background_of(it));
        let scene = self.bible_scene(&p, &[pv], background);
        self.sel_item = Some(r);
        self.set_live(scene, None);
        let nav = self.nav_for(r);
        nav.outside = Some((book, chapter, verse));
        nav.book = Some(book);
        nav.chapter = Some(chapter);
        true
    }

    /// The live verse outside the passage of the selected bible item, if one is live.
    pub fn bible_outside_live(&self) -> Option<(ItemRef, (u32, u32, u32))> {
        let r = self.sel_item?;
        let o = self.bible_nav.outside?;
        (self.live_pos.is_none() && self.bible_nav.item_id == self.item_id(r)).then_some((r, o))
    }

    /// Next / previous verse of the book after a verse outside the passage (the slide keys).
    pub fn bible_step_outside(&mut self, forward: bool) -> bool {
        let Some((r, (book, chapter, verse))) = self.bible_outside_live() else { return false };
        let Some(p) = self.passage_of(r) else { return false };
        let Some(bible) = self.passage_bible_id(&p).and_then(|id| self.cached_bible(&id)) else { return false };
        let Some(b) = bible.books.iter().find(|b| b.number == book) else { return false };
        // every verse of the book in order
        let all: Vec<(u32, u32)> = b.chapters.iter().flat_map(|c| c.verses.iter().map(move |v| (c.number, v.number))).collect();
        let Some(at) = all.iter().position(|x| *x == (chapter, verse)) else { return false };
        let next = if forward { all.get(at + 1) } else { at.checked_sub(1).and_then(|i| all.get(i)) };
        match next {
            Some(&(c, v)) => self.bible_item_show_verse(r, book, c, v),
            // the end of the book: stays
            None => true,
        }
    }

    /// Sets the translations shown by bible item `r` (bible ids, the main one first): every
    /// verse of the passage gets their texts. Returns false when nothing could be set.
    pub fn bible_item_set_shown(&mut self, r: ItemRef, shown: Vec<String>) -> bool {
        let Some(mut p) = self.passage_of(r) else { return false };
        let mut shown: Vec<String> = shown.into_iter().filter(|id| self.bible.bibles.iter().any(|b| &b.id == id)).collect();
        shown.dedup();
        if shown.is_empty() || shown == p.shown_bibles() {
            return false;
        }
        let Some(old_main) = self.passage_bible_id(&p).and_then(|id| self.cached_bible(&id)) else { return false };
        let Some(main) = self.cached_bible(&shown[0]) else { return false };
        let extras: Vec<Option<Arc<Bible>>> = shown[1..].iter().map(|id| self.cached_bible(id)).collect();
        let lang = current_language();
        let mut hits = Vec::new();
        for v in &mut p.verses {
            let Some(book) = self.verse_book(&old_main, v) else { continue };
            v.book_number = book;
            if v.verse == 0 {
                continue;
            }
            if let Some(bi) = main.books.iter().position(|b| b.number == book) {
                let b = &main.books[bi];
                if let Some(ci) = b.chapters.iter().position(|c| c.number == v.chapter)
                    && let Some(vi) = b.chapters[ci].verses.iter().position(|x| x.number == v.verse)
                {
                    v.text = b.chapters[ci].verses[vi].text.clone();
                    hits.push(crate::bible::Hit { book: bi, chapter: ci, verse: vi });
                }
                v.book = main.book_name(bi, &lang);
            }
            v.extra = extras
                .iter()
                .map(|b| b.as_ref().and_then(|b| find_verse(b, book, v.chapter, v.verse)).map(|x| x.text.clone()).unwrap_or_default())
                .collect();
        }
        if hits.len() == p.verses.len() && !hits.is_empty() {
            p.reference = main.format_reference(&hits, &lang);
        }
        p.bible = main.meta.id.clone();
        p.version = main.meta.abbrev.clone();
        p.extra_versions = extras.iter().map(|b| b.as_ref().map(|b| b.meta.abbrev.clone()).unwrap_or_default()).collect();
        p.extra_bibles = shown[1..].to_vec();
        match r {
            ItemRef::Service(i) => {
                if let Some(it) = self.service.items.get_mut(i) {
                    it.kind = ItemKind::Bible(p);
                    self.service_dirty = true;
                }
            }
            ItemRef::Quick => {
                if let Some(q) = &mut self.quick {
                    q.kind = ItemKind::Bible(p);
                }
            }
        }
        // what is live of this item shows the new translations right away (a blanked output
        // stays blank)
        let blank = self.blank;
        if let Some((o_book, o_ch, o_v)) = self.bible_outside_live().filter(|(x, _)| *x == r).map(|(_, o)| o) {
            self.bible_item_show_verse(r, o_book, o_ch, o_v);
        } else if let Some((lr, ls)) = self.live_pos
            && lr == r
            && let Some(s) = self.slides_of(r).get(ls)
        {
            self.set_live(s.scene.clone(), Some((lr, ls)));
        }
        if blank && !self.blank {
            self.blank = true;
            self.broadcast();
        }
        true
    }

    /// The language of a bible (by id).
    fn bible_language(&self, id: &str) -> Option<String> {
        self.bible.bibles.iter().find(|b| b.id == id).map(|b| b.language.clone())
    }

    /// Turns language `lang` on (its first or given translation is added) or off for item `r`.
    pub fn bible_item_language(&mut self, r: ItemRef, lang: &str, on: bool) -> bool {
        let Some(p) = self.passage_of(r) else { return false };
        let mut shown = p.shown_bibles();
        if shown.first().is_some_and(String::is_empty) {
            match self.passage_bible_id(&p) {
                Some(id) => shown[0] = id,
                None => return false,
            }
        }
        let has = shown.iter().position(|id| self.bible_language(id).as_deref() == Some(lang));
        match (on, has) {
            (true, None) => {
                let Some(b) = self.bible.bibles.iter().find(|b| b.language == lang) else { return false };
                shown.push(b.id.clone());
            }
            (false, Some(i)) if shown.len() > 1 => {
                shown.remove(i);
            }
            _ => return false,
        }
        self.bible_item_set_shown(r, shown)
    }

    /// Shows translation `id` in item `r`: it replaces the translation of its language, or is
    /// added when its language is not shown yet.
    pub fn bible_item_translation(&mut self, r: ItemRef, id: &str) -> bool {
        let Some(p) = self.passage_of(r) else { return false };
        let Some(lang) = self.bible_language(id) else { return false };
        let mut shown = p.shown_bibles();
        if shown.first().is_some_and(String::is_empty) {
            match self.passage_bible_id(&p) {
                Some(main) => shown[0] = main,
                None => return false,
            }
        }
        match shown.iter().position(|s| self.bible_language(s).as_deref() == Some(lang.as_str())) {
            Some(i) => shown[i] = id.to_string(),
            None => shown.push(id.to_string()),
        }
        self.nav_for(r).focus_lang = Some(lang);
        self.bible_item_set_shown(r, shown)
    }

    /// The bible controls in the middle column (instead of the slide pictures). Returns
    /// false when the passage's bible is not installed (the slides are shown then).
    pub(super) fn bible_item_panel(&mut self, ui: &mut egui::Ui, r: ItemRef) -> bool {
        let Some(p) = self.passage_of(r) else { return false };
        let Some(main_id) = self.passage_bible_id(&p) else { return false };
        let Some(main) = self.cached_bible(&main_id) else { return false };
        let Some(now) = self.bible_now(r) else { return false };
        let lang = current_language();
        let languages: Vec<(String, Vec<BibleMeta>)> = self.bible_languages();
        let mut shown = p.shown_bibles();
        shown[0] = main_id.clone();
        let shown_langs: Vec<String> = shown.iter().filter_map(|id| self.bible_language(id)).collect();
        let main_lang = shown_langs.first().cloned().unwrap_or_default();
        let focus = self.nav_for(r).focus_lang.clone().filter(|l| languages.iter().any(|(x, _)| x == l)).unwrap_or(main_lang);
        // the translation of the focused language: the shown one, else its first
        let focus_bible = shown
            .iter()
            .find(|id| self.bible_language(id).as_deref() == Some(focus.as_str()))
            .cloned()
            .or_else(|| languages.iter().find(|(l, _)| *l == focus).and_then(|(_, list)| list.first()).map(|b| b.id.clone()));
        let list_bible = focus_bible.as_deref().and_then(|id| self.cached_bible(id)).unwrap_or_else(|| main.clone());

        let mut set_translation = None;
        let mut set_language: Option<(String, bool)> = None;
        let mut set_focus = None;
        let mut set_book = None;
        let mut set_chapter = None;
        let mut show_verse = None;
        let (now_book, now_chapter, now_verse) = now;
        // the book and chapter the lists show
        let (sel_book, chapter) = {
            let nav = self.nav_for(r);
            match nav.book {
                Some(b) if b != now_book => (b, nav.chapter.unwrap_or(1)),
                _ => (now_book, nav.chapter.unwrap_or(now_chapter)),
            }
        };
        let outside = self.bible_outside_live().map(|(_, o)| o);
        let live_verse = match (outside, self.live_pos) {
            (Some(o), _) => Some(o),
            (None, Some((lr, ls))) if lr == r => {
                let per = self.settings.verses_per_slide.max(1);
                p.verses.get(ls * per).and_then(|v| Some((self.verse_book(&main, v)?, v.chapter, v.verse)))
            }
            _ => None,
        };
        let in_passage = |book: u32, c: u32, v: u32| {
            p.verses.iter().any(|x| x.chapter == c && x.verse == v && (x.book_number == 0 || x.book_number == book))
        };

        // top: translations (left), languages and the verse shown now (right)
        ui.horizontal_top(|ui| {
            ui.vertical(|ui| {
                ui.set_width(170.0);
                ui.label(RichText::new(t("bible_item.translation")).strong()).on_hover_text(t("bible_item.translation_hint"));
                if let Some((_, list)) = languages.iter().find(|(l, _)| *l == focus) {
                    for b in list {
                        let on = shown.contains(&b.id);
                        if ui.selectable_label(on, format!("{} ({})", b.abbrev, b.name)).on_hover_text(&b.name).clicked() && !on {
                            set_translation = Some(b.id.clone());
                        }
                    }
                }
            });
            // (no separator here: in a row it would take the whole height of the panel)
            ui.add_space(16.0);
            ui.vertical(|ui| {
                ui.label(RichText::new(t("bible_item.languages")).strong()).on_hover_text(t("bible_item.languages_hint"));
                ui.horizontal_wrapped(|ui| {
                    for (l, _) in &languages {
                        let mut on = shown_langs.contains(l);
                        let was = on;
                        if ui.checkbox(&mut on, "").on_hover_text(t("bible_item.show_language")).changed() && on != was {
                            set_language = Some((l.clone(), on));
                        }
                        if ui.selectable_label(*l == focus, crate::bible::language_name(l)).clicked() {
                            set_focus = Some(l.clone());
                        }
                        ui.add_space(8.0);
                    }
                });
                ui.add_space(4.0);
                // the verse shown now
                let book_index = main.books.iter().position(|b| b.number == now_book);
                let name = book_index.map(|i| main.book_name(i, &lang)).unwrap_or_default();
                let sep = if lang == "cs" { "," } else { ":" };
                let text = find_verse(&list_bible, now_book, now_chapter, now_verse).map(|v| v.text.clone()).unwrap_or_default();
                egui::Frame::group(ui.style()).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    let is_live = live_verse == Some(now);
                    let head = RichText::new(format!("{name} {now_chapter}{sep}{now_verse}")).strong();
                    ui.horizontal_wrapped(|ui| {
                        ui.label(if is_live { head.color(LIVE_COLOR) } else { head }).on_hover_text(t("bible_item.now_hint"));
                        ui.label(text);
                    });
                });
            });
        });
        ui.separator();

        // bottom: books, chapters of the chosen book, the chapter's text in the focused language
        let Some(book) = main.books.iter().find(|b| b.number == sel_book).or_else(|| main.books.iter().find(|b| b.number == now_book))
        else {
            return true;
        };
        let sel_book = book.number;
        // room for the notes under it
        let height = (ui.available_height() - 36.0).max(120.0);
        ui.horizontal_top(|ui| {
            ui.vertical(|ui| {
                ui.set_width(130.0);
                ui.label(RichText::new(t("bible_item.book")).strong());
                egui::ScrollArea::vertical().id_salt("bi_books").max_height(height - 24.0).auto_shrink([false, false]).show(ui, |ui| {
                    for (i, b) in main.books.iter().enumerate() {
                        let mut label = RichText::new(main.book_name(i, &lang));
                        if b.number == now_book {
                            label = label.strong();
                        }
                        let resp = ui.selectable_label(b.number == sel_book, label);
                        if b.number == sel_book && self.scroll_pending {
                            resp.scroll_to_me(Some(egui::Align::Center));
                        }
                        if resp.clicked() && b.number != sel_book {
                            set_book = Some((b.number, b.chapters.first().map(|c| c.number).unwrap_or(1)));
                        }
                    }
                });
            });
            ui.vertical(|ui| {
                ui.set_width(64.0);
                ui.label(RichText::new(t("bible_item.chapter")).strong());
                egui::ScrollArea::vertical().id_salt("bi_chapters").max_height(height - 24.0).auto_shrink([false, false]).show(ui, |ui| {
                    for c in &book.chapters {
                        let mut label = RichText::new(c.number.to_string());
                        if sel_book == now_book && c.number == now_chapter {
                            label = label.strong();
                        }
                        let resp = ui.selectable_label(c.number == chapter, label);
                        if c.number == chapter && self.scroll_pending {
                            resp.scroll_to_me(Some(egui::Align::Center));
                        }
                        if resp.clicked() {
                            set_chapter = Some(c.number);
                        }
                    }
                });
            });
            ui.separator();
            ui.vertical(|ui| {
                let focus_name =
                    focus_bible.as_deref().and_then(|id| self.bible.bibles.iter().find(|b| b.id == id)).map(|b| b.abbrev.clone());
                ui.label(
                    RichText::new(format!(
                        "{} {}{}",
                        main.book_name(main.books.iter().position(|b| b.number == sel_book).unwrap_or(0), &lang),
                        chapter,
                        focus_name.map(|n| format!("  ({n})")).unwrap_or_default()
                    ))
                    .strong(),
                );
                egui::ScrollArea::vertical().id_salt("bi_text").max_height(height - 24.0).auto_shrink([false, false]).show(ui, |ui| {
                    let verses = list_bible
                        .books
                        .iter()
                        .find(|b| b.number == sel_book)
                        .and_then(|b| b.chapters.iter().find(|c| c.number == chapter))
                        .map(|c| c.verses.clone())
                        .unwrap_or_default();
                    for v in &verses {
                        let here = (sel_book, chapter, v.number);
                        let mut job = egui::text::LayoutJob::default();
                        let num_color = if live_verse == Some(here) {
                            LIVE_COLOR
                        } else if in_passage(sel_book, chapter, v.number) {
                            ui.visuals().strong_text_color()
                        } else {
                            ui.visuals().weak_text_color()
                        };
                        job.append(&format!("{}  ", v.number), 0.0, egui::TextFormat { color: num_color, ..Default::default() });
                        job.append(&v.text, 0.0, egui::TextFormat { color: ui.visuals().text_color(), ..Default::default() });
                        job.wrap.max_width = ui.available_width() - 10.0;
                        let resp = ui.add(egui::Button::selectable(now == here, job).wrap()).on_hover_text(t("bible_item.verse_hint"));
                        if now == here && self.scroll_pending {
                            resp.scroll_to_me(Some(egui::Align::Center));
                        }
                        if resp.clicked() {
                            show_verse = Some(here);
                        }
                    }
                });
            });
        });
        self.scroll_pending = false;

        if let Some(l) = set_focus {
            self.nav_for(r).focus_lang = Some(l);
        }
        if let Some((b, c)) = set_book {
            let nav = self.nav_for(r);
            nav.book = Some(b);
            nav.chapter = Some(c);
            self.scroll_pending = true;
        }
        if let Some(c) = set_chapter {
            let nav = self.nav_for(r);
            nav.book = Some(sel_book);
            nav.chapter = Some(c);
        }
        if let Some(id) = set_translation {
            self.bible_item_translation(r, &id);
        }
        if let Some((l, on)) = set_language {
            if on {
                self.nav_for(r).focus_lang = Some(l.clone());
            }
            self.bible_item_language(r, &l, on);
        }
        if let Some((b, c, v)) = show_verse {
            self.bible_item_show_verse(r, b, c, v);
        }
        true
    }
}
