//! The presenter application (control window + output window).

mod alerts;
mod bible_index;
mod bible_tab;
mod designer_tab;
pub(crate) mod files;
mod keynav;
mod legacy;
mod service_tab;
mod settings_tab;
mod songs_tab;
mod video_ctl;

use crate::bible::{Bible, BibleMeta, Bookmarks, Hit};
use crate::formats;
use crate::i18n::{t, tf};
use crate::model::*;
use crate::net::Server;
use crate::render::{self, TextureCache};
use crate::settings::Settings;
use crate::shortcuts::{self, Action, KeyBinding};
use crate::storage::{self, Library};
use eframe::egui::{self, Key, ViewportCommand, ViewportId};
use std::path::PathBuf;
use std::time::{Instant, SystemTime};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tab {
    Service,
    Songs,
    Designer,
    Bible,
    Settings,
}

/// Which item the presenter cursor is on: a service item, or the quick
/// (not yet added) bible passage.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ItemRef {
    Service(usize),
    Quick,
}

/// What a file write is for — used by the "file exists" dialog to finish the job.
#[derive(Clone, Debug)]
pub enum WriteKind {
    Service,
    ServiceOsz,
    SongOpenLyrics(Song),
    AllSongsOpenLyrics,
    Design(Design),
    Backup,
    LangTemplate,
}

/// What to do after the user resolved an "unsaved changes" question.
#[derive(Clone, Debug)]
pub enum Then {
    NewService,
    OpenService(Option<PathBuf>),
    Quit,
    NewSong,
    EditSong(Song),
    NewDesign,
    EditDesign(Design),
}

#[derive(Clone, Debug)]
pub enum Dialog {
    Message(String),
    /// Target file exists already.
    FileExists {
        path: PathBuf,
        kind: WriteKind,
    },
    /// The service file was changed on disk after we opened it.
    ChangedOnDisk {
        path: PathBuf,
    },
    UnsavedService {
        then: Then,
    },
    UnsavedSong {
        then: Then,
    },
    UnsavedDesign {
        then: Then,
    },
    UnsavedOnQuit,
    /// Quit with nothing unsaved: still asks, so a stray Ctrl+Q cannot end a service.
    ConfirmQuit,
    SongTitleClash {
        as_new: bool,
    },
    DesignNameClash {
        as_new: bool,
    },
    DeleteSong(String),
    DeleteDesign(String),
    DeleteBible(String),
    /// Remove a song text column that still has text.
    RemoveSongColumn(usize),
    /// First run: old oh-presenter files were found.
    LegacyImport,
    /// The alert window (F7): type a message and send it to the outputs.
    Alert,
    /// First word search in a bible: for which language should the index be made?
    /// (the current bible's language is suggested)
    BibleIndexLanguage(String),
}

/// Color of the "add" (➕) icons.
pub const ADD_COLOR: egui::Color32 = egui::Color32::from_rgb(66, 150, 255);

/// The blue ➕ icon, to put in front of a button label: `ui.button((plus_icon(), label))`.
pub fn plus_icon() -> egui::RichText {
    egui::RichText::new("➕").color(ADD_COLOR)
}

/// What a dialog button does once the dialog is closed.
type DialogAction = Box<dyn FnOnce(&mut App)>;

pub struct BibleUi {
    pub bibles: Vec<BibleMeta>,
    pub current: Option<Bible>,
    pub query: String,
    pub results: Vec<Hit>,
    pub selected: Vec<bool>,
    pub last_clicked: Option<usize>,
    pub info: String,
    pub bookmarks: Bookmarks,
    pub book: usize,
    pub chapter: usize,
    /// Word search index of the current bible (bible id, index).
    pub index: Option<(String, crate::bible::SearchIndex)>,
    /// Indexes being made in the background.
    pub indexing: Option<bible_index::Indexing>,
    /// Check for missing indexes again when the running job ends.
    pub index_again: bool,
    /// Language picked in the "which language" dialog.
    pub index_choice: Option<String>,
    /// Word search is off for this language: offer to turn it on.
    pub offer_index: Option<String>,
    /// Missing languages of older bibles were guessed already.
    pub languages_filled: bool,
    /// The word query is also a book name: offer to open that book.
    pub book_hint: Option<usize>,
}

pub struct DesignerState {
    pub design: Design,
    pub in_library: bool,
    pub dirty: bool,
    pub selected: Vec<usize>,
    pub drag: Option<designer_tab::Drag>,
    pub undo: Vec<CanvasDoc>,
    pub redo: Vec<CanvasDoc>,
    pub committed: CanvasDoc,
    pub last_push: Instant,
    pub search: String,
}

pub struct SongEditor {
    pub song: Song,
    pub in_library: bool,
    pub dirty: bool,
    pub search: String,
    /// Name typed for a new text column.
    pub new_column: String,
}

/// State shared with the output window (a deferred viewport, see [`App::show_output`]).
pub struct OutputShared {
    scene: Scene,
    alert: Option<(Alert, Instant)>,
    /// The output window's own texture cache (created on its first frame).
    textures: Option<TextureCache>,
    keys_enabled: bool,
    shortcuts: shortcuts::Shortcuts,
    mark_keys: MarkKeys,
    // filled by the output window, handled by the main window
    actions: Vec<Action>,
    jump: Option<(String, Option<u32>)>,
    fullscreen: Option<bool>,
    close: bool,
}

pub struct App {
    pub settings: Settings,
    /// The alert running on the outputs and when it started.
    pub alert: Option<(Alert, Instant)>,
    /// Text in the alert window.
    pub alert_text: String,
    /// Focus the alert window's text field (once, when it opens).
    alert_focus: bool,
    /// Alt + letters chooses a menu or tab (see [`keynav`]).
    pub alt_nav: keynav::AltNav,
    /// Arrow / page keys scroll the current tab (see [`keynav`]).
    key_scroll: Option<keynav::KeyScroll>,
    /// Scroll position and largest position of the tabs' main scroll areas (last frame).
    scroll_pos: std::collections::HashMap<&'static str, (f32, f32)>,
    last_frame: Option<Instant>,
    pub output: std::sync::Arc<std::sync::Mutex<OutputShared>>,
    /// Search for / offer to import files of the old oh-presenter.
    pub legacy: Option<legacy::LegacyState>,
    pub lib: Library,
    pub tab: Tab,

    pub service: Service,
    pub service_path: Option<PathBuf>,
    pub service_mtime: Option<SystemTime>,
    pub service_dirty: bool,
    pub quick: Option<ServiceItem>,
    pub sel_item: Option<ItemRef>,
    pub sel_slide: usize,
    pub live: Scene,
    pub live_pos: Option<(ItemRef, usize)>,
    pub blank: bool,
    pub add_search: String,

    pub songs: SongEditor,
    pub designer: DesignerState,
    pub bible: BibleUi,

    pub output_open: bool,
    pub output_fullscreen: bool,
    pub server: Option<Server>,
    pub textures: TextureCache,

    pub dialog: Option<Dialog>,
    pub status: Option<(String, Instant)>,
    pub capture: Option<settings_tab::Capture>,
    pub focus_search: bool,
    pub scroll_pending: bool,
    pub quit_confirmed: bool,
    pub new_mark: (String, String),
    /// A system file dialog that is open right now (runs on its own thread).
    pub file_dialog: Option<files::Pending>,
    /// When the live video's state (`VideoScene::position`) was set.
    pub video_since: Instant,
    /// The live video's sound (ffplay) and the playback token it belongs to.
    video_audio: Option<(u64, crate::video::Audio)>,
    /// Videos being copied into the library and probed in the background.
    video_import: Option<std::sync::mpsc::Receiver<Result<ItemKind, String>>>,
}

impl App {
    pub fn new(cc: &eframe::CreationContext) -> Self {
        let settings = Settings::load();
        crate::i18n::load_dir(&storage::paths().lang());
        crate::i18n::set_language(&settings.language);
        let mut app = Self::with_settings(&cc.egui_ctx, settings);
        if !app.settings.legacy_import_answered {
            // first run (until answered): look for files of the old oh-presenter
            app.start_legacy_scan(false);
        }
        app
    }

