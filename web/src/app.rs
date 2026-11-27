//! The web edition's state and what its buttons, keys and tools do. The drawing is in
//! [`crate::ui`]. Data is kept in this browser: settings, songs, the service and the list of
//! bibles in local storage, bibles and pictures in IndexedDB (see [`crate::js`]).

use crate::bible::{Bible, BibleMeta, Hit, SearchIndex};
use crate::i18n::{t, tf};
use crate::model::*;
use crate::output::{FromOutput, OutputMsg};
use crate::render::TextureCache;
use crate::shortcuts::{Action, Shortcuts};
use eframe::egui;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::cell::RefCell;
use std::rc::Rc;

pub const KEY_SETTINGS: &str = "oh-presenter.web.settings";
pub const KEY_SONGS: &str = "oh-presenter.web.songs";
pub const KEY_SERVICE: &str = "oh-presenter.web.service";
pub const KEY_BIBLES: &str = "oh-presenter.web.bibles";
/// The first web version's queue of text slides (imported as text items once).
pub const KEY_V1: &str = "oh-presenter.web.v1";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tab {
    Service,
    Songs,
    Bible,
    Settings,
}

/// The tabs in the order shown (from left to right), their label and shortcut.
pub const TABS: [(Tab, &str, Action); 4] = [
    (Tab::Service, "tab.service", Action::TabService),
    (Tab::Songs, "tab.songs", Action::TabSongs),
    (Tab::Bible, "tab.bible", Action::TabBible),
    (Tab::Settings, "tab.settings", Action::TabSettings),
];

/// A service item, or the quick bible passage (shown live from the Bible tab, not in the service).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ItemRef {
    Service(usize),
    Quick,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// Empty: the browser's language.
    pub language: String,
    pub dark_mode: bool,
    pub text_style: TextStyle,
    /// Bible slides show their reference (songs and texts: `text_style.show_footer`).
    pub show_bible_reference: bool,
    pub bible_reference: ReferenceParts,
    pub verses_per_slide: usize,
    pub live_follows_selection: bool,
    /// Number keys 1–9 jump to verse N of a song.
    pub number_keys_jump_to_verse: bool,
    /// The bible chosen in the Bible tab.
    pub bible: Option<String>,
    pub shortcuts: Shortcuts,
    pub alert_style: AlertStyle,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            language: String::new(),
            dark_mode: true,
            text_style: TextStyle::default(),
            show_bible_reference: true,
            bible_reference: ReferenceParts::default(),
            verses_per_slide: 1,
            live_follows_selection: true,
            number_keys_jump_to_verse: true,
            bible: None,
            shortcuts: Shortcuts::default(),
            alert_style: AlertStyle::default(),
        }
    }
}

/// The marks of song parts and their keys (as the program's defaults): V verse, C chorus, …
pub const MARKS: &[(&str, egui::Key)] = &[
    ("V", egui::Key::V),
    ("C", egui::Key::C),
    ("P", egui::Key::P),
    ("B", egui::Key::B),
    ("I", egui::Key::I),
    ("E", egui::Key::E),
    ("O", egui::Key::O),
];

#[derive(Default, Serialize, Deserialize)]
#[serde(default)]
pub struct SavedService {
    pub service: Service,
    /// File name it was opened from / is saved as.
    pub name: String,
}

#[derive(Default)]
pub struct BibleState {
    pub current: Option<Bible>,
    /// The id being loaded from IndexedDB.
    pub loading: Option<String>,
    pub query: String,
    pub results: Vec<Hit>,
    pub selected: Vec<bool>,
    pub info: String,
    /// Word search index of the current bible (made at the first word search).
    pub index: Option<SearchIndex>,
    pub focus_search: bool,
}

pub enum Dialog {
    Rename(String),
    Alert(String),
    AddText { title: String, text: String },
    NewService,
    DeleteSong(String),
    DeleteBible(String),
    Message(String),
}

/// Where a chosen file goes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Purpose {
    Service,
    Song,
    Bible,
    Image,
}

/// Results of the browser's asynchronous work, taken in the next frame.
pub enum Loaded {
    Bible(String, Result<Bible, String>),
    File(Purpose, String, Vec<u8>),
    Media(String, Vec<u8>),
    Error(String),
}

pub struct WebApp {
    pub settings: Settings,
    pub songs: Vec<Song>,
    pub service: Service,
    pub service_name: String,
    pub bibles: Vec<BibleMeta>,

    pub tab: Tab,
    pub sel_item: Option<ItemRef>,
    pub sel_slide: usize,
    pub live: Scene,
    pub live_pos: Option<(ItemRef, usize)>,
    pub blank: bool,
    /// Freeze: the output keeps this picture while slides change.
    pub frozen: Option<Scene>,
    pub quick: Option<ServiceItem>,
    /// The alert running across the output, since (ms).
    pub alert: Option<(Alert, f64)>,

    /// The song in the editor (Songs tab), and whether it is the library's.
    pub song: Song,
    pub song_search: String,
    pub focus_song_search: bool,
    pub bible: BibleState,

    pub tex: TextureCache,
    pub notice: Option<(String, f64)>,
    pub dialog: Option<Dialog>,
    /// Waiting for a key to bind to this action (Settings › Keyboard shortcuts).
    pub capture: Option<Action>,
    pub inbox: Rc<RefCell<Vec<Loaded>>>,
    /// The WebMCP tools the browser took (0: no WebMCP in this browser).
    pub tools: u32,
    last_sent: String,
    saved: [String; 4],
}

fn load<T: Default + for<'de> Deserialize<'de>>(key: &str) -> T {
    crate::js::local_get(key).and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
}

impl WebApp {
    pub fn new(ctx: &egui::Context) -> Self {
        let settings: Settings = load(KEY_SETTINGS);
        let saved: SavedService = load(KEY_SERVICE);
        let mut app = Self {
            songs: load(KEY_SONGS),
            service: saved.service,
            service_name: saved.name,
            bibles: load(KEY_BIBLES),
            settings,
            tab: Tab::Service,
            sel_item: None,
            sel_slide: 0,
            live: Scene::Blank,
            live_pos: None,
            blank: false,
            frozen: None,
            quick: None,
            alert: None,
            song: Song::new(),
            song_search: String::new(),
            focus_song_search: false,
            bible: BibleState::default(),
            tex: TextureCache::default(),
            notice: None,
            dialog: None,
            capture: None,
            inbox: Rc::default(),
            tools: 0,
            last_sent: String::new(),
            saved: Default::default(),
        };
        // what is stored counts as saved; what changes below is written in the first frame
        app.saved = app.snapshot();
        app.import_v1();
        let lang = if app.settings.language.is_empty() { crate::i18n::browser_language() } else { app.settings.language.clone() };
        crate::i18n::set_language(&lang);
        set_theme(ctx, app.settings.dark_mode);
        // new actions of later versions get their keys
        for a in Action::ALL {
            app.settings.shortcuts.map.entry(*a).or_insert_with(|| a.defaults());
        }
        crate::js::ask_persistent_storage();
        app.tools = crate::js::register_tools(&crate::commands::tools_json());
        if let Some(id) = app.settings.bible.clone() {
            app.load_bible(&id);
        }
        app.load_service_media();
        app
    }

    /// The text slides of the first web version become text items of the service.
    fn import_v1(&mut self) {
        #[derive(Deserialize)]
        struct V1Slide {
            title: String,
            text: String,
        }
        #[derive(Deserialize)]
        struct V1 {
            slides: Vec<V1Slide>,
        }
        let Some(v1) = crate::js::local_get(KEY_V1).and_then(|s| serde_json::from_str::<V1>(&s).ok()) else { return };
        if self.service.items.is_empty() {
            for s in v1.slides {
                self.service.items.push(ServiceItem::new(ItemKind::Text { title: s.title, text: s.text }));
            }
        }
        let _ = crate::js::local_set(KEY_V1, "{\"slides\":[],\"imported\":true}");
    }

    // -----------------------------------------------------------------------
    // Saving in the browser

    fn snapshot(&self) -> [String; 4] {
        let service = SavedService { service: self.service.clone(), name: self.service_name.clone() };
        [
            serde_json::to_string(&self.settings).unwrap_or_default(),
            serde_json::to_string(&self.songs).unwrap_or_default(),
            serde_json::to_string(&service).unwrap_or_default(),
            serde_json::to_string(&self.bibles).unwrap_or_default(),
        ]
    }

    /// Writes what changed into local storage (every frame; only changes are written).
    pub fn save_changes(&mut self) {
        let now = self.snapshot();
        for (i, key) in [KEY_SETTINGS, KEY_SONGS, KEY_SERVICE, KEY_BIBLES].iter().enumerate() {
            if now[i] != self.saved[i] {
                match crate::js::local_set(key, &now[i]) {
                    Ok(()) => self.saved[i] = now[i].clone(),
                    Err(e) => {
                        // full: tell once, try again with the next change
                        self.saved[i] = now[i].clone();
                        self.error(tf("storage.failed", &[("error", &e)]));
                    }
                }
            }
        }
    }

    pub fn notify(&mut self, text: impl Into<String>) {
        self.notice = Some((text.into(), crate::js::now_ms()));
    }

    pub fn error(&mut self, text: impl Into<String>) {
        self.dialog = Some(Dialog::Message(text.into()));
    }

    // -----------------------------------------------------------------------
    // Asynchronous work (files, IndexedDB)

    pub fn choose_file(&mut self, purpose: Purpose) {
        let accept = match purpose {
            Purpose::Service => ".ohsvc",
            Purpose::Song => ".xml,.txt,.cho,.chordpro,.chopro,.crd,.pro",
            Purpose::Bible => ".xml,.txt,.osis",
            Purpose::Image => "image/*",
        };
        let inbox = self.inbox.clone();
        crate::js::spawn(async move {
            if let Some((name, bytes)) = crate::js::choose_file(accept).await {
                inbox.borrow_mut().push(Loaded::File(purpose, name, bytes));
            }
        });
    }

    pub fn load_bible(&mut self, id: &str) {
        if self.bible.current.as_ref().is_some_and(|b| b.meta.id == id) {
            return;
        }
        self.bible.loading = Some(id.to_string());
        let (inbox, id) = (self.inbox.clone(), id.to_string());
        crate::js::spawn(async move {
            let r = match crate::js::db_get(&format!("bible:{id}")).await {
                Ok(Some(text)) => serde_json::from_str::<Bible>(&text).map_err(|e| e.to_string()),
                Ok(None) => Err(t("bible.missing")),
                Err(e) => Err(e),
            };
            inbox.borrow_mut().push(Loaded::Bible(id, r));
        });
    }

    /// Loads the pictures of the service from IndexedDB.
    pub fn load_service_media(&mut self) {
        for m in crate::service_file::service_media(&self.service) {
            if self.tex.media.contains_key(&m) {
                continue;
            }
            let inbox = self.inbox.clone();
            crate::js::spawn(async move {
                if let Ok(Some(bytes)) = crate::js::db_get_bytes(&format!("media:{m}")).await {
                    inbox.borrow_mut().push(Loaded::Media(m, bytes));
                }
            });
        }
    }

    fn store_media(&mut self, name: &str, bytes: Vec<u8>) {
        let (key, data, inbox) = (format!("media:{name}"), bytes.clone(), self.inbox.clone());
        crate::js::spawn(async move {
            if let Err(e) = crate::js::db_set_bytes(&key, &data).await {
                inbox.borrow_mut().push(Loaded::Error(tf("storage.failed", &[("error", &e)])));
            }
        });
        self.tex.add_media(name, bytes);
    }

    pub fn take_loaded(&mut self) {
        let loaded: Vec<Loaded> = self.inbox.borrow_mut().drain(..).collect();
        for l in loaded {
            match l {
                Loaded::Bible(id, r) => {
                    if self.bible.loading.as_deref() == Some(id.as_str()) {
                        self.bible.loading = None;
                    }
                    match r {
                        Ok(b) => self.set_bible(b),
                        Err(e) => self.notify(tf("bible.load_failed", &[("error", &e)])),
                    }
                }
                Loaded::Media(name, bytes) => self.tex.add_media(&name, bytes),
                Loaded::Error(e) => self.error(e),
                Loaded::File(purpose, name, bytes) => self.open_file(purpose, &name, &bytes),
            }
        }
    }