    pub fn with_settings(ctx: &egui::Context, settings: Settings) -> Self {
        render::install_fonts(ctx, None);
        // Ctrl+Q goes through our own (customisable) Quit action, which asks first
        ctx.options_mut(|o| o.quit_shortcuts.clear());
        ctx.set_visuals(if settings.dark_mode { egui::Visuals::dark() } else { egui::Visuals::light() });
        let server = settings.network_enabled.then(|| Server::start(settings.network_port));
        let mut bible = BibleUi {
            bibles: crate::bible::list_bibles(),
            current: None,
            query: String::new(),
            results: vec![],
            selected: vec![],
            last_clicked: None,
            info: String::new(),
            bookmarks: Bookmarks::load(),
            book: 0,
            chapter: 0,
            index: None,
            indexing: None,
            index_again: false,
            index_choice: None,
            offer_index: None,
            languages_filled: false,
            book_hint: None,
        };
        let bible_id = settings.bible.clone().or_else(|| bible.bibles.first().map(|b| b.id.clone()));
        bible.current = bible_id.and_then(|id| Bible::load(&id));
        let design = Design { id: make_id(), name: t("designer.untitled"), doc: CanvasDoc::default() };
        let mut app = Self {
            lib: Library::load(),
            tab: Tab::Service,
            service: Service::default(),
            service_path: None,
            service_mtime: None,
            service_dirty: false,
            quick: None,
            sel_item: None,
            sel_slide: 0,
            live: Scene::Blank,
            live_pos: None,
            blank: false,
            add_search: String::new(),
            songs: SongEditor {
                song: Self::blank_song(),
                in_library: false,
                dirty: false,
                search: String::new(),
                new_column: String::new(),
            },
            designer: DesignerState {
                committed: design.doc.clone(),
                design,
                in_library: false,
                dirty: false,
                selected: vec![],
                drag: None,
                undo: vec![],
                redo: vec![],
                last_push: Instant::now(),
                search: String::new(),
            },
            bible,
            output_open: false,
            output_fullscreen: false,
            server,
            textures: TextureCache::new(storage::paths().media()),
            dialog: None,
            status: None,
            capture: None,
            focus_search: false,
            scroll_pending: false,
            quit_confirmed: false,
            new_mark: (String::new(), String::new()),
            file_dialog: None,
            video_since: Instant::now(),
            video_audio: None,
            video_import: None,
            legacy: None,
            alert: None,
            alert_text: String::new(),
            alert_focus: false,
            alt_nav: Default::default(),
            key_scroll: None,
            scroll_pos: Default::default(),
            last_frame: None,
            output: std::sync::Arc::new(std::sync::Mutex::new(OutputShared {
                scene: Scene::Blank,
                alert: None,
                textures: None,
                keys_enabled: true,
                shortcuts: settings.shortcuts.clone(),
                mark_keys: MarkKeys::default(),
                actions: Vec::new(),
                jump: None,
                fullscreen: None,
                close: false,
            })),
            settings,
        };
        if let Some(err) = app.server.as_ref().and_then(|s| s.error.clone()) {
            app.notify(tf("net.error", &[("err", &err)]));
        }
        app
    }

    /// Presenting runs without vsync (see `main.rs`), so a continuous repaint (an animation)
    /// would run as fast as the GPU allows: cap it at ~120 frames per second.
    fn limit_frame_rate(&mut self) {
        const MIN_FRAME: std::time::Duration = std::time::Duration::from_micros(8_333);
        let now = Instant::now();
        if let Some(last) = self.last_frame {
            let since = now.duration_since(last);
            if since < MIN_FRAME {
                std::thread::sleep(MIN_FRAME - since);
            }
        }
        self.last_frame = Some(Instant::now());
    }

    pub fn blank_song() -> Song {
        let mut s = Song::new();
        s.parts.push(SongPart { tag: "V".into(), number: 1, text: String::new(), translations: Vec::new() });
        s
    }

    pub fn notify(&mut self, msg: impl Into<String>) {
        self.status = Some((msg.into(), Instant::now()));
    }

    pub fn error(&mut self, msg: impl Into<String>) {
        self.dialog = Some(Dialog::Message(msg.into()));
    }

    pub fn save_settings(&mut self) {
        if let Err(e) = self.settings.save() {
            self.notify(tf("error.save_settings", &[("err", &e.to_string())]));
        }
    }

    // -----------------------------------------------------------------------
    // Slides and output

    pub fn item(&self, r: ItemRef) -> Option<&ServiceItem> {
        match r {
            ItemRef::Service(i) => self.service.items.get(i),
            ItemRef::Quick => self.quick.as_ref(),
        }
    }

    fn text_background(&self) -> Option<CanvasDoc> {
        self.settings.text_background_design.as_ref().and_then(|id| self.lib.design(id)).map(|d| d.doc.clone())
    }