    /// A file the user chose (also used by tests).
    pub fn open_file(&mut self, purpose: Purpose, name: &str, bytes: &[u8]) {
        match purpose {
            Purpose::Service => match crate::service_file::read_service(bytes) {
                Ok((service, media)) => {
                    for (m, b) in media {
                        self.store_media(&m, b);
                    }
                    self.service = service;
                    self.service_name = name.to_string();
                    self.sel_item = None;
                    self.live_pos = None;
                    self.load_service_media();
                    self.notify(tf("service.opened", &[("name", name), ("n", &self.service.items.len().to_string())]));
                }
                Err(e) => self.error(tf("service.open_failed", &[("error", &e)])),
            },
            Purpose::Song => match crate::service_file::import_song(name, bytes) {
                Ok(song) => {
                    let title = song.title.clone();
                    self.songs.push(song.clone());
                    self.songs.sort_by_key(|s| crate::storage::fold(&s.title));
                    self.song = song;
                    self.tab = Tab::Songs;
                    self.notify(tf("songs.imported", &[("title", &title)]));
                }
                Err(e) => self.error(tf("songs.import_failed", &[("error", &e)])),
            },
            Purpose::Bible => match crate::service_file::import_bible(name, bytes) {
                Ok(b) => {
                    let (key, text, inbox) = (format!("bible:{}", b.meta.id), serde_json::to_string(&b).unwrap(), self.inbox.clone());
                    crate::js::spawn(async move {
                        if let Err(e) = crate::js::db_set(&key, &text).await {
                            inbox.borrow_mut().push(Loaded::Error(tf("storage.failed", &[("error", &e)])));
                        }
                    });
                    self.bibles.retain(|m| m.id != b.meta.id);
                    self.bibles.push(b.meta.clone());
                    self.notify(tf("bible.imported", &[("name", &b.meta.name), ("n", &b.verse_count().to_string())]));
                    self.set_bible(b);
                    self.tab = Tab::Bible;
                }
                Err(e) => self.error(tf("bible.import_failed", &[("error", &e)])),
            },
            Purpose::Image => match crate::service_file::image_design(name, bytes) {
                Ok((media, design)) => {
                    self.store_media(&media, bytes.to_vec());
                    self.add_to_service(ItemKind::Design(design));
                }
                Err(e) => self.error(tf("service.image_failed", &[("error", &e)])),
            },
        }
    }

    pub fn save_service_file(&mut self) {
        let tex = &self.tex;
        match crate::service_file::write_service(&self.service, &|m| tex.media.get(m).cloned()) {
            Ok(bytes) => {
                let mut name = if self.service_name.is_empty() { t("service.file_name") } else { self.service_name.clone() };
                if !name.ends_with(".ohsvc") {
                    name.push_str(".ohsvc");
                }
                crate::js::save_file(&name, &bytes, "application/zip");
                self.service_name = name;
            }
            Err(e) => self.error(e),
        }
    }

    pub fn set_bible(&mut self, b: Bible) {
        self.settings.bible = Some(b.meta.id.clone());
        self.bible.current = Some(b);
        self.bible.index = None;
        self.bible.results.clear();
        self.bible.selected.clear();
        if !self.bible.query.trim().is_empty() {
            self.run_bible_search();
        }
    }

    pub fn delete_bible(&mut self, id: &str) {
        self.bibles.retain(|m| m.id != id);
        if self.bible.current.as_ref().is_some_and(|b| b.meta.id == id) {
            self.bible = BibleState::default();
        }
        if self.settings.bible.as_deref() == Some(id) {
            self.settings.bible = self.bibles.first().map(|m| m.id.clone());
            if let Some(next) = self.settings.bible.clone() {
                self.load_bible(&next);
            }
        }
        let key = format!("bible:{id}");
        crate::js::spawn(async move {
            let _ = crate::js::db_del(&key).await;
        });
    }

    // -----------------------------------------------------------------------
    // Slides

    fn text_background_of(&self, item: &ServiceItem) -> Option<CanvasDoc> {
        item.background
            .as_ref()
            .or(self.service.background.as_ref())
            .map(|pic| CanvasDoc { background: self.settings.text_style.background, objects: vec![pic.object()] })
    }

    /// The reference under bible verses, with the parts chosen in the settings.
    pub fn bible_footer(&self, p: &Passage, chunk: &[PassageVerse]) -> String {
        let parts = self.settings.bible_reference;
        let (Some(first), Some(last)) = (chunk.first(), chunk.last()) else { return String::new() };
        let sep = if crate::i18n::current_language() == "cs" { "," } else { ":" };
        let reference = if first.verse == 0 {
            if parts.book || parts.chapter || parts.verse { p.reference.clone() } else { String::new() }
        } else {
            verse_reference(&first.book, (first.chapter, first.verse), (last.chapter, last.verse), &parts, sep)
        };
        let versions: Vec<&str> =
            std::iter::once(p.version.as_str()).chain(p.extra_versions.iter().map(String::as_str)).filter(|v| !v.is_empty()).collect();
        match (parts.translation && !versions.is_empty(), reference.is_empty()) {
            (true, true) => versions.join(" / "),
            (true, false) => format!("{reference} ({})", versions.join(" / ")),
            (false, _) => reference,
        }
    }

    /// The slides of an item (as the program makes them).
    pub fn item_slides(&self, item: &ServiceItem) -> Vec<Slide> {
        let style = self.settings.text_style.clone();
        let background = self.text_background_of(item);
        let scene = |texts: Vec<String>, footer: String, layout: ColumnLayout, style: &TextStyle| {
            let mut texts = texts;
            let text = if texts.is_empty() { String::new() } else { texts.remove(0) };
            Scene::Text(TextScene { text, extra: texts, layout, footer, style: style.clone(), background: background.clone() })
        };
        match &item.kind {
            ItemKind::Song(song) => {
                let shown = song.shown_columns();
                let mut out = Vec::new();
                for part in song.ordered_parts() {
                    let pages = part.column_pages(&shown);
                    let n = pages.len();
                    for (i, page) in pages.into_iter().enumerate() {
                        let label = if n > 1 { format!("{} ({}/{})", part.code(), i + 1, n) } else { part.code() };
                        out.push(Slide {
                            label,
                            tag: Some(part.tag.clone()),
                            number: part.number,
                            scene: scene(page, song.footer(), song.column_layout, &style),
                        });
                    }
                }
                out
            }
            ItemKind::Bible(p) => {
                // the reference has its own option (songs and texts: text_style.show_footer)
                let bible_style = TextStyle { show_footer: self.settings.show_bible_reference, ..style.clone() };
                let sep = if crate::i18n::current_language() == "cs" { "," } else { ":" };
                p.verses
                    .chunks(self.settings.verses_per_slide.max(1))
                    .map(|chunk| {
                        let first = &chunk[0];
                        let text_of = |get: &dyn Fn(&PassageVerse) -> Option<String>| -> String {
                            if chunk.len() == 1 {
                                get(&chunk[0]).unwrap_or_default()
                            } else {
                                chunk
                                    .iter()
                                    .filter_map(|v| get(v).map(|t| if v.verse > 0 { format!("{} {}", v.verse, t) } else { t }))
                                    .collect::<Vec<_>>()
                                    .join(" ")
                            }
                        };
                        let mut texts = vec![text_of(&|v: &PassageVerse| Some(v.text.clone()))];
                        texts.extend((0..p.extra_bibles.len()).map(|k| text_of(&|v: &PassageVerse| v.extra.get(k).cloned())));
                        Slide {
                            label: if first.verse > 0 { format!("{}{}{}", first.chapter, sep, first.verse) } else { String::new() },
                            tag: None,
                            number: first.verse,
                            scene: scene(texts, self.bible_footer(p, chunk), ColumnLayout::Stacked, &bible_style),
                        }
                    })
                    .collect()
            }
            ItemKind::Text { title, text } => {
                let part = SongPart { tag: "O".into(), number: 0, text: text.clone(), translations: Vec::new() };
                part.pages()
                    .into_iter()
                    .enumerate()
                    .map(|(i, page)| Slide {
                        label: (i + 1).to_string(),
                        tag: None,
                        number: i as u32 + 1,
                        scene: scene(vec![page], title.clone(), ColumnLayout::default(), &style),
                    })
                    .collect()
            }
            ItemKind::Design(d) => {
                let mut doc = d.doc.clone();
                if let Some(pic) = &item.background
                    && doc.background.0[3] == 0
                {
                    doc.objects.insert(0, pic.object());
                }
                vec![Slide { label: d.name.clone(), tag: None, number: 0, scene: Scene::Canvas(doc) }]
            }
            ItemKind::Video(v) => vec![Slide { label: v.title.clone(), tag: None, number: 0, scene: Scene::Video(VideoScene::new(v)) }],
        }
    }

    pub fn item(&self, r: ItemRef) -> Option<&ServiceItem> {
        match r {
            ItemRef::Service(i) => self.service.items.get(i),
            ItemRef::Quick => self.quick.as_ref(),
        }
    }

    pub fn slides_of(&self, r: ItemRef) -> Vec<Slide> {
        self.item(r).map(|i| self.item_slides(i)).unwrap_or_default()
    }

    /// What the output shows: the frozen picture, black, or the live slide.
    pub fn shown(&self) -> Scene {
        if let Some(f) = &self.frozen {
            f.clone()
        } else if self.blank {
            Scene::Blank
        } else {
            self.live.clone()
        }
    }

    // -----------------------------------------------------------------------
    // Presenting

    pub fn select(&mut self, r: ItemRef, slide: usize) {
        self.sel_item = Some(r);
        self.sel_slide = slide;
        if self.settings.live_follows_selection {
            self.go_live();
        }
    }

    pub fn go_live(&mut self) {
        let Some(r) = self.sel_item else { return };
        let slides = self.slides_of(r);
        if let Some(s) = slides.get(self.sel_slide) {
            self.live = s.scene.clone();
            self.live_pos = Some((r, self.sel_slide));
        }
    }

    pub fn step_slide(&mut self, forward: bool) {
        let Some(r) = self.sel_item else {
            if !self.service.items.is_empty() {
                self.select(ItemRef::Service(0), 0);
            }
            return;
        };
        let n = self.slides_of(r).len();
        if forward && self.sel_slide + 1 < n {
            self.select(r, self.sel_slide + 1);
        } else if !forward && self.sel_slide > 0 {
            self.select(r, self.sel_slide - 1);
        } else if let ItemRef::Service(i) = r {
            // on to the next / previous item
            if forward && i + 1 < self.service.items.len() {
                self.select(ItemRef::Service(i + 1), 0);
            } else if !forward && i > 0 {
                let last = self.slides_of(ItemRef::Service(i - 1)).len().saturating_sub(1);
                self.select(ItemRef::Service(i - 1), last);
            }
        }
    }

    pub fn step_item(&mut self, forward: bool) {
        let n = self.service.items.len();
        if n == 0 {
            return;
        }
        let next = match self.sel_item {
            Some(ItemRef::Service(i)) if forward => (i + 1).min(n - 1),
            Some(ItemRef::Service(i)) => i.saturating_sub(1),
            _ => 0,
        };
        self.select(ItemRef::Service(next), 0);
    }

    /// The next slide with this mark (V, C, …) in the selected item, wrapping around.
    pub fn jump_to_mark(&mut self, code: &str) {
        let Some(r) = self.sel_item else { return };
        let slides = self.slides_of(r);
        let n = slides.len();
        let found = (1..=n).map(|k| (self.sel_slide + k) % n).find(|&i| slides[i].tag.as_deref() == Some(code));
        if let Some(i) = found {
            self.select(r, i);
        }
    }

    /// Verse `n` of a song.
    pub fn jump_to_verse(&mut self, n: u32) {
        let Some(r) = self.sel_item else { return };
        if let Some(i) = self.slides_of(r).iter().position(|s| s.tag.as_deref() == Some("V") && s.number == n) {
            self.select(r, i);
        }
    }

    pub fn set_blank(&mut self, on: bool) {
        self.blank = on;
    }

    pub fn set_freeze(&mut self, on: bool) {
        self.frozen = if on { Some(self.shown()) } else { None };
    }