    pub fn item_slides(&self, item: &ServiceItem) -> Vec<Slide> {
        let style = self.settings.text_style.clone();
        let background = self.text_background();
        let columns_scene = |mut texts: Vec<String>, footer: String, layout: ColumnLayout| {
            let text = if texts.is_empty() { String::new() } else { texts.remove(0) };
            Scene::Text(TextScene { text, extra: texts, layout, footer, style: style.clone(), background: background.clone() })
        };
        let text_scene = |text: String, footer: String| columns_scene(vec![text], footer, ColumnLayout::default());
        match &item.kind {
            ItemKind::Song(song) => {
                let mut out = Vec::new();
                let shown = song.shown_columns();
                for part in song.ordered_parts() {
                    let pages = part.column_pages(&shown);
                    let n = pages.len();
                    for (i, page) in pages.into_iter().enumerate() {
                        let label = if n > 1 { format!("{} ({}/{})", part.code(), i + 1, n) } else { part.code() };
                        out.push(Slide {
                            label,
                            tag: Some(part.tag.clone()),
                            number: part.number,
                            scene: columns_scene(page, song.footer(), song.column_layout),
                        });
                    }
                }
                out
            }
            ItemKind::Design(d) => vec![Slide { label: d.name.clone(), tag: None, number: 0, scene: Scene::Canvas(d.doc.clone()) }],
            ItemKind::Bible(p) => {
                let per = self.settings.verses_per_slide.max(1);
                let lang = crate::i18n::current_language();
                let sep = if lang == "cs" { "," } else { ":" };
                p.verses
                    .chunks(per)
                    .map(|chunk| {
                        let first = &chunk[0];
                        let last = chunk.last().unwrap();
                        let text = if chunk.len() == 1 {
                            first.text.clone()
                        } else {
                            chunk
                                .iter()
                                .map(|v| if v.verse > 0 { format!("{} {}", v.verse, v.text) } else { v.text.clone() })
                                .collect::<Vec<_>>()
                                .join(" ")
                        };
                        let reference = if first.verse == 0 {
                            p.reference.clone()
                        } else if chunk.len() == 1 || first.chapter != last.chapter {
                            if chunk.len() == 1 {
                                format!("{} {}{}{}", first.book, first.chapter, sep, first.verse)
                            } else {
                                format!("{} {}{}{}-{}{}{}", first.book, first.chapter, sep, first.verse, last.chapter, sep, last.verse)
                            }
                        } else {
                            format!("{} {}{}{}-{}", first.book, first.chapter, sep, first.verse, last.verse)
                        };
                        let footer = if p.version.is_empty() { reference.clone() } else { format!("{reference} ({})", p.version) };
                        Slide {
                            label: if first.verse > 0 { format!("{}{}{}", first.chapter, sep, first.verse) } else { String::new() },
                            tag: None,
                            number: first.verse,
                            scene: text_scene(text, footer),
                        }
                    })
                    .collect()
            }
            ItemKind::Video(v) => {
                vec![Slide { label: v.title.clone(), tag: None, number: 0, scene: Scene::Video(VideoScene::new(v)) }]
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
                        scene: text_scene(page, title.clone()),
                    })
                    .collect()
            }
        }
    }

    pub fn slides_of(&self, r: ItemRef) -> Vec<Slide> {
        self.item(r).map(|i| self.item_slides(i)).unwrap_or_default()
    }

    pub fn set_live(&mut self, mut scene: Scene, pos: Option<(ItemRef, usize)>) {
        if let Scene::Video(v) = &mut scene {
            // a new playback from the start
            v.token = crate::video::next_token();
            self.video_since = Instant::now();
        }
        self.live = scene;
        self.live_pos = pos;
        self.blank = false;
        self.broadcast();
    }

    pub fn broadcast(&self) {
        if let Some(server) = &self.server {
            server.broadcast(if self.blank { &Scene::Blank } else { &self.live });
        }
    }

    /// Selects a slide; with live mode on it also goes live.
    pub fn select(&mut self, item: ItemRef, slide: usize) {
        self.sel_item = Some(item);
        self.sel_slide = slide;
        self.scroll_pending = true;
        if self.settings.live_follows_selection {
            self.go_live();
        }
    }

    pub fn go_live(&mut self) {
        let Some(item) = self.sel_item else { return };
        let slides = self.slides_of(item);
        if let Some(s) = slides.get(self.sel_slide) {
            self.set_live(s.scene.clone(), Some((item, self.sel_slide)));
        }
    }

    pub fn toggle_blank(&mut self) {
        if !self.blank {
            // a blanked video pauses (and its sound stops); it goes on with ▶
            self.video_update(|v| v.paused = true);
        }
        self.blank = !self.blank;
        self.broadcast();
    }

    fn service_len(&self) -> usize {
        self.service.items.len()
    }

    pub fn step_slide(&mut self, forward: bool) {
        let Some(item) = self.sel_item.or_else(|| (self.service_len() > 0).then_some(ItemRef::Service(0))) else { return };
        let n = self.slides_of(item).len();
        if self.sel_item.is_none() {
            return self.select(item, 0);
        }
        if forward {
            if self.sel_slide + 1 < n {
                return self.select(item, self.sel_slide + 1);
            }
            if let ItemRef::Service(i) = item
                && i + 1 < self.service_len()
            {
                self.select(ItemRef::Service(i + 1), 0)
            }
        } else {
            if self.sel_slide > 0 {
                return self.select(item, self.sel_slide - 1);
            }
            if let ItemRef::Service(i) = item
                && i > 0
            {
                let last = self.slides_of(ItemRef::Service(i - 1)).len().saturating_sub(1);
                self.select(ItemRef::Service(i - 1), last)
            }
        }
    }

    pub fn step_item(&mut self, forward: bool) {
        let next = match self.sel_item {
            None if self.service_len() > 0 => Some(0),
            Some(ItemRef::Service(i)) if forward && i + 1 < self.service_len() => Some(i + 1),
            Some(ItemRef::Service(i)) if !forward && i > 0 => Some(i - 1),
            Some(ItemRef::Quick) if self.service_len() > 0 => Some(0),
            _ => None,
        };
        if let Some(i) = next {
            self.select(ItemRef::Service(i), 0);
        }
    }

    /// Jumps to the next slide marked `tag` (and `number`, if given) in the current item.
    pub fn jump_to_mark(&mut self, tag: &str, number: Option<u32>) {
        let Some(item) = self.sel_item else { return };
        let slides = self.slides_of(item);
        let matches = |s: &Slide| s.tag.as_deref() == Some(tag) && number.map(|n| s.number.max(1) == n).unwrap_or(true);
        let start = self.sel_slide + 1;
        let found = (start..slides.len()).chain(0..start.min(slides.len())).find(|i| matches(&slides[*i]));
        match found {
            Some(i) => self.select(item, i),
            None => self.notify(tf("service.no_mark", &[("mark", &self.settings.mark_name(tag))])),
        }
    }

    // -----------------------------------------------------------------------
    // Service file handling

    pub fn new_service(&mut self) {
        self.service = Service::default();
        self.service_path = None;
        self.service_mtime = None;
        self.service_dirty = false;
        self.sel_item = None;
        self.sel_slide = 0;
    }

    pub fn request(&mut self, then: Then) {
        let needs = match &then {
            Then::NewService | Then::OpenService(_) => self.service_dirty.then(|| Dialog::UnsavedService { then: then.clone() }),
            Then::NewSong | Then::EditSong(_) => self.songs.dirty.then(|| Dialog::UnsavedSong { then: then.clone() }),
            Then::NewDesign | Then::EditDesign(_) => self.designer.dirty.then(|| Dialog::UnsavedDesign { then: then.clone() }),
            Then::Quit if self.quit_confirmed => None,
            Then::Quit if self.has_unsaved() => Some(Dialog::UnsavedOnQuit),
            Then::Quit => Some(Dialog::ConfirmQuit),
        };
        match needs {
            Some(d) => self.dialog = Some(d),
            None => self.run(then),
        }
    }

    pub fn has_unsaved(&self) -> bool {
        self.service_dirty || self.songs.dirty || self.designer.dirty
    }

    pub fn run(&mut self, then: Then) {
        match then {
            Then::NewService => self.new_service(),
            Then::OpenService(Some(p)) => self.open_service(p),
            Then::OpenService(None) => self.open_file_dialog(
                files::Spec::new(files::Mode::PickFile, t("service.open"))
                    .filter(t("file.service_filter"), &[formats::SERVICE_EXT, "osz", "oszl", "ohpres"]),
                files::Purpose::OpenService,
            ),
            Then::Quit => {
                self.quit_confirmed = true;
            }
            Then::NewSong => {
                self.songs.song = Self::blank_song();
                self.songs.in_library = false;
                self.songs.dirty = false;
            }
            Then::EditSong(song) => {
                self.songs.in_library = self.lib.song(&song.id).is_some();
                self.songs.song = song;
                self.songs.dirty = false;
                self.tab = Tab::Songs;
            }
            Then::NewDesign => {
                let d = Design { id: make_id(), name: t("designer.untitled"), doc: CanvasDoc::default() };
                self.designer_load(d, false);
            }
            Then::EditDesign(d) => {
                let in_lib = self.lib.design(&d.id).is_some();
                self.designer_load(d, in_lib);
                self.tab = Tab::Designer;
            }
        }
    }

    fn designer_load(&mut self, d: Design, in_library: bool) {
        let ds = &mut self.designer;
        ds.committed = d.doc.clone();
        ds.design = d;
        ds.in_library = in_library;
        ds.dirty = false;
        ds.selected.clear();
        ds.undo.clear();
        ds.redo.clear();
    }

    pub fn open_service(&mut self, path: PathBuf) {
        match formats::load_service(&path) {
            Ok(service) => {
                let ext = path.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
                self.new_service();
                self.service = service;
                if ext == formats::SERVICE_EXT {
                    self.service_path = Some(path.clone());
                    self.service_mtime = storage::mtime(&path);
                    self.settings.add_recent(&path.to_string_lossy());
                    self.save_settings();
                } else {
                    // imported from another format: saving must go to a new .ohsvc file,
                    // never back over the original
                    self.service_dirty = true;
                }
                self.notify(tf("service.opened", &[("file", &path.to_string_lossy()), ("n", &self.service.items.len().to_string())]));
                if !self.service.items.is_empty() {
                    self.sel_item = Some(ItemRef::Service(0));
                    self.sel_slide = 0;
                }
            }
            Err(e) => self.error(tf("error.open", &[("file", &path.to_string_lossy()), ("err", &e)])),
        }
    }

    /// "Save service": writes back to the service's own file, unless that file was
    /// changed by someone else in the meantime, then asks.
    pub fn save_service(&mut self) {
        self.save_service_then(None);
    }

    /// Saves the service, then (once it really is saved) does `then`. A new
    /// service first asks for a file name, so `then` may run a few frames later.
    pub fn save_service_then(&mut self, then: Option<Then>) {
        match self.service_path.clone() {
            None => self.save_service_as_then(then),
            Some(path) => {
                if path.exists() && storage::mtime(&path) != self.service_mtime {
                    self.dialog = Some(Dialog::ChangedOnDisk { path });
                } else {
                    self.write(WriteKind::Service, path);
                    self.after_service_saved(then);
                }
            }
        }
    }

    fn after_service_saved(&mut self, then: Option<Then>) {
        if let Some(th) = then
            && !self.service_dirty
            && self.dialog.is_none()
        {
            self.run(th);
        }
    }

    pub fn save_service_as(&mut self) {
        self.save_service_as_then(None);
    }

    fn save_service_as_then(&mut self, then: Option<Then>) {
        let name = self
            .service_path
            .as_ref()
            .and_then(|p| p.file_name())
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| format!("{}.{}", t("service.default_name"), formats::SERVICE_EXT));
        self.open_file_dialog(
            files::Spec::new(files::Mode::SaveFile, t("service.save_as"))
                .filter(t("file.service_filter"), &[formats::SERVICE_EXT])
                .file_name(name),
            files::Purpose::SaveServiceAs { then },
        );
    }

    /// Opens a system file dialog without blocking the UI; the answer is
    /// handled in [`Self::poll_file_dialog`]. Only one dialog at a time.
    pub fn open_file_dialog(&mut self, spec: files::Spec, purpose: files::Purpose) {
        if self.file_dialog.is_none() {
            self.file_dialog = Some(files::Pending::open(spec, purpose));
        }
    }

    /// Called every frame: finishes the job of a file dialog that was closed.
    fn poll_file_dialog(&mut self, ctx: &egui::Context) {
        let Some(pending) = &self.file_dialog else { return };
        let Some(chosen) = pending.poll() else {
            // keep polling while the dialog is open
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
            return;
        };
        let purpose = self.file_dialog.take().unwrap().purpose;
        if chosen.is_empty() {
            return; // cancelled
        }
        let first = chosen[0].clone();
        match purpose {
            files::Purpose::OpenService => self.open_service(first),
            files::Purpose::SaveServiceAs { then } => {
                self.write_new(WriteKind::Service, storage::with_extension(first, formats::SERVICE_EXT));
                self.after_service_saved(then);
            }
            files::Purpose::Export { kind, ext } => self.write_new(kind, storage::with_extension(first, &ext)),
            files::Purpose::ExportAllSongs => self.write(WriteKind::AllSongsOpenLyrics, first),
            files::Purpose::ImportSongs => self.import_song_files(chosen),
            files::Purpose::ImportDesigns => self.import_design_files(chosen),
            files::Purpose::ImportImages => self.import_image_files(chosen),
            files::Purpose::ImportVideos => self.import_video_files(chosen),
            files::Purpose::ImportBackup => self.import_backup_file(first),
            files::Purpose::ImportBible => self.import_bible_files(chosen),
            files::Purpose::ImportLegacy => self.import_legacy_files(chosen),
            files::Purpose::DesignerAddImage => {
                if let Some(media) = self.store_image(&first) {
                    self.designer_add(ObjectKind::Image { media });
                }
            }
            files::Purpose::DesignerReplaceImage(id) => {
                if let Some(m) = self.store_image(&first)
                    && let Some(ObjectKind::Image { media }) =
                        self.designer.design.doc.objects.iter_mut().find(|o| o.id == id).map(|o| &mut o.kind)
                {
                    *media = m;
                }
            }
        }
    }

    /// A small modal while a system file dialog is open, so nothing is changed
    /// underneath it (the output window keeps running).
    fn show_file_dialog_wait(&mut self, ctx: &egui::Context) {
        if self.file_dialog.is_none() {
            return;
        }
        egui::Modal::new(egui::Id::unique("file_dialog_wait")).show(ctx, |ui| {
            ui.set_max_width(420.0);
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label(t("file.dialog_open"));
            });
        });
    }

    /// Writes to a newly chosen path: asks before replacing an existing file.
    pub fn write_new(&mut self, kind: WriteKind, path: PathBuf) {
        let is_own_service = matches!(kind, WriteKind::Service) && self.service_path.as_ref() == Some(&path);
        if path.exists() && !is_own_service {
            self.dialog = Some(Dialog::FileExists { path, kind });
        } else {
            self.write(kind, path);
        }
    }

    /// Picks a file to export to, then writes it (with overwrite protection).
    pub fn export_to(&mut self, kind: WriteKind, default_name: &str, filter: &str, ext: &str) {
        self.open_file_dialog(
            files::Spec::new(files::Mode::SaveFile, t("menu.export")).filter(filter, &[ext]).file_name(default_name),
            files::Purpose::Export { kind, ext: ext.to_string() },
        );
    }

    pub fn write(&mut self, kind: WriteKind, path: PathBuf) {
        let file = path.to_string_lossy().to_string();
        let result: Result<String, String> = match &kind {
            WriteKind::Service => formats::save_service(&path, &self.service).map(|_| {
                self.service_path = Some(path.clone());
                self.service_mtime = storage::mtime(&path);
                self.service_dirty = false;
                self.settings.add_recent(&file);
                let _ = self.settings.save();
                tf("service.saved", &[("file", &file)])
            }),
            WriteKind::ServiceOsz => formats::export_osz(&path, &self.service).map(|skipped| {
                if skipped.is_empty() {
                    tf("export.done", &[("file", &file)])
                } else {
                    tf("export.osz_skipped", &[("file", &file), ("items", &skipped.join(", "))])
                }
            }),
            WriteKind::SongOpenLyrics(song) => storage::write_atomic(&path, formats::song_to_openlyrics(song).as_bytes(), true)
                .map(|_| tf("export.done", &[("file", &file)]))
                .map_err(|e| e.to_string()),
            WriteKind::AllSongsOpenLyrics => self.export_all_songs(&path),
            WriteKind::Design(d) => formats::export_design(&path, d).map(|_| tf("export.done", &[("file", &file)])),
            WriteKind::Backup => formats::export_backup(&path).map(|n| tf("export.backup_done", &[("file", &file), ("n", &n.to_string())])),
            WriteKind::LangTemplate => storage::write_atomic(&path, crate::i18n::BUILTIN_EN.as_bytes(), true)
                .map(|_| tf("export.done", &[("file", &file)]))
                .map_err(|e| e.to_string()),
        };
        match result {
            Ok(msg) => self.notify(msg),
            Err(e) => self.error(tf("error.save", &[("file", &file), ("err", &e)])),
        }
    }

    /// Exports every song into a folder (chosen as `dir`), one OpenLyrics file each.
    /// Existing files are never replaced: clashing names get a number.
    fn export_all_songs(&mut self, dir: &std::path::Path) -> Result<String, String> {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        for song in &self.lib.songs {
            let path = storage::unique_path(&dir.join(format!("{}.xml", storage::safe_file_name(&song.title))));
            storage::write_atomic(&path, formats::song_to_openlyrics(song).as_bytes(), false).map_err(|e| e.to_string())?;
        }
        Ok(tf("export.songs_done", &[("n", &self.lib.songs.len().to_string()), ("file", &dir.to_string_lossy())]))
    }

    // -----------------------------------------------------------------------
    // Songs / designs in the library

    pub fn save_song(&mut self, as_new: bool, confirmed: bool) {
        let mut song = self.songs.song.clone();
        song.title = song.title.trim().to_string();
        if song.title.is_empty() {
            return self.error(t("songs.need_title"));
        }
        if as_new {
            song.id = make_id();
            if self.lib.title_clash(&song) || self.lib.songs.iter().any(|s| s.title == song.title) {
                song.title = self.unique_song_title(&song.title);
            }
        } else if !confirmed && self.lib.title_clash(&song) {
            self.dialog = Some(Dialog::SongTitleClash { as_new });
            return;
        }
        match self.lib.save_song(&song) {
            Ok(()) => {
                // refresh copies of this song in the service
                let mut updated = 0;
                for item in &mut self.service.items {
                    if let ItemKind::Song(s) = &mut item.kind
                        && s.id == song.id
                        && *s != song
                    {
                        *s = song.clone();
                        updated += 1;
                    }
                }
                if updated > 0 {
                    self.service_dirty = true;
                }
                self.notify(if updated > 0 {
                    tf("songs.saved_and_updated", &[("title", &song.title), ("n", &updated.to_string())])
                } else {
                    tf("songs.saved", &[("title", &song.title)])
                });
                self.songs.song = song;
                self.songs.in_library = true;
                self.songs.dirty = false;
            }
            Err(e) => self.error(tf("error.save", &[("file", &song.title), ("err", &e.to_string())])),
        }
    }

    fn unique_song_title(&self, title: &str) -> String {
        (2..).map(|n| format!("{title} ({n})")).find(|t| !self.lib.songs.iter().any(|s| &s.title == t)).unwrap()
    }

    pub fn save_design(&mut self, as_new: bool, confirmed: bool) {
        let mut d = self.designer.design.clone();
        d.name = d.name.trim().to_string();
        if d.name.is_empty() {
            return self.error(t("designer.need_name"));
        }
        if as_new {
            d.id = make_id();
            if self.lib.designs.iter().any(|x| x.name == d.name) {
                d.name = (2..).map(|n| format!("{} ({n})", d.name)).find(|t| !self.lib.designs.iter().any(|x| &x.name == t)).unwrap();
            }
        } else if !confirmed && self.lib.design_name_clash(&d) {
            self.dialog = Some(Dialog::DesignNameClash { as_new });
            return;
        }
        match self.lib.save_design(&d) {
            Ok(()) => {
                self.notify(tf("designer.saved", &[("name", &d.name)]));
                self.designer.design = d;
                self.designer.in_library = true;
                self.designer.dirty = false;
            }
            Err(e) => self.error(tf("error.save", &[("file", &d.name), ("err", &e.to_string())])),
        }
    }

    pub fn add_to_service(&mut self, kind: ItemKind) {
        let pos = match self.sel_item {
            Some(ItemRef::Service(i)) => i + 1,
            _ => self.service.items.len(),
        };
        let title = ServiceItem::new(kind.clone()).title();
        self.service.items.insert(pos, ServiceItem::new(kind));
        self.service_dirty = true;
        // keep the live position pointing at the same item
        if let Some((ItemRef::Service(i), s)) = self.live_pos
            && i >= pos
        {
            self.live_pos = Some((ItemRef::Service(i + 1), s));
        }
        self.notify(tf("service.added", &[("title", &title)]));
    }

    // -----------------------------------------------------------------------
    // Imports

    pub fn import_songs(&mut self) {
        self.open_file_dialog(
            files::Spec::new(files::Mode::PickFiles, t("import.songs"))
                .filter(t("file.song_filter"), &["xml", "cho", "chordpro", "chopro", "pro", "txt", "ohdata", "sqlite"]),
            files::Purpose::ImportSongs,
        );
    }

    fn import_song_files(&mut self, files: Vec<PathBuf>) {
        let (mut added, mut same, mut errors) = (0, 0, Vec::new());
        for f in files {
            match formats::import_song_file(&f) {
                Ok(songs) => {
                    for s in songs {
                        match self.lib.import_song(s) {
                            Ok(true) => added += 1,
                            Ok(false) => same += 1,
                            Err(e) => errors.push(e.to_string()),
                        }
                    }
                }
                Err(e) => errors.push(format!("{}: {e}", f.display())),
            }
        }
        let msg = tf("import.songs_done", &[("n", &added.to_string()), ("same", &same.to_string())]);
        if errors.is_empty() {
            self.notify(msg);
        } else {
            self.error(format!("{msg}\n\n{}", errors.join("\n")));
        }
    }

    pub fn import_designs(&mut self) {
        self.open_file_dialog(
            files::Spec::new(files::Mode::PickFiles, t("import.designs"))
                .filter(t("file.design_filter"), &[formats::DESIGN_EXT, "ohcanvas"]),
            files::Purpose::ImportDesigns,
        );
    }

    fn import_design_files(&mut self, files: Vec<PathBuf>) {
        let mut errors = Vec::new();
        let mut n = 0;
        for f in files {
            match formats::import_design_file(&f).and_then(|d| self.lib.import_design(d).map_err(|e| e.to_string())) {
                Ok(true) => n += 1,
                Ok(false) => {}
                Err(e) => errors.push(format!("{}: {e}", f.display())),
            }
        }
        let msg = tf("import.designs_done", &[("n", &n.to_string())]);
        if errors.is_empty() { self.notify(msg) } else { self.error(format!("{msg}\n\n{}", errors.join("\n"))) }
    }

    pub fn import_image_slides(&mut self) {
        self.open_file_dialog(
            files::Spec::new(files::Mode::PickFiles, t("import.images")).filter(t("file.image_filter"), IMAGE_EXTS),
            files::Purpose::ImportImages,
        );
    }

    fn import_image_files(&mut self, files: Vec<PathBuf>) {
        for f in files {
            match formats::read_image_file(&f)
                .and_then(|(b, ext)| storage::store_media_bytes(&b, &ext).map(|m| (m, b)).map_err(|e| e.to_string()))
            {
                Ok((media, bytes)) => {
                    let name = f.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
                    self.add_to_service(ItemKind::Design(formats::full_screen_image(&name, media, &bytes)));
                }
                Err(e) => self.error(tf("error.open", &[("file", &f.to_string_lossy()), ("err", &e)])),
            }
        }
    }

    pub fn import_backup(&mut self) {
        self.open_file_dialog(
            files::Spec::new(files::Mode::PickFile, t("import.backup")).filter(t("file.backup_filter"), &[formats::BACKUP_EXT]),
            files::Purpose::ImportBackup,
        );
    }

    fn import_backup_file(&mut self, f: PathBuf) {
        match formats::import_backup(&f, &mut self.lib) {
            Ok(r) => {
                self.bible.bibles = crate::bible::list_bibles();
                self.bible.bookmarks = Bookmarks::load();
                self.notify(tf(
                    "import.backup_done",
                    &[
                        ("songs", &r.songs.to_string()),
                        ("designs", &r.designs.to_string()),
                        ("bibles", &r.bibles.to_string()),
                        ("skipped", &r.skipped.to_string()),
                    ],
                ));
            }
            Err(e) => self.error(tf("error.open", &[("file", &f.to_string_lossy()), ("err", &e)])),
        }
    }

    pub fn import_bible(&mut self) {
        self.open_file_dialog(
            files::Spec::new(files::Mode::PickFiles, t("import.bible")).filter(t("file.bible_filter"), &["xml", "osis", "zef", "sqlite"]),
            files::Purpose::ImportBible,
        );
    }

    fn import_bible_files(&mut self, files: Vec<PathBuf>) {
        for f in files {
            match crate::bible::import_file(&f) {
                Ok(bible) if bible.verse_count() == 0 => self.error(tf("bible.import_empty", &[("file", &f.to_string_lossy())])),
                Ok(bible) => {
                    if let Err(e) = bible.save() {
                        self.error(e.to_string());
                        continue;
                    }
                    self.notify(tf("bible.imported", &[("name", &bible.meta.name), ("n", &bible.verse_count().to_string())]));
                    self.settings.bible = Some(bible.meta.id.clone());
                    self.bible.current = Some(bible);
                    self.bible.bibles = crate::bible::list_bibles();
                    self.bible.index = None;
                    self.bible.offer_index = None;
                    self.save_settings();
                    // a bible in a language with word search gets its index right away
                    self.build_missing_indexes(false);
                }
                Err(e) => self.error(tf("error.open", &[("file", &f.to_string_lossy()), ("err", &e)])),
            }
        }
    }

    // -----------------------------------------------------------------------
    // Keyboard

    fn handle_actions(&mut self, actions: Vec<Action>, ctx: &egui::Context) {
        for a in actions {
            match a {
                Action::NewService => self.request(Then::NewService),
                Action::OpenService => self.request(Then::OpenService(None)),
                Action::Save => match self.tab {
                    Tab::Songs => self.save_song(false, false),
                    Tab::Designer => self.save_design(false, false),
                    _ => self.save_service(),
                },
                Action::SaveAs => match self.tab {
                    Tab::Songs => self.save_song(true, false),
                    Tab::Designer => self.save_design(true, false),
                    _ => self.save_service_as(),
                },
                Action::NextSlide => self.step_slide(true),
                Action::PrevSlide => self.step_slide(false),
                Action::FirstSlide => {
                    if let Some(i) = self.sel_item {
                        self.select(i, 0)
                    }
                }
                Action::LastSlide => {
                    if let Some(i) = self.sel_item {
                        let n = self.slides_of(i).len();
                        self.select(i, n.saturating_sub(1))
                    }
                }
                Action::NextItem => self.step_item(true),
                Action::PrevItem => self.step_item(false),
                Action::GoLive => self.go_live(),
                Action::ToggleLive => {
                    self.settings.live_follows_selection = !self.settings.live_follows_selection;
                    self.save_settings();
                }
                Action::Blank => self.toggle_blank(),
                Action::ToggleOutput => self.output_open = !self.output_open,
                Action::Alert => self.open_alert_dialog(),
                Action::OutputFullscreen => {
                    self.output_open = true;
                    self.output_fullscreen = !self.output_fullscreen;
                    ctx.send_viewport_cmd_to(output_id(), ViewportCommand::Fullscreen(self.output_fullscreen));
                }
                Action::Find => {
                    if self.tab == Tab::Service || self.tab == Tab::Settings {
                        self.tab = Tab::Songs;
                    }
                    self.focus_search = true;
                }
                Action::TabService => self.tab = Tab::Service,
                Action::TabSongs => self.tab = Tab::Songs,
                Action::TabDesigner => self.tab = Tab::Designer,
                Action::TabBible => self.tab = Tab::Bible,
                Action::TabSettings => self.tab = Tab::Settings,
                Action::PinPassage => self.pin_current_passage(),
                Action::Undo => self.designer_undo(),
                Action::Redo => self.designer_redo(),
                Action::Quit => self.request(Then::Quit),
            }
        }
    }

    /// Whether slide navigation keys are active (where slides are being presented).
    fn presenting(&self) -> bool {
        matches!(self.tab, Tab::Service | Tab::Bible)
    }

    fn action_enabled(&self, a: Action) -> bool {
        match a {
            Action::PinPassage => self.tab == Tab::Bible,
            Action::Undo | Action::Redo => self.tab == Tab::Designer,
            // arrow keys move objects in the designer
            a if a.is_navigation() => self.presenting(),
            _ => true,
        }
    }

    /// Keys pressed in the main window.
    fn handle_keys(&mut self, ctx: &egui::Context) {
        if self.capture.is_some() || self.dialog.is_some() || self.file_dialog.is_some() {
            return;
        }
        let shortcuts = self.settings.shortcuts.clone();
        let actions = shortcuts.triggered(ctx, |a| self.action_enabled(a));
        if !actions.is_empty() {
            // a shortcut (Ctrl+1, F5, …) closes an open menu, as in other programs
            egui::Popup::close_all(ctx);
        }
        self.handle_actions(actions, ctx);
        if self.presenting()
            && !ctx.egui_wants_keyboard_input()
            && let Some((code, n)) = self.mark_keys().consume(ctx)
        {
            egui::Popup::close_all(ctx);
            self.jump_to_mark(&code, n);
        }
    }

    fn mark_keys(&self) -> MarkKeys {
        MarkKeys {
            marks: self.settings.marks.iter().filter_map(|m| m.key.map(|k| (m.code.clone(), k))).collect(),
            numbers: self.settings.number_keys_jump_to_verse,
        }
    }

    // -----------------------------------------------------------------------
    // Bible helpers used by shortcuts

    pub fn pin_current_passage(&mut self) {
        if let Some(p) = self.bible_selected_passage() {
            let bible = self.bible.current.as_ref().map(|b| b.meta.id.clone()).unwrap_or_default();
            self.bible.bookmarks.toggle_pin(crate::bible::Bookmark {
                reference: p.reference.clone(),
                bible: bible.clone(),
                note: String::new(),
            });
            let key = if self.bible.bookmarks.is_pinned(&p.reference, &bible) { "bible.pinned" } else { "bible.unpinned" };
            self.notify(tf(key, &[("ref", &p.reference)]));
        }
    }

    // -----------------------------------------------------------------------
    // Output window

    /// Shows the output window. Presenting keys pressed while it has focus work there too.
    /// The output (projector) window. It is a *deferred* viewport: it is repainted only
    /// when its slide changes or it gets input itself, not on every frame of the main
    /// window. (As an immediate viewport it was painted inside every main frame, and
    /// each paint waited for that window's vsync, so with two windows — on two screens
    /// — every frame waited twice and the whole program crawled.)
    fn show_output(&mut self, ctx: &egui::Context) {
        if !self.output_open {
            return;
        }
        let scene = if self.blank { Scene::Blank } else { self.live.clone() };
        let (actions, jump, fullscreen, close) = {
            let mut o = self.output.lock().unwrap_or_else(|e| e.into_inner());
            if o.scene != scene {
                o.scene = scene;
                ctx.request_repaint_of(output_id());
            }
            let alert = self.alert.as_ref().map(|(a, _)| a);
            if o.alert.as_ref().map(|(a, _)| a) != alert {
                o.alert = alert.map(|a| (a.clone(), Instant::now()));
                ctx.request_repaint_of(output_id());
            }
            o.keys_enabled = self.capture.is_none() && self.dialog.is_none() && self.file_dialog.is_none();
            o.shortcuts = self.settings.shortcuts.clone();
            o.mark_keys = self.mark_keys();
            (std::mem::take(&mut o.actions), o.jump.take(), o.fullscreen.take(), std::mem::take(&mut o.close))
        };
        let shared = self.output.clone();
        ctx.show_viewport_deferred(
            output_id(),
            egui::ViewportBuilder::default().with_title(t("output.title")).with_inner_size([960.0, 540.0]).with_window_level(
                if self.settings.output_always_on_top { egui::WindowLevel::AlwaysOnTop } else { egui::WindowLevel::Normal },
            ),
            move |ui, _class| {
                let mut guard = shared.lock().unwrap_or_else(|e| e.into_inner());
                let o = &mut *guard;
                let vctx = ui.ctx().clone();
                vctx.input_mut(|i| shortcuts::digits_by_position(&mut i.events));
                let rect = ui.max_rect();
                let painter = ui.painter_at(rect);
                painter.rect_filled(rect, 0.0, egui::Color32::BLACK);
                let textures = o.textures.get_or_insert_with(|| TextureCache::new(storage::paths().media()));
                let slide = render::fit_16_9(rect);
                render::draw_scene_live(&painter, slide, &o.scene, textures);
                if let Some((alert, since)) = &o.alert
                    && render::draw_alert(&painter, slide, alert, since.elapsed().as_secs_f32())
                {
                    // ~60 frames per second while it runs
                    vctx.request_repaint_after(std::time::Duration::from_millis(16));
                }
                let mut wake_main = false;
                // double click toggles fullscreen, Esc leaves it
                let resp = ui.interact(rect, egui::Id::unique("output_area"), egui::Sense::click());
                if resp.double_clicked() {
                    o.fullscreen = Some(true);
                    wake_main = true;
                }
                if vctx.input(|i| i.key_pressed(Key::Escape)) {
                    o.fullscreen = Some(false);
                    wake_main = true;
                }
                if vctx.input(|i| i.viewport().close_requested()) {
                    o.close = true;
                    wake_main = true;
                }
                if o.keys_enabled && vctx.input(|i| i.focused) {
                    let actions =
                        o.shortcuts.triggered(&vctx, |a| a.is_navigation() || matches!(a, Action::ToggleOutput | Action::OutputFullscreen));
                    if let Some(j) = o.mark_keys.consume(&vctx) {
                        o.jump = Some(j);
                        wake_main = true;
                    }
                    if !actions.is_empty() {
                        o.actions.extend(actions);
                        wake_main = true;
                    }
                }
                if wake_main {
                    // the main window does the work (and sends the new slide back here)
                    vctx.request_repaint_of(ViewportId::ROOT);
                }
            },
        );
        self.handle_actions(actions, ctx);
        if let Some((code, n)) = jump {
            self.jump_to_mark(&code, n);
        }
        match fullscreen {
            Some(true) => self.output_fullscreen = !self.output_fullscreen,
            Some(false) => self.output_fullscreen = false,
            None => {}
        }
        if fullscreen.is_some() {
            ctx.send_viewport_cmd_to(output_id(), ViewportCommand::Fullscreen(self.output_fullscreen));
        }
        if close {
            self.output_open = false;
            self.output_fullscreen = false;
        }
    }

    // -----------------------------------------------------------------------
    // Dialogs

    fn show_dialog(&mut self, ctx: &egui::Context) {
        let Some(dialog) = self.dialog.clone() else { return };
        let mut close = false;
        let mut next: Option<DialogAction> = None;
        egui::Modal::new(egui::Id::unique("dialog")).show(ctx, |ui| {
            ui.set_max_width(480.0);
            let mut btn = |ui: &mut egui::Ui, label: String, f: Option<DialogAction>| {
                if ui.button(label).clicked() {
                    close = true;
                    next = f;
                }
            };
            match &dialog {
                Dialog::Message(m) => {
                    ui.label(m);
                    ui.add_space(8.0);
                    btn(ui, t("ok"), None);
                }
                Dialog::FileExists { path, kind } => {
                    ui.heading(t("dialog.file_exists"));
                    ui.label(tf("dialog.file_exists_text", &[("file", &path.to_string_lossy())]));
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        let unique = storage::unique_path(path);
                        let (k1, k2, p1) = (kind.clone(), kind.clone(), path.clone());
                        btn(
                            ui,
                            tf("dialog.save_as_copy", &[("name", &unique.file_name().unwrap_or_default().to_string_lossy())]),
                            Some(Box::new(move |app| app.write(k1, unique))),
                        );
                        btn(ui, t("dialog.overwrite"), Some(Box::new(move |app| app.write(k2, p1))));
                        btn(ui, t("cancel"), None);
                    });
                }
                Dialog::ChangedOnDisk { path } => {
                    ui.heading(t("dialog.changed_on_disk"));
                    ui.label(tf("dialog.changed_on_disk_text", &[("file", &path.to_string_lossy())]));
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        btn(ui, t("service.save_as"), Some(Box::new(|app| app.save_service_as())));
                        let p = path.clone();
                        btn(ui, t("dialog.overwrite"), Some(Box::new(move |app| app.write(WriteKind::Service, p))));
                        btn(ui, t("cancel"), None);
                    });
                }
                Dialog::UnsavedService { then } => {
                    ui.heading(t("dialog.unsaved_service"));
                    ui.label(t("dialog.unsaved_service_text"));
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        let th = then.clone();
                        btn(ui, t("service.save"), Some(Box::new(move |app| app.save_service_then(Some(th)))));
                        let th = then.clone();
                        btn(ui, t("dialog.discard"), Some(Box::new(move |app| app.run(th))));
                        btn(ui, t("cancel"), None);
                    });
                }
                Dialog::UnsavedSong { then } => {
                    ui.heading(t("dialog.unsaved_song"));
                    ui.label(tf("dialog.unsaved_song_text", &[("title", &self.songs.song.title)]));
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        let th = then.clone();
                        btn(
                            ui,
                            t("songs.save"),
                            Some(Box::new(move |app| {
                                app.save_song(false, true);
                                if !app.songs.dirty {
                                    app.run(th);
                                }
                            })),
                        );
                        let th = then.clone();
                        btn(ui, t("dialog.discard"), Some(Box::new(move |app| app.run(th))));
                        btn(ui, t("cancel"), None);
                    });
                }
                Dialog::UnsavedDesign { then } => {
                    ui.heading(t("dialog.unsaved_design"));
                    ui.label(tf("dialog.unsaved_design_text", &[("name", &self.designer.design.name)]));
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        let th = then.clone();
                        btn(
                            ui,
                            t("designer.save"),
                            Some(Box::new(move |app| {
                                app.save_design(false, true);
                                if !app.designer.dirty {
                                    app.run(th);
                                }
                            })),
                        );
                        let th = then.clone();
                        btn(ui, t("dialog.discard"), Some(Box::new(move |app| app.run(th))));
                        btn(ui, t("cancel"), None);
                    });
                }
                Dialog::UnsavedOnQuit => {
                    ui.heading(t("dialog.unsaved_quit"));
                    if self.service_dirty {
                        ui.label(format!("• {}", t("dialog.unsaved_quit_service")));
                    }
                    if self.songs.dirty {
                        ui.label(format!("• {}", tf("dialog.unsaved_quit_song", &[("title", &self.songs.song.title)])));
                    }
                    if self.designer.dirty {
                        ui.label(format!("• {}", tf("dialog.unsaved_quit_design", &[("name", &self.designer.design.name)])));
                    }
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        btn(
                            ui,
                            t("dialog.save_all_quit"),
                            Some(Box::new(|app| {
                                if app.songs.dirty {
                                    app.save_song(false, true);
                                }
                                if app.designer.dirty {
                                    app.save_design(false, true);
                                }
                                if app.songs.dirty || app.designer.dirty || app.dialog.is_some() {
                                    return; // a song / design could not be saved: stay
                                }
                                if app.service_dirty {
                                    // quits once the service is saved (a new one asks for a file first)
                                    app.save_service_then(Some(Then::Quit));
                                } else {
                                    app.quit_confirmed = true;
                                }
                            })),
                        );
                        btn(ui, t("dialog.quit_discard"), Some(Box::new(|app| app.quit_confirmed = true)));
                        btn(ui, t("cancel"), None);
                    });
                }
                Dialog::ConfirmQuit => {
                    ui.heading(t("dialog.confirm_quit"));
                    ui.label(t("dialog.confirm_quit_text"));
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        btn(ui, t("menu.quit"), Some(Box::new(|app| app.quit_confirmed = true)));
                        btn(ui, t("cancel"), None);
                    });
                }
                Dialog::SongTitleClash { as_new } => {
                    ui.heading(t("dialog.title_clash"));
                    ui.label(tf("dialog.title_clash_text", &[("title", &self.songs.song.title)]));
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        let a = *as_new;
                        btn(ui, t("dialog.save_anyway"), Some(Box::new(move |app| app.save_song(a, true))));
                        btn(ui, t("cancel"), None);
                    });
                }
                Dialog::DesignNameClash { as_new } => {
                    ui.heading(t("dialog.title_clash"));
                    ui.label(tf("dialog.title_clash_text", &[("title", &self.designer.design.name)]));
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        let a = *as_new;
                        btn(ui, t("dialog.save_anyway"), Some(Box::new(move |app| app.save_design(a, true))));
                        btn(ui, t("cancel"), None);
                    });
                }
                Dialog::DeleteSong(id) => {
                    let title = self.lib.song(id).map(|s| s.title.clone()).unwrap_or_default();
                    ui.label(tf("dialog.delete_song", &[("title", &title)]));
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        let id = id.clone();
                        btn(
                            ui,
                            t("delete"),
                            Some(Box::new(move |app| {
                                if let Err(e) = app.lib.delete_song(&id) {
                                    app.error(e.to_string());
                                } else if app.songs.song.id == id {
                                    app.songs.in_library = false;
                                    app.songs.dirty = true;
                                }
                            })),
                        );
                        btn(ui, t("cancel"), None);
                    });
                }
                Dialog::DeleteDesign(id) => {
                    let name = self.lib.design(id).map(|s| s.name.clone()).unwrap_or_default();
                    ui.label(tf("dialog.delete_design", &[("name", &name)]));
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        let id = id.clone();
                        btn(
                            ui,
                            t("delete"),
                            Some(Box::new(move |app| {
                                if let Err(e) = app.lib.delete_design(&id) {
                                    app.error(e.to_string());
                                } else if app.designer.design.id == id {
                                    app.designer.in_library = false;
                                    app.designer.dirty = true;
                                }
                            })),
                        );
                        btn(ui, t("cancel"), None);
                    });
                }
                Dialog::RemoveSongColumn(c) => {
                    let name = self.songs.song.columns.get(*c).map(|c| c.name.clone()).unwrap_or_default();
                    ui.label(tf("dialog.remove_column", &[("name", &name)]));
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        let c = *c;
                        btn(
                            ui,
                            t("delete"),
                            Some(Box::new(move |app| {
                                app.songs.song.remove_column(c);
                                app.songs.dirty = true;
                            })),
                        );
                        btn(ui, t("cancel"), None);
                    });
                }
                Dialog::LegacyImport => {
                    self.legacy_import_dialog(ui, &mut close);
                }
                Dialog::Alert => {
                    self.alert_dialog(ui, &mut close);
                }
                Dialog::BibleIndexLanguage(suggested) => {
                    self.bible_index_dialog(ui, suggested, &mut close);
                }
                Dialog::DeleteBible(id) => {
                    let name = self.bible.bibles.iter().find(|b| &b.id == id).map(|b| b.name.clone()).unwrap_or_default();
                    ui.label(tf("dialog.delete_bible", &[("name", &name)]));
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        let id = id.clone();
                        btn(
                            ui,
                            t("delete"),
                            Some(Box::new(move |app| {
                                Bible::delete(&id);
                                app.bible.bibles = crate::bible::list_bibles();
                                if app.bible.current.as_ref().map(|b| b.meta.id == id).unwrap_or(false) {
                                    app.bible.current = None;
                                    app.bible.results.clear();
                                    app.bible.index = None;
                                }
                            })),
                        );
                        btn(ui, t("cancel"), None);
                    });
                }
            }
        });
        // Esc = Cancel (or OK for a plain message) in every dialog
        if !close && ctx.input(|i| i.key_pressed(Key::Escape)) {
            close = true;
            next = None;
            match dialog {
                // "Later": ask again on the next start (and not again right away)
                Dialog::LegacyImport => self.legacy = None,
                Dialog::BibleIndexLanguage(_) => self.bible.index_choice = None,
                _ => {}
            }
        }
        if close {
            self.dialog = None;
            if let Some(f) = next {
                f(self);
            }
        }
    }

    // -----------------------------------------------------------------------
    // Menu and chrome

    fn menu_bar(&mut self, ui: &mut egui::Ui) {
        egui::MenuBar::new().ui(ui, |ui| {
            let sc = self.settings.shortcuts.clone();
            let item = |ui: &mut egui::Ui, label: String, action: Option<Action>| {
                let mut b = egui::Button::new(label);
                if let Some(a) = action {
                    b = b.shortcut_text(sc.describe(a));
                }
                ui.add(b).clicked()
            };
            let open_menu = self.alt_nav.open_menu.take();
            let file_menu = ui.menu_button(t("menu.file"), |ui| {
                if item(ui, t("service.new"), Some(Action::NewService)) {
                    self.request(Then::NewService);
                }
                if item(ui, t("service.open"), Some(Action::OpenService)) {
                    self.request(Then::OpenService(None));
                }
                ui.menu_button(t("menu.recent"), |ui| {
                    if self.settings.recent_services.is_empty() {
                        ui.weak(t("menu.no_recent"));
                    }
                    for r in self.settings.recent_services.clone() {
                        if ui.button(&r).clicked() {
                            self.request(Then::OpenService(Some(PathBuf::from(r))));
                        }
                    }
                });
                ui.separator();
                if item(ui, t("service.save"), None) {
                    self.save_service();
                }
                if item(ui, t("service.save_as"), None) {
                    self.save_service_as();
                }
                ui.separator();
                ui.menu_button(t("menu.import"), |ui| {
                    if ui.button(t("import.service")).on_hover_text(t("import.service_hint")).clicked() {
                        self.request(Then::OpenService(None));
                    }
                    if ui.button(t("import.songs")).on_hover_text(t("import.songs_hint")).clicked() {
                        self.import_songs();
                    }
                    if ui.button(t("import.designs")).clicked() {
                        self.import_designs();
                    }
                    if ui.button(t("import.images")).clicked() {
                        self.import_image_slides();
                    }
                    if ui.button(t("import.videos")).on_hover_text(t("video.add_hint")).clicked() {
                        self.import_videos();
                    }
                    if ui.button(t("import.bible")).on_hover_text(t("import.bible_hint")).clicked() {
                        self.import_bible();
                    }
                    ui.separator();
                    if ui.button(t("import.backup")).clicked() {
                        self.import_backup();
                    }
                    if ui.button(t("legacy.menu")).on_hover_text(t("legacy.menu_hint")).clicked() {
                        self.start_legacy_scan(true);
                    }
                });
                ui.menu_button(t("menu.export"), |ui| {
                    if ui.button(t("export.osz")).on_hover_text(t("export.osz_hint")).clicked() {
                        self.export_to(WriteKind::ServiceOsz, &format!("{}.osz", t("service.default_name")), "OpenLP", "osz");
                    }
                    if ui.button(t("export.all_songs")).clicked() {
                        self.open_file_dialog(
                            files::Spec::new(files::Mode::PickFolder, t("export.all_songs")),
                            files::Purpose::ExportAllSongs,
                        );
                    }
                    ui.separator();
                    if ui.button(t("export.backup")).on_hover_text(t("export.backup_hint")).clicked() {
                        self.export_to(
                            WriteKind::Backup,
                            &format!("oh-presenter.{}", formats::BACKUP_EXT),
                            &t("file.backup_filter"),
                            formats::BACKUP_EXT,
                        );
                    }
                });
                ui.separator();
                if item(ui, t("menu.quit"), Some(Action::Quit)) {
                    self.request(Then::Quit);
                }
            });
            let output_menu = ui.menu_button(t("menu.output"), |ui| {
                if item(ui, t("alert.menu"), Some(Action::Alert)) {
                    self.open_alert_dialog();
                }
                if item(ui, t("output.toggle"), Some(Action::ToggleOutput)) {
                    self.output_open = !self.output_open;
                }
                if item(ui, t("output.fullscreen"), Some(Action::OutputFullscreen)) {
                    self.handle_actions(vec![Action::OutputFullscreen], &ui.ctx().clone());
                }
                if item(ui, t("output.blank"), Some(Action::Blank)) {
                    self.toggle_blank();
                }
                ui.separator();
                if ui.button(t("output.start_client")).on_hover_text(t("output.start_client_hint")).clicked() {
                    self.start_local_client();
                }
            });
            self.alt_nav.menu_ids = [
                Some(egui::Popup::default_response_id(&file_menu.response)),
                Some(egui::Popup::default_response_id(&output_menu.response)),
            ];
            // Alt + letters asked for a menu (see keynav)
            if let Some(m) = open_menu {
                let r = if m == 0 { &file_menu.response } else { &output_menu.response };
                egui::Popup::open_id(ui.ctx(), egui::Popup::default_response_id(r));
            }
            ui.separator();
            for (tab, key, action) in [
                (Tab::Service, "tab.service", Action::TabService),
                (Tab::Songs, "tab.songs", Action::TabSongs),
                (Tab::Designer, "tab.designer", Action::TabDesigner),
                (Tab::Bible, "tab.bible", Action::TabBible),
                (Tab::Settings, "tab.settings", Action::TabSettings),
            ] {
                let mut label = t(key);
                if (tab == Tab::Service && self.service_dirty)
                    || (tab == Tab::Songs && self.songs.dirty)
                    || (tab == Tab::Designer && self.designer.dirty)
                {
                    label.push_str(" •");
                }
                if ui.selectable_label(self.tab == tab, label).on_hover_text(sc.describe(action)).clicked() {
                    self.tab = tab;
                }
            }
        });
    }

    pub fn start_local_client(&mut self) {
        match std::env::current_exe().and_then(|exe| std::process::Command::new(exe).arg("canvas").arg("127.0.0.1").spawn()) {
            Ok(_) => self.notify(t("output.client_started")),
            Err(e) => self.error(e.to_string()),
        }
    }

    fn status_bar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            let live = if self.settings.live_follows_selection { t("status.live_on") } else { t("status.live_off") };
            ui.label(live);
            ui.separator();
            if self.blank {
                ui.colored_label(LIVE_COLOR, t("status.blank"));
                ui.separator();
            }
            self.alert_status(ui);
            if let Some(s) = &self.server {
                let n = s.clients().len();
                ui.label(tf("status.clients", &[("n", &n.to_string())]));
                ui.separator();
            }
            let zoom = ui.ctx().zoom_factor();
            if (zoom - 1.0).abs() > 0.001 {
                let r = ui.add(egui::Button::new(format!("🔍 {:.0} %", zoom * 100.0)).small()).on_hover_text(t("status.zoom_reset"));
                if r.clicked() {
                    ui.ctx().set_zoom_factor(1.0);
                }
                ui.separator();
            }
            let file = self.service_path.as_ref().map(|p| p.to_string_lossy().to_string()).unwrap_or_else(|| t("service.unsaved_new"));
            ui.label(format!("{}{}", file, if self.service_dirty { " *" } else { "" }));
            if let Some((msg, at)) = &self.status
                && at.elapsed().as_secs() < 8
            {
                ui.separator();
                ui.label(msg);
            }
        });
    }
}

/// Keys that jump to marked song parts (and number keys to verses).
#[derive(Default)]
struct MarkKeys {
    marks: Vec<(String, KeyBinding)>,
    numbers: bool,
}

impl MarkKeys {
    fn consume(&self, ctx: &egui::Context) -> Option<(String, Option<u32>)> {
        let mut jump = None;
        ctx.input_mut(|i| {
            for (code, key) in &self.marks {
                if key.consume(i) {
                    jump = Some((code.clone(), None));
                }
            }
            if self.numbers {
                let nums = [Key::Num1, Key::Num2, Key::Num3, Key::Num4, Key::Num5, Key::Num6, Key::Num7, Key::Num8, Key::Num9];
                for (n, k) in nums.iter().enumerate() {
                    if KeyBinding::plain(*k).consume(i) {
                        jump = Some(("V".into(), Some(n as u32 + 1)));
                    }
                }
            }
        });
        jump
    }
}

/// Red of everything that is live (and of warnings).
pub const LIVE_COLOR: egui::Color32 = egui::Color32::from_rgb(230, 80, 80);

/// Picture formats that can be imported.
pub const IMAGE_EXTS: &[&str] = &[
    "png", "jpg", "jpeg", "jfif", "gif", "bmp", "webp", "tif", "tiff", "ico", "tga", "pbm", "pgm", "ppm", "pnm", "pam", "qoi", "dds",
    "hdr", "exr", "ff", "heic", "heif", "hif", "avif",
];