    pub fn start_alert(&mut self, text: &str) {
        let style = self.settings.alert_style.clone();
        let duration = style.timeout;
        self.alert = Some((Alert { text: text.to_string(), style, duration }, crate::js::now_ms()));
    }

    pub fn toggle_output(&mut self) {
        if crate::js::output_open() {
            crate::js::close_output();
        } else if !crate::js::open_output() {
            self.error(t("output.blocked"));
        }
        self.last_sent.clear();
    }

    // -----------------------------------------------------------------------
    // The service

    pub fn add_to_service(&mut self, kind: ItemKind) {
        let pos = match self.sel_item {
            Some(ItemRef::Service(i)) => (i + 1).min(self.service.items.len()),
            _ => self.service.items.len(),
        };
        let item = ServiceItem::new(kind);
        let title = item.title();
        self.service.items.insert(pos, item);
        if let Some((ItemRef::Service(i), s)) = self.live_pos
            && i >= pos
        {
            self.live_pos = Some((ItemRef::Service(i + 1), s));
        }
        self.notify(tf("service.added", &[("title", &title)]));
    }

    /// "Add to service" of the open tab (its button, key A, the `add-to-service` tool).
    pub fn add_selected_to_service(&mut self) -> Result<String, String> {
        let kind = match self.tab {
            Tab::Songs => {
                if self.song.title.trim().is_empty() && self.song.parts.is_empty() {
                    return Err(t("songs.nothing_to_add"));
                }
                ItemKind::Song(self.song.clone())
            }
            Tab::Bible => ItemKind::Bible(self.bible_selected_passage().ok_or_else(|| t("bible.nothing_to_add"))?),
            Tab::Service => {
                let q = self.quick.take().ok_or_else(|| t("service.no_quick"))?;
                if self.sel_item == Some(ItemRef::Quick) {
                    self.sel_item = None;
                }
                if let Some((ItemRef::Quick, _)) = self.live_pos {
                    self.live_pos = None;
                }
                q.kind
            }
            Tab::Settings => return Err(t("settings.nothing_to_add")),
        };
        let title = ServiceItem::new(kind.clone()).title();
        self.add_to_service(kind);
        Ok(title)
    }

    pub fn new_service(&mut self) {
        self.service = Service::default();
        self.service_name.clear();
        self.sel_item = None;
        self.live_pos = None;
    }

    pub fn move_item(&mut self, up: bool) {
        let Some(ItemRef::Service(i)) = self.sel_item else { return };
        let j = if up { i.checked_sub(1) } else { (i + 1 < self.service.items.len()).then_some(i + 1) };
        let Some(j) = j else { return };
        self.service.items.swap(i, j);
        self.sel_item = Some(ItemRef::Service(j));
        if let Some((ItemRef::Service(l), s)) = self.live_pos {
            let l = if l == i {
                j
            } else if l == j {
                i
            } else {
                l
            };
            self.live_pos = Some((ItemRef::Service(l), s));
        }
    }

    pub fn remove_item(&mut self) {
        let Some(ItemRef::Service(i)) = self.sel_item else { return };
        if i >= self.service.items.len() {
            return;
        }
        self.service.items.remove(i);
        self.live_pos = match self.live_pos {
            Some((ItemRef::Service(l), _)) if l == i => None,
            Some((ItemRef::Service(l), s)) if l > i => Some((ItemRef::Service(l - 1), s)),
            other => other,
        };
        self.sel_item = if self.service.items.is_empty() { None } else { Some(ItemRef::Service(i.min(self.service.items.len() - 1))) };
        self.sel_slide = 0;
    }

    pub fn rename_item(&mut self, name: &str) {
        if let Some(ItemRef::Service(i)) = self.sel_item
            && let Some(item) = self.service.items.get_mut(i)
        {
            item.name = if name.trim() == item.own_title() { String::new() } else { name.trim().to_string() };
        }
    }

    // -----------------------------------------------------------------------
    // Songs

    pub fn save_song(&mut self) {
        if self.song.title.trim().is_empty() {
            self.error(t("songs.needs_title"));
            return;
        }
        match self.songs.iter_mut().find(|s| s.id == self.song.id) {
            Some(s) => *s = self.song.clone(),
            None => self.songs.push(self.song.clone()),
        }
        self.songs.sort_by_key(|s| crate::storage::fold(&s.title));
        self.notify(tf("songs.saved", &[("title", &self.song.title)]));
    }

    pub fn song_in_library(&self) -> bool {
        self.songs.iter().any(|s| s.id == self.song.id)
    }

    pub fn song_changed(&self) -> bool {
        self.songs.iter().find(|s| s.id == self.song.id).map(|s| *s != self.song).unwrap_or(!self.song.parts.is_empty())
    }

    // -----------------------------------------------------------------------
    // Bible

    pub fn run_bible_search(&mut self) {
        let lang = crate::i18n::current_language();
        let query = self.bible.query.trim().to_string();
        self.bible.selected.clear();
        self.bible.results.clear();
        let Some(bible) = self.bible.current.as_ref() else {
            self.bible.info = t("bible.none");
            return;
        };
        if query.is_empty() {
            self.bible.info.clear();
            return;
        }
        // a reference needs a chapter number: "Jan" alone is a word to search for
        if query.chars().any(|c| c.is_ascii_digit())
            && let Some(ranges) = bible.parse_reference(&query, &lang)
        {
            let hits: Vec<Hit> = ranges.iter().flat_map(|r| bible.verses_in(r)).collect();
            if !hits.is_empty() {
                self.bible.info = tf("bible.found_ref", &[("ref", &bible.format_reference(&hits, &lang)), ("n", &hits.len().to_string())]);
                self.bible.selected = vec![true; hits.len()];
                self.bible.results = hits;
                return;
            }
        }
        if self.bible.index.is_none() {
            self.bible.index = Some(SearchIndex::build(bible));
        }
        let hits = self.bible.index.as_ref().map(|i| i.search_where(&query, 500, |_| true)).unwrap_or_default();
        self.bible.info =
            if hits.is_empty() { t("bible.nothing_found") } else { tf("bible.found_words", &[("n", &hits.len().to_string())]) };
        self.bible.selected = vec![false; hits.len()];
        self.bible.results = hits;
    }