pub fn output_id() -> ViewportId {
    ViewportId::from_hash_of("oh-presenter-output")
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.frame(ui);
    }

    fn raw_input_hook(&mut self, ctx: &egui::Context, raw_input: &mut egui::RawInput) {
        self.menu_arrows_as_tab(ctx, raw_input);
    }
}

impl App {
    /// One frame of the whole UI.
    pub fn frame(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        self.limit_frame_rate();

        ctx.input_mut(|i| shortcuts::digits_by_position(&mut i.events));

        // closing the main window always asks first (and offers to save unsaved work)
        if ctx.input(|i| i.viewport().close_requested()) && !self.quit_confirmed {
            ctx.send_viewport_cmd(ViewportCommand::CancelClose);
            if self.dialog.is_none() {
                self.request(Then::Quit);
            }
        }
        if self.quit_confirmed {
            ctx.send_viewport_cmd(ViewportCommand::Close);
        }

        self.poll_file_dialog(&ctx);
        self.poll_legacy_scan(&ctx);
        self.poll_indexing(&ctx);
        self.poll_alert(&ctx);
        self.poll_video_import(&ctx);
        self.sync_video_audio();
        if self.tab != Tab::Settings {
            // a key capture started in Settings must not keep swallowing keys elsewhere
            self.capture = None;
        }
        self.show_output(&ctx);
        if ctx.input(|i| i.focused) {
            self.handle_keys(&ctx);
        }
        // after the shortcuts (an Alt shortcut someone set up still works), before the UI
        // (so the letters typed with Alt never reach a text field)
        self.alt_navigation(&ctx);
        self.keyboard_scroll(&ctx);

        egui::Panel::top("menu").show(ui, |ui| self.menu_bar(ui));
        egui::Panel::bottom("status").show(ui, |ui| self.status_bar(ui));
        egui::CentralPanel::default_margins().show(ui, |ui| match self.tab {
            Tab::Service => self.service_tab(ui),
            Tab::Songs => self.songs_tab(ui),
            Tab::Designer => self.designer_tab(ui),
            Tab::Bible => self.bible_tab(ui),
            Tab::Settings => self.settings_tab(ui),
        });
        self.show_dialog(&ctx);
        self.show_file_dialog_wait(&ctx);
        self.alt_navigation_hint(&ctx);
        self.keyboard_focus_frame(&ctx);
        if self.tab == Tab::Designer {
            self.designer_commit(&ctx);
        }
        if self.tab != Tab::Service {
            // the live preview (with its video decoder) is only on the Service tab
            self.textures.stop_video();
        }
        if self.server.is_some() {
            // keeps the client counter fresh
            ctx.request_repaint_after(std::time::Duration::from_secs(2));
        }
    }
}

#[cfg(test)]
pub fn settings_tab_capture(a: Action) -> settings_tab::Capture {
    settings_tab::Capture::Action(a)
}