    /// The selected verses (all found when none is selected) as a passage.
    pub fn bible_selected_passage(&self) -> Option<Passage> {
        let bible = self.bible.current.as_ref()?;
        let lang = crate::i18n::current_language();
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

    pub fn bible_show_live(&mut self) -> bool {
        let Some(p) = self.bible_selected_passage() else { return false };
        self.quick = Some(ServiceItem::new(ItemKind::Bible(p)));
        self.sel_item = Some(ItemRef::Quick);
        self.sel_slide = 0;
        self.go_live();
        true
    }

    fn find_bible(&self, q: &str) -> Option<String> {
        let f = crate::storage::fold(q);
        self.bibles
            .iter()
            .find(|m| m.id == q || crate::storage::fold(&m.abbrev) == f || crate::storage::fold(&m.name) == f)
            .map(|m| m.id.clone())
    }

    // -----------------------------------------------------------------------
    // Keys

    pub fn action_enabled(&self, a: Action) -> bool {
        match a {
            Action::RenameItem | Action::RemoveItem => self.tab == Tab::Service && matches!(self.sel_item, Some(ItemRef::Service(_))),
            Action::AddToService => match self.tab {
                Tab::Service => self.quick.is_some(),
                Tab::Settings => false,
                _ => true,
            },
            Action::OutputFullscreen => false,
            a if a.is_navigation() => matches!(self.tab, Tab::Service | Tab::Bible),
            _ => true,
        }
    }

    pub fn handle_actions(&mut self, actions: Vec<Action>) {
        for a in actions {
            match a {
                Action::NextSlide => self.step_slide(true),
                Action::PrevSlide => self.step_slide(false),
                Action::FirstSlide => {
                    if let Some(r) = self.sel_item {
                        self.select(r, 0)
                    }
                }
                Action::LastSlide => {
                    if let Some(r) = self.sel_item {
                        let n = self.slides_of(r).len();
                        self.select(r, n.saturating_sub(1))
                    }
                }
                Action::NextItem => self.step_item(true),
                Action::PrevItem => self.step_item(false),
                Action::GoLive => {
                    if self.tab == Tab::Bible {
                        self.bible_show_live();
                    } else {
                        self.go_live();
                    }
                }
                Action::ToggleLive => self.settings.live_follows_selection = !self.settings.live_follows_selection,
                Action::Blank => self.blank = !self.blank,
                Action::Freeze => self.set_freeze(self.frozen.is_none()),
                Action::ToggleOutput => self.toggle_output(),
                Action::OutputFullscreen => {}
                Action::Alert => self.dialog = Some(Dialog::Alert(self.alert.as_ref().map(|a| a.0.text.clone()).unwrap_or_default())),
                Action::Find => match self.tab {
                    Tab::Bible => self.bible.focus_search = true,
                    _ => {
                        self.tab = Tab::Songs;
                        self.focus_song_search = true;
                    }
                },
                Action::AddToService => {
                    if let Err(e) = self.add_selected_to_service() {
                        self.notify(e);
                    }
                }
                Action::RenameItem => {
                    if let Some(item) = self.sel_item.and_then(|r| self.item(r)) {
                        self.dialog = Some(Dialog::Rename(item.title()));
                    }
                }
                Action::RemoveItem => self.remove_item(),
                Action::TabService => self.tab = Tab::Service,
                Action::TabSongs => self.tab = Tab::Songs,
                Action::TabBible => self.tab = Tab::Bible,
                Action::TabSettings => self.tab = Tab::Settings,
            }
        }
    }

    pub fn handle_keys(&mut self, ctx: &egui::Context) {
        if self.dialog.is_some() || self.capture.is_some() || ctx.egui_wants_keyboard_input() {
            return;
        }
        let sc = self.settings.shortcuts.clone();
        let actions = sc.triggered(ctx, |a| self.action_enabled(a));
        self.handle_actions(actions);
        if self.tab != Tab::Service {
            return;
        }
        // the marks of song parts (V, C, …) and the verse numbers
        let pressed: Vec<egui::Key> = ctx.input(|i| {
            i.events
                .iter()
                .filter_map(|e| match e {
                    egui::Event::Key { key, pressed: true, modifiers, .. } if modifiers.is_none() => Some(*key),
                    _ => None,
                })
                .collect()
        });
        for key in pressed {
            if let Some((code, _)) = MARKS.iter().find(|(_, k)| *k == key) {
                self.jump_to_mark(code);
            } else if self.settings.number_keys_jump_to_verse
                && let Some(n) = crate::shortcuts::digit_of(key)
            {
                self.jump_to_verse(n);
            }
        }
    }

    // -----------------------------------------------------------------------
    // The output window and the tools

    pub fn output_message(&self) -> OutputMsg {
        let scene = self.shown();
        let media = scene.media();
        OutputMsg {
            scene,
            alert: self.alert.as_ref().map(|(a, since)| (a.clone(), ((crate::js::now_ms() - since) / 1000.0) as f32)),
            media,
            fullscreen_keys: self.settings.shortcuts.bindings(Action::OutputFullscreen),
            language: crate::i18n::current_language(),
        }
    }

    /// Sends the output window what it shows when that changed (or when it asks).
    pub fn sync_output(&mut self) {
        let mut hello = false;
        for text in crate::js::take_output_messages() {
            match serde_json::from_str::<FromOutput>(&text) {
                Ok(FromOutput::Hello) => hello = true,
                Ok(FromOutput::Action(a)) if a.is_navigation() => self.handle_actions(vec![a]),
                _ => {}
            }
        }
        let msg = self.output_message();
        // the alert's age changes every frame: compared without it
        let key = serde_json::to_string(&(
            &msg.scene,
            msg.alert.as_ref().map(|a| (&a.0.text, self.alert.as_ref().map(|x| x.1))),
            &msg.fullscreen_keys,
            &msg.language,
        ))
        .unwrap_or_default();
        if hello || key != self.last_sent {
            crate::js::output_post(&serde_json::to_string(&msg).unwrap_or_default());
            self.last_sent = key;
        }
    }

    pub fn run_tools(&mut self, ctx: &egui::Context) {
        for text in crate::js::take_commands() {
            let Ok(call) = serde_json::from_str::<Value>(&text) else { continue };
            let id = call["id"].as_f64().unwrap_or(0.0);
            let name = call["name"].as_str().unwrap_or_default().to_string();
            let args = call["args"].as_object().cloned().unwrap_or_default();
            let r = self.run_command(&name, &args, ctx);
            match r {
                Ok(text) => crate::js::answer_command(id, true, &text),
                Err(e) => crate::js::answer_command(id, false, &e),
            }
        }
    }

    pub fn status_text(&self) -> String {
        let live = match self.live_pos {
            Some((r, s)) => self.item(r).map(|i| format!("{} — {} {}", i.title(), t("status.slide"), s + 1)).unwrap_or_default(),
            None => "-".into(),
        };
        let selected = match self.sel_item {
            Some(r) => self.item(r).map(|i| format!("{} ({} {})", i.title(), t("status.slide"), self.sel_slide + 1)).unwrap_or_default(),
            None => "-".into(),
        };
        format!(
            "live: {live}\nselected: {selected}\nlive mode: {}\nblank: {}\nfrozen: {}\nalert: {}\noutput window: {}",
            on_off(self.settings.live_follows_selection),
            on_off(self.blank),
            on_off(self.frozen.is_some()),
            self.alert.as_ref().map(|a| a.0.text.as_str()).unwrap_or("-"),
            if crate::js::output_open() { "open" } else { "closed" },
        )
    }

    /// A tool call: what the button of the same name does.
    pub fn run_command(&mut self, name: &str, args: &Map<String, Value>, ctx: &egui::Context) -> Result<String, String> {
        crate::commands::check(name, args)?;
        let text = |k: &str| args.get(k).and_then(Value::as_str).unwrap_or_default().to_string();
        let state = || args.get("state").and_then(Value::as_str).map(|s| s == "on");
        let need_item = |app: &Self| -> Result<usize, String> {
            match app.sel_item {
                Some(ItemRef::Service(i)) if i < app.service.items.len() => Ok(i),
                _ => Err("no service item is selected (select-item first)".into()),
            }
        };
        match name {
            "new-service" => {
                self.new_service();
                Ok("new service".into())
            }
            "add-song" => {
                let q = crate::storage::fold(&text("title"));
                let song = self
                    .songs
                    .iter()
                    .find(|s| crate::storage::fold(&s.title) == q)
                    .or_else(|| self.songs.iter().find(|s| crate::storage::fold(&s.title).starts_with(&q)))
                    .cloned()
                    .ok_or_else(|| format!("no song {:?} in the library (list-songs)", text("title")))?;
                let title = song.title.clone();
                self.add_to_service(ItemKind::Song(song));
                Ok(format!("added song {title}"))
            }
            "add-text" => {
                self.add_to_service(ItemKind::Text { title: text("title"), text: text("text").replace("\\n", "\n") });
                Ok("added text".into())
            }
            "add-to-service" => {
                if args.contains_key("tab") {
                    self.tab = tab_named(&text("tab"));
                }
                self.add_selected_to_service().map(|title| format!("added {title}"))
            }
            "list-items" => {
                let mut out = String::new();
                for (i, it) in self.service.items.iter().enumerate() {
                    let sel = if self.sel_item == Some(ItemRef::Service(i)) { "*" } else { " " };
                    let live = if matches!(self.live_pos, Some((ItemRef::Service(l), _)) if l == i) { " LIVE" } else { "" };
                    out.push_str(&format!("{sel}{}. {}{live}\n", i + 1, it.title()));
                }
                Ok(if out.is_empty() { "the service is empty".into() } else { out.trim_end().into() })
            }
            "select-item" => {
                let q = text("item");
                let i = match q.trim().parse::<usize>() {
                    Ok(n) if n >= 1 && n <= self.service.items.len() => n - 1,
                    _ => {
                        let f = crate::storage::fold(&q);
                        self.service
                            .items
                            .iter()
                            .position(|it| crate::storage::fold(&it.title()) == f)
                            .or_else(|| self.service.items.iter().position(|it| crate::storage::fold(&it.title()).starts_with(&f)))
                            .ok_or_else(|| format!("no item {q:?} (list-items)"))?
                    }
                };
                self.select(ItemRef::Service(i), 0);
                Ok(format!("selected {}", self.service.items[i].title()))
            }
            "rename-item" => {
                need_item(self)?;
                self.rename_item(&text("name"));
                Ok("renamed".into())
            }
            "move-up" | "move-down" => {
                need_item(self)?;
                self.move_item(name == "move-up");
                Ok("moved".into())
            }
            "remove-item" => {
                need_item(self)?;
                self.remove_item();
                Ok("removed".into())
            }
            "list-slides" => {
                let r = self.sel_item.ok_or("no item is selected")?;
                let out: Vec<String> = self
                    .slides_of(r)
                    .iter()
                    .enumerate()
                    .map(|(i, s)| {
                        let first = match &s.scene {
                            Scene::Text(t) => t.text.lines().next().unwrap_or("").to_string(),
                            _ => String::new(),
                        };
                        let sel = if i == self.sel_slide { "*" } else { " " };
                        format!("{sel}{}. {}  {first}", i + 1, s.label)
                    })
                    .collect();
                Ok(out.join("\n"))
            }
            "select-slide" => {
                let r = self.sel_item.ok_or("no item is selected")?;
                let n = args.get("slide").and_then(Value::as_u64).unwrap_or(0) as usize;
                let count = self.slides_of(r).len();
                if n < 1 || n > count {
                    return Err(format!("the item has {count} slides"));
                }
                self.select(r, n - 1);
                Ok(format!("slide {n}"))
            }
            "next-slide" | "prev-slide" | "first-slide" | "last-slide" | "next-item" | "prev-item" | "go-live" => {
                let a = match name {
                    "next-slide" => Action::NextSlide,
                    "prev-slide" => Action::PrevSlide,
                    "first-slide" => Action::FirstSlide,
                    "last-slide" => Action::LastSlide,
                    "next-item" => Action::NextItem,
                    "prev-item" => Action::PrevItem,
                    _ => Action::GoLive,
                };
                if a == Action::GoLive {
                    self.go_live();
                } else {
                    self.handle_actions(vec![a]);
                }
                Ok(self.status_text())
            }
            "live-mode" => {
                self.settings.live_follows_selection = state().unwrap_or(true);
                Ok(format!("live mode {}", on_off(self.settings.live_follows_selection)))
            }
            "blank" => {
                let on = state().unwrap_or(!self.blank);
                self.set_blank(on);
                Ok(format!("blank {}", on_off(on)))
            }
            "freeze" => {
                let on = state().unwrap_or(self.frozen.is_none());
                if on != self.frozen.is_some() {
                    self.set_freeze(on);
                }
                Ok(format!("freeze {}", on_off(on)))
            }
            "output" => {
                if text("state") == "open" {
                    if !crate::js::output_open() && !crate::js::open_output() {
                        return Err(t("output.blocked"));
                    }
                } else {
                    crate::js::close_output();
                }
                self.last_sent.clear();
                Ok(format!("output window {}", if crate::js::output_open() { "open" } else { "closed" }))
            }
            "alert" => {
                self.start_alert(&text("text"));
                Ok("alert running".into())
            }
            "stop-alert" => {
                self.alert = None;
                Ok("alert stopped".into())
            }
            "show-tab" => {
                self.tab = tab_named(&text("tab"));
                Ok(format!("{:?}", self.tab))
            }
            "list-songs" => {
                let q = crate::storage::fold(&text("search"));
                let list: Vec<&str> =
                    self.songs.iter().filter(|s| crate::storage::fold(&s.title).contains(&q)).map(|s| s.title.as_str()).collect();
                Ok(if list.is_empty() { "no songs".into() } else { list.join("\n") })
            }
            "list-bibles" => {
                let cur = self.settings.bible.clone().unwrap_or_default();
                let list: Vec<String> = self
                    .bibles
                    .iter()
                    .map(|m| format!("{}{}  {} ({})", if m.id == cur { "*" } else { " " }, m.id, m.name, m.abbrev))
                    .collect();
                Ok(if list.is_empty() { "no bibles (import one in the Bible tab)".into() } else { list.join("\n") })
            }
            "bible" => {
                if args.contains_key("bible") {
                    let id = self.find_bible(&text("bible")).ok_or_else(|| format!("no bible {:?} (list-bibles)", text("bible")))?;
                    if self.bible.current.as_ref().map(|b| b.meta.id.as_str()) != Some(id.as_str()) {
                        self.load_bible(&id);
                        return Err("the bible is being loaded: call again in a moment".into());
                    }
                }
                if self.bible.current.is_none() {
                    return Err(if self.bible.loading.is_some() { "the bible is being loaded".into() } else { t("bible.none") });
                }
                if args.contains_key("query") {
                    self.bible.query = text("query");
                    self.run_bible_search();
                }
                match text("action").as_str() {
                    "search" => {
                        let bible = self.bible.current.as_ref().unwrap();
                        let lang = crate::i18n::current_language();
                        let mut out: Vec<String> = self
                            .bible
                            .results
                            .iter()
                            .take(50)
                            .filter_map(|h| {
                                let (_, c, v) = bible.verse(h)?;
                                Some(format!("{} {}:{}  {}", bible.book_name(h.book, &lang), c.number, v.number, v.text))
                            })
                            .collect();
                        if out.is_empty() {
                            out.push(self.bible.info.clone());
                        } else if self.bible.results.len() > 50 {
                            out.push(format!("… {} verses found, 50 listed", self.bible.results.len()));
                        }
                        Ok(out.join("\n"))
                    }
                    "show" => {
                        if !self.bible_show_live() {
                            return Err("nothing found to show (bible search first)".into());
                        }
                        Ok(self.status_text())
                    }
                    _ => {
                        let p = self.bible_selected_passage().ok_or("nothing found to add (bible search first)")?;
                        let r = p.reference.clone();
                        self.add_to_service(ItemKind::Bible(p));
                        Ok(format!("added {r}"))
                    }
                }
            }
            "status" => Ok(self.status_text()),
            "get-settings" => serde_json::to_string_pretty(&self.settings).map_err(|e| e.to_string()),
            "set-setting" => {
                let key = text("key");
                let value: Value =
                    serde_json::from_str(&text("value")).map_err(|e| format!("value is not JSON ({e}); a text needs quotes: \"cs\""))?;
                let mut all = serde_json::to_value(&self.settings).map_err(|e| e.to_string())?;
                let mut at = &mut all;
                for part in key.split('.') {
                    at = at.get_mut(part).ok_or_else(|| format!("no setting {key:?} (get-settings lists them)"))?;
                }
                *at = value;
                let new: Settings = serde_json::from_value(all).map_err(|e| format!("{key}: {e}"))?;
                self.apply_settings(new, ctx);
                Ok(format!("{key} set"))
            }
            _ => Err(format!("unknown command {name:?}")),
        }
    }

    pub fn apply_settings(&mut self, new: Settings, ctx: &egui::Context) {
        if new.language != self.settings.language {
            let lang = if new.language.is_empty() { crate::i18n::browser_language() } else { new.language.clone() };
            crate::i18n::set_language(&lang);
        }
        if new.dark_mode != self.settings.dark_mode {
            set_theme(ctx, new.dark_mode);
        }
        self.settings = new;
    }
}

/// Dark or light (the browser's own theme would win otherwise).
pub fn set_theme(ctx: &egui::Context, dark: bool) {
    ctx.set_theme(if dark { egui::ThemePreference::Dark } else { egui::ThemePreference::Light });
}

fn on_off(b: bool) -> &'static str {
    if b { "on" } else { "off" }
}

pub fn tab_named(name: &str) -> Tab {
    match name {
        "songs" => Tab::Songs,
        "bible" => Tab::Bible,
        "settings" => Tab::Settings,
        _ => Tab::Service,
    }
}

impl eframe::App for WebApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.frame(ui);
    }

    fn raw_input_hook(&mut self, _ctx: &egui::Context, raw: &mut egui::RawInput) {
        crate::shortcuts::digits_by_position(&mut raw.events);
    }
}
