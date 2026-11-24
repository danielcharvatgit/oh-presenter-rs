//! The presenter application (control window + output window).

mod alerts;
mod bible_index;
mod bible_tab;
mod designer_tab;
pub(crate) mod files;
mod keynav;
mod legacy;
mod remote_cmd;
mod service_tab;
mod settings_tab;
mod songs_tab;
mod startup;
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
use keynav::FollowFocus;
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
    /// A bible (by id) as plain text.
    BibleTxt(String),
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
    /// The output is full screen on another screen (the operator does not see it): leave
    /// full screen / close it only when confirmed here.
    LeaveOutput(OutputChange),
    /// Rename a service item (F2); the name is typed into `App::rename_text`.
    RenameItem(usize),
}

/// A change of the output window that is confirmed first while it is full screen on
/// another screen (see [`App::request_output_change`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OutputChange {
    /// Out of full screen, into a window.
    Windowed,
    /// Close it (it opens full screen again).
    Close,
    /// Its own close button (or Alt+F4): closed, and opens as a window next time.
    CloseButton,
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

/// Putting the output window full screen on its screen (or a windowed one onto it), step by
/// step: every step is checked on a later frame of the output window, and tried another way
/// when the window did not get there (see [`crate::screens::fullscreen_step`]).
/// The window is never created full screen: created hidden and full screen at once, Windows
/// left it borderless on the primary screen at its first size (a quarter of the screen).
pub struct Placing {
    fullscreen: bool,
    done: u32,
    next_at: Instant,
}

impl Placing {
    fn new(fullscreen: bool) -> Self {
        // the first look once the new window has been shown
        Self { fullscreen, done: 0, next_at: Instant::now() + std::time::Duration::from_millis(120) }
    }
}

/// State shared with the output window (a deferred viewport, see [`App::show_output`]).
pub struct OutputShared {
    scene: Scene,
    alert: Option<(Alert, Instant)>,
    clock: ClockStyle,
    /// The output window's own texture cache (created on its first frame).
    textures: Option<TextureCache>,
    keys_enabled: bool,
    /// No mouse pointer over the output (with "keep above all other windows").
    hide_cursor: bool,
    /// When the output window was last marked to stay shown during Windows Peek
    /// (again now and then: a closed and reopened output window is a new window).
    peek_marked: Option<Instant>,
    shortcuts: shortcuts::Shortcuts,
    mark_keys: MarkKeys,
    /// The screen the output goes on (its index for egui and the screen itself).
    target: Option<(usize, crate::screens::Monitor)>,
    /// The screens (to tell which one the output window is on).
    monitors: Vec<crate::screens::Monitor>,
    /// Putting the output window on its screen (see [`Placing`]).
    placing: Option<Placing>,
    // filled by the output window, handled by the main window
    /// The screen the output window is on (its middle), as last seen.
    on_screen: Option<usize>,
    actions: Vec<Action>,
    jump: Option<(String, Option<u32>)>,
    fullscreen: Option<bool>,
    close: bool,
    /// Esc was pressed in the output window.
    escape: bool,
    /// The output must stay full screen (it is on another screen): put it back when the
    /// desktop takes it out ([`keep_output_fullscreen`]).
    guard_fullscreen: bool,
    /// Since when the output window is out of full screen without being asked to.
    lost_since: Option<Instant>,
    /// The output window was put back into full screen (the main window warns).
    lost_fullscreen: bool,
    /// Transparent output: what is behind the window shows through transparent pictures
    /// (setting `output_transparent`, the windows are created transparent at the start).
    transparent: bool,
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
    /// Freeze: the scene the outputs keep showing (and when its video state was set) while
    /// `live` goes on changing.
    pub frozen: Option<(Scene, Instant)>,
    /// Where in the Service tab was clicked last (for the video keys).
    pub service_area: Option<service_tab::ServiceArea>,
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
    /// The library being loaded at the start (the splash screen shows meanwhile).
    loading: Option<startup::Loading>,
    /// The screen (index of winit's monitors) the output window goes full screen on.
    pub output_monitor: Option<usize>,
    /// Names of the screens found at the start.
    screen_names: Vec<String>,
    /// The screens as last looked at (see [`App::refresh_output_monitor`]).
    monitors: Vec<crate::screens::Monitor>,
    /// The screen (index into `monitors`) the main window is on.
    main_monitor: Option<usize>,
    /// Where to move the main window on its first frame (onto the primary screen).
    pending_main_position: Option<egui::Pos2>,
    /// The main window (for its screens); `None` in tests.
    root_window: Option<std::sync::Arc<winit::window::Window>>,
    /// Frames left to wait for the screen the main window is on (Wayland knows it only
    /// once the window is shown); 0 = the windows are placed.
    placement_tries: u32,
    /// The output window was open on the last frame (to notice it opening).
    output_was_open: bool,
    /// The Service tab's columns on the last frame (for the keyboard, see service_tab).
    service_cols: service_tab::Columns,
    /// Ids of the rows of the service item list (last frame): Space there is the video key.
    service_row_ids: Vec<egui::Id>,
    /// The name typed in the rename dialog (F2).
    pub rename_text: String,
    /// `--cli`: no program window, the root window is the output (see [`crate::cli`]).
    pub cli: bool,
    /// Commands from the command line and the MCP server (see [`crate::remote`]).
    remote_rx: std::sync::mpsc::Receiver<crate::remote::Request>,
    remote_tx: std::sync::mpsc::Sender<crate::remote::Request>,
    /// The MCP server while it is on (Settings › Advanced), and why it could not start.
    mcp: Option<crate::remote::mcp::Server>,
    pub mcp_error: Option<String>,
    /// The MCP server is on whatever the settings say (`--mcp` on the command line).
    pub mcp_forced: bool,
    /// Keeps the desktop's keys off the projector while presenting (created when needed).
    desktop_guard: Option<crate::desktop_guard::Guard>,
    /// The windows were created transparent (setting `output_transparent` at the start).
    transparent_windows: bool,
    /// For the threads that send commands (they wake the UI up).
    remote_ctx: Option<egui::Context>,
    /// Commands of the command line's options, run once the library is loaded.
    pub startup_commands: Vec<(String, serde_json::Map<String, serde_json::Value>)>,
}

impl App {
    /// The program as the command line asks for it (see [`crate::cli`]).
    pub fn with_options(cc: &eframe::CreationContext, options: crate::cli::Options) -> Self {
        let mut app = Self::new_inner(cc, options.cli, !options.windowed);
        app.mcp_forced = options.mcp;
        if let Some(port) = options.mcp_port {
            app.settings.mcp_port = port;
        }
        app.startup_commands = options.startup_commands();
        if options.cli {
            // commands typed on the command line
            let sender = app.remote_sender(&cc.egui_ctx);
            std::thread::spawn(move || crate::remote::stdin_loop(sender));
            println!("oh! Presenter {} — type help for the commands, quit to end", env!("CARGO_PKG_VERSION"));
        }
        app
    }

    fn new_inner(cc: &eframe::CreationContext, cli: bool, fullscreen: bool) -> Self {
        let settings = Settings::load();
        crate::i18n::load_dir(&storage::paths().lang());
        crate::i18n::set_language(&settings.language);
        // the window appears right away with the splash screen; the library loads meanwhile
        let loading = startup::Loading::start(&cc.egui_ctx, settings.bible.clone());
        let mut app = Self::with_loaded(&cc.egui_ctx, settings, startup::Loaded::empty());
        app.loading = Some(loading);
        if cli {
            app.cli = true;
            app.output_open = true;
            app.output_fullscreen = fullscreen;
        }
        if let Some(window) = cc.winit_window() {
            app.place_windows(window);
        }
        app
    }

    /// The app with everything loaded right away (tests).
    #[cfg(test)]
    pub fn with_settings(ctx: &egui::Context, settings: Settings) -> Self {
        let loaded = startup::Loaded::load(settings.bible.clone(), &mut |_| {});
        Self::with_loaded(ctx, settings, loaded)
    }

    fn with_loaded(ctx: &egui::Context, settings: Settings, loaded: startup::Loaded) -> Self {
        let (remote_tx, remote_rx) = std::sync::mpsc::channel();
        render::install_fonts(ctx, None);
        // Ctrl+Q goes through our own (customisable) Quit action, which asks first
        ctx.options_mut(|o| o.quit_shortcuts.clear());
        ctx.set_visuals(if settings.dark_mode { egui::Visuals::dark() } else { egui::Visuals::light() });
        let server = settings.network_enabled.then(|| Server::start(settings.network_port));
        let bible = BibleUi {
            bibles: loaded.bibles,
            current: loaded.bible,
            query: String::new(),
            results: vec![],
            selected: vec![],
            last_clicked: None,
            info: String::new(),
            bookmarks: loaded.bookmarks,
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
        let design = Design { id: make_id(), name: t("designer.untitled"), doc: CanvasDoc::default() };
        let mut app = Self {
            lib: loaded.lib,
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
            frozen: None,
            service_area: None,
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
            loading: None,
            output_monitor: None,
            screen_names: Vec::new(),
            monitors: Vec::new(),
            main_monitor: None,
            pending_main_position: None,
            root_window: None,
            placement_tries: 0,
            output_was_open: false,
            service_cols: Default::default(),
            service_row_ids: Vec::new(),
            rename_text: String::new(),
            cli: false,
            remote_rx,
            remote_tx,
            mcp: None,
            mcp_error: None,
            mcp_forced: false,
            // made at the start: it puts back desktop settings a crashed run left changed
            desktop_guard: Some(crate::desktop_guard::Guard::for_this_computer()),
            transparent_windows: settings.output_transparent,
            remote_ctx: Some(ctx.clone()),
            startup_commands: Vec::new(),
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
                clock: settings.clock,
                textures: None,
                keys_enabled: true,
                hide_cursor: false,
                peek_marked: None,
                shortcuts: settings.shortcuts.clone(),
                mark_keys: MarkKeys::default(),
                actions: Vec::new(),
                jump: None,
                fullscreen: None,
                close: false,
                target: None,
                monitors: Vec::new(),
                placing: None,
                on_screen: None,
                escape: false,
                guard_fullscreen: false,
                lost_since: None,
                lost_fullscreen: false,
                transparent: settings.output_transparent,
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

    /// What the outputs show: black while blanked, the frozen scene while frozen, else live.
    pub fn output_scene(&self) -> Scene {
        if self.blank {
            Scene::Blank
        } else if let Some((scene, _)) = &self.frozen {
            scene.clone()
        } else {
            self.live.clone()
        }
    }

    pub fn broadcast(&self) {
        if let Some(server) = &self.server {
            server.broadcast(&self.output_scene());
        }
    }

    /// Freeze on: the outputs keep the current picture while slides are changed. Off: they
    /// show the live slide again (a live video from where it is now).
    pub fn toggle_freeze(&mut self) {
        if self.frozen.take().is_none() {
            self.frozen = Some((self.live.clone(), self.video_since));
            self.notify(t("output.frozen_on"));
            return;
        }
        self.notify(t("output.frozen_off"));
        if matches!(self.live, Scene::Video(_)) {
            // new playback state from the current position, sent to every output
            self.video_update(|_| {});
        } else {
            self.broadcast();
        }
    }

    /// Live mode on / off. Turned on, the selected slide goes live right away (so what is
    /// selected is what is shown, as live mode promises).
    pub fn set_live_mode(&mut self, on: bool) {
        self.settings.live_follows_selection = on;
        self.save_settings();
        let selected = self.sel_item.map(|i| (i, self.sel_slide));
        if on && selected.is_some() && selected != self.live_pos {
            self.go_live();
        }
        self.notify(if on { t("status.live_on") } else { t("status.live_off") });
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
            // ...also the one frozen on the output
            if let Some((Scene::Video(v), since)) = &mut self.frozen {
                v.position = v.position_after(since.elapsed().as_secs_f64());
                v.paused = true;
                v.token = crate::video::next_token();
                *since = Instant::now();
            }
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
                    self.designer_add(ObjectKind::Image { media, opacity: 1.0 });
                }
            }
            files::Purpose::DesignerReplaceImage(id) => {
                if let Some(m) = self.store_image(&first)
                    && let Some(ObjectKind::Image { media, .. }) =
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
            WriteKind::BibleTxt(id) => match Bible::load(id) {
                Some(b) => storage::write_atomic(&path, b.to_txt().as_bytes(), true)
                    .map(|_| tf("export.done", &[("file", &file)]))
                    .map_err(|e| e.to_string()),
                None => Err(t("bible.none")),
            },
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
            files::Spec::new(files::Mode::PickFiles, t("import.bible"))
                .filter(t("file.bible_filter"), &["xml", "osis", "zef", "sqlite", "txt"]),
            files::Purpose::ImportBible,
        );
    }

    /// Exports the current bible as plain text (it can be imported again).
    pub fn export_bible_txt(&mut self) {
        let Some(b) = &self.bible.current else {
            return self.notify(t("bible.no_bible"));
        };
        let name = format!("{}.txt", storage::safe_file_name(&b.meta.name));
        self.export_to(WriteKind::BibleTxt(b.meta.id.clone()), &name, &t("file.text_filter"), "txt");
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
                Action::ToggleLive => self.set_live_mode(!self.settings.live_follows_selection),
                Action::Blank => self.toggle_blank(),
                Action::Freeze => self.toggle_freeze(),
                Action::RenameItem => self.open_rename_dialog(),
                Action::VideoPlayPause => {
                    if let Some(target) = self.play_pause_target() {
                        self.video_play_pause(target);
                    }
                }
                Action::VideoForward | Action::VideoBack => {
                    // a video that is not live has no position: the key does nothing
                    if self.video_key_target() == Some(video_ctl::VideoTarget::Live) {
                        let step = if a == Action::VideoForward { video_ctl::VIDEO_SEEK_STEP } else { -video_ctl::VIDEO_SEEK_STEP };
                        self.video_seek(step);
                    }
                }
                Action::ToggleOutput => {
                    if self.output_open {
                        self.request_output_change(OutputChange::Close, ctx);
                    } else {
                        self.output_open = true;
                    }
                }
                Action::Alert => self.open_alert_dialog(),
                Action::OutputFullscreen => {
                    if self.output_open && self.output_fullscreen {
                        self.request_output_change(OutputChange::Windowed, ctx);
                    } else {
                        self.output_open = true;
                        self.output_fullscreen = !self.output_fullscreen;
                        self.send_output_fullscreen(ctx);
                    }
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
            Action::RenameItem => self.tab == Tab::Service && matches!(self.sel_item, Some(ItemRef::Service(_))),
            Action::VideoPlayPause => self.play_pause_target().is_some(),
            Action::VideoForward | Action::VideoBack => self.video_key_target().is_some(),
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
        // in the Service tab a focused button, list row… keeps only the keys it uses itself
        let focus = shortcuts::KeyFocus::of(ctx, self.tab == Tab::Service);
        let actions = shortcuts.triggered(ctx, focus, |a| self.action_enabled(a));
        if !actions.is_empty() {
            // a shortcut (Ctrl+1, F5, …) closes an open menu, as in other programs
            egui::Popup::close_all(ctx);
        }
        self.handle_actions(actions, ctx);
        if self.presenting()
            && let Some((code, n)) = self.mark_keys().consume(ctx, focus)
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
        let opening = self.output_open && !self.output_was_open;
        self.output_was_open = self.output_open;
        if !self.output_open {
            return;
        }
        if opening {
            // a projector may have been plugged in or out since the screens were looked at;
            // also a windowed output (F5) opens on the output's screen
            self.refresh_output_monitor();
        }
        let scene = self.output_scene();
        let mut save = false;
        let mut lost = false;
        let escape;
        let (actions, jump, fullscreen, close) = {
            let mut o = self.output.lock().unwrap_or_else(|e| e.into_inner());
            if opening {
                // a new window: mark it for Peek on its first frame
                o.peek_marked = None;
                o.placing = Some(Placing::new(self.output_fullscreen));
                o.on_screen = None;
            }
            o.target = self.output_monitor.and_then(|i| self.monitors.get(i).cloned().map(|m| (i, m)));
            o.monitors.clone_from(&self.monitors);
            if o.scene != scene {
                o.scene = scene;
                ctx.request_repaint_of(output_id());
            }
            let alert = self.alert.as_ref().map(|(a, _)| a);
            if o.alert.as_ref().map(|(a, _)| a) != alert {
                o.alert = alert.map(|a| (a.clone(), Instant::now()));
                ctx.request_repaint_of(output_id());
            }
            if o.clock != self.settings.clock {
                o.clock = self.settings.clock;
                ctx.request_repaint_of(output_id());
            }
            o.keys_enabled = self.capture.is_none() && self.dialog.is_none() && self.file_dialog.is_none();
            o.guard_fullscreen = self.output_on_other_screen();
            // a window (not full screen) shows the pointer, so it can be moved and resized
            o.hide_cursor = self.settings.output_always_on_top && self.output_fullscreen;
            o.shortcuts = self.settings.shortcuts.clone();
            o.mark_keys = self.mark_keys();
            // remember the screen the output was shown on (it opens there next time)
            if let Some(i) = o.on_screen
                && Some(i) != self.main_monitor
                && let Some(m) = self.monitors.get(i)
                && self.settings.output_screen.as_ref() != Some(&crate::screens::ScreenId::of(m))
            {
                self.settings.output_screen = Some(crate::screens::ScreenId::of(m));
                // saved below, once the output's state is let go
                save = true;
            }
            if std::mem::take(&mut o.lost_fullscreen) {
                lost = true;
            }
            escape = std::mem::take(&mut o.escape);
            (std::mem::take(&mut o.actions), o.jump.take(), o.fullscreen.take(), std::mem::take(&mut o.close))
        };
        if lost {
            self.warn_desktop_took_output(ctx);
        }
        if save {
            self.save_settings();
        }
        let shared = self.output.clone();
        let title = t("output.title");
        let builder = self.output_viewport(&title);
        // `--cli`: the program's own window is the output, drawn by `frame`
        if !self.cli {
            ctx.show_viewport_deferred(output_id(), builder, move |ui, _class| {
                let mut guard = shared.lock().unwrap_or_else(|e| e.into_inner());
                output_frame(ui, &mut guard, &title, false);
            });
        }
        self.handle_actions(actions, ctx);
        if let Some((code, n)) = jump {
            self.jump_to_mark(&code, n);
        }
        match fullscreen {
            // double click on a window: full screen
            Some(true) if !self.output_fullscreen => {
                self.output_fullscreen = true;
                self.send_output_fullscreen(ctx);
            }
            // double click or Esc on the full screen output: out of it (asked first when
            // it is on another screen)
            Some(_) if self.output_fullscreen => self.request_output_change(OutputChange::Windowed, ctx),
            _ => {}
        }
        if escape {
            match self.dialog {
                // Esc in the output window answers the question about it: Cancel (the output
                // window often keeps the keyboard: Wayland does not let the main window take it)
                Some(Dialog::LeaveOutput(_)) => {
                    self.dialog = None;
                    ctx.request_repaint();
                }
                // Esc on the full screen output: out of it (asked first on another screen)
                None if self.output_fullscreen => self.request_output_change(OutputChange::Windowed, ctx),
                _ => {}
            }
        }
        if close {
            // a child window stays open as long as it is shown: not closing = cancelled
            self.request_output_change(OutputChange::CloseButton, ctx);
        }
    }

    /// The desktop took the output out of full screen (it was put back): tells the operator.
    fn warn_desktop_took_output(&mut self, ctx: &egui::Context) {
        self.notify(t("output.fullscreen_restored"));
        ctx.request_repaint_of(ViewportId::ROOT);
    }

    /// The output window: created as an ordinary window, then put on its screen (see
    /// [`Placing`]); transparent when the program's windows are (eframe's wgpu renderer does
    /// not make child windows like the first one by itself).
    pub fn output_viewport(&self, title: &str) -> egui::ViewportBuilder {
        egui::ViewportBuilder::default()
            .with_title(title)
            .with_inner_size([960.0, 540.0])
            .with_transparent(self.transparent_windows)
            .with_window_level(if self.settings.output_always_on_top { egui::WindowLevel::AlwaysOnTop } else { egui::WindowLevel::Normal })
    }

    /// The output window is full screen on another screen than the program's, where the
    /// operator does not see it.
    pub fn output_on_other_screen(&self) -> bool {
        self.output_open && self.output_fullscreen && self.output_monitor.is_some() && self.output_monitor != self.main_monitor
    }

    /// Leaves full screen / closes the output window; while it is full screen on another
    /// screen, a dialog in the main window asks first (a stray key on the projector window
    /// must not show the desktop to everybody).
    pub fn request_output_change(&mut self, change: OutputChange, ctx: &egui::Context) {
        if !self.output_on_other_screen() {
            return self.apply_output_change(change, ctx);
        }
        if self.dialog.is_none() {
            self.dialog = Some(Dialog::LeaveOutput(change));
            // the question is in the main window: bring it to the front
            ctx.send_viewport_cmd_to(ViewportId::ROOT, ViewportCommand::Focus);
            ctx.request_repaint_of(ViewportId::ROOT);
        }
    }

    pub fn apply_output_change(&mut self, change: OutputChange, ctx: &egui::Context) {
        match change {
            OutputChange::Windowed => {
                self.output_fullscreen = false;
                self.send_output_fullscreen(ctx);
            }
            OutputChange::Close => self.output_open = false,
            OutputChange::CloseButton => {
                self.output_open = false;
                self.output_fullscreen = false;
            }
        }
    }

    /// Applies `output_fullscreen` to the output window: full screen on the output's screen
    /// when there is one, else on the screen the window is on (done step by step by the
    /// output window itself, see [`Placing`]).
    fn send_output_fullscreen(&mut self, ctx: &egui::Context) {
        if self.output_fullscreen {
            // a projector may have been plugged in or out since the start
            self.refresh_output_monitor();
        }
        let mut o = self.output.lock().unwrap_or_else(|e| e.into_inner());
        o.target = self.output_monitor.and_then(|i| self.monitors.get(i).cloned().map(|m| (i, m)));
        o.monitors.clone_from(&self.monitors);
        o.placing = Some(Placing { next_at: Instant::now(), ..Placing::new(self.output_fullscreen) });
        drop(o);
        ctx.request_repaint_of(output_id());
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
                Dialog::LeaveOutput(change) => {
                    let windowed = *change == OutputChange::Windowed;
                    ui.heading(t(if windowed { "dialog.output_leave" } else { "dialog.output_close" }));
                    ui.label(t("dialog.output_other_screen"));
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        let (change, c) = (*change, ctx.clone());
                        let yes = t(if windowed { "dialog.output_leave_yes" } else { "dialog.output_close_yes" });
                        btn(ui, yes, Some(Box::new(move |app| app.apply_output_change(change, &c))));
                        btn(ui, t("cancel"), None);
                    });
                }
                Dialog::RenameItem(i) => {
                    let i = *i;
                    ui.heading(t("service.rename"));
                    let own = self.service.items.get(i).map(|it| it.own_title()).unwrap_or_default();
                    ui.weak(tf("service.rename_hint", &[("title", &own)]));
                    let r = ui.add(egui::TextEdit::singleline(&mut self.rename_text).desired_width(400.0));
                    if !r.has_focus() && !r.lost_focus() {
                        r.request_focus();
                    }
                    let enter = r.lost_focus() && ui.input(|inp| inp.key_pressed(Key::Enter));
                    ui.add_space(8.0);
                    let name = self.rename_text.clone();
                    let rename: DialogAction = Box::new(move |app| {
                        app.rename_item(i, &name);
                    });
                    if enter {
                        close = true;
                        next = Some(rename);
                    } else {
                        ui.horizontal(|ui| {
                            btn(ui, t("ok"), Some(rename));
                            btn(ui, t("cancel"), None);
                        });
                    }
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
                    if ui
                        .add_enabled(self.bible.current.is_some(), egui::Button::new(t("export.bible")))
                        .on_hover_text(t("export.bible_hint"))
                        .clicked()
                    {
                        self.export_bible_txt();
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
                    self.handle_actions(vec![Action::ToggleOutput], &ui.ctx().clone());
                }
                if item(ui, t("output.fullscreen"), Some(Action::OutputFullscreen)) {
                    self.handle_actions(vec![Action::OutputFullscreen], &ui.ctx().clone());
                }
                if item(ui, t("output.blank"), Some(Action::Blank)) {
                    self.toggle_blank();
                }
                let freeze = if self.frozen.is_some() { format!("✔ {}", t("output.freeze")) } else { t("output.freeze") };
                if item(ui, freeze, Some(Action::Freeze)) {
                    self.toggle_freeze();
                }
                ui.separator();
                if ui.button(t("output.start_client")).on_hover_text(t("output.start_client_hint")).clicked() {
                    self.start_local_client();
                }
            });
            // Alt+1 / Alt+2 open the menus (see keynav: a number alone picks the n-th item)
            self.alt_nav.menu_button_ids = vec![file_menu.response.id, output_menu.response.id];
            for (n, m) in [&file_menu, &output_menu].into_iter().enumerate() {
                m.response.clone().on_hover_text(tf("menu.shortcut_hint", &[("key", &format!("Alt+{}", n + 1))]));
            }
            self.alt_nav.menu_ids = [
                Some(egui::Popup::default_response_id(&file_menu.response)),
                Some(egui::Popup::default_response_id(&output_menu.response)),
            ];
            // Alt + letters asked for a menu (see keynav)
            if let Some(m) = open_menu {
                let r = if m == 0 { &file_menu.response } else { &output_menu.response };
                egui::Popup::open_id(ui.ctx(), egui::Popup::default_response_id(r));
                // Tab (or ↓) then goes on to the menu's first item
                r.request_focus();
            }
            ui.separator();
            self.alt_nav.tab_ids.clear();
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
                let r = ui.selectable_label(self.tab == tab, label).on_hover_text(sc.describe(action));
                self.alt_nav.tab_ids.push((tab, r.id));
                if r.clicked() {
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
            if self.frozen.is_some() {
                ui.colored_label(LIVE_COLOR, t("status.frozen")).on_hover_text(t("output.freeze_hint"));
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

/// One frame of the output window (or of the program's window in `--cli` mode, `root`):
/// the live picture, the alert and clock, its keys, and putting it on its screen.
fn output_frame(ui: &mut egui::Ui, o: &mut OutputShared, title: &str, root: bool) {
    let vctx = ui.ctx().clone();
    vctx.input_mut(|i| shortcuts::digits_by_position(&mut i.events));
    let rect = ui.max_rect();
    let painter = ui.painter_at(rect);
    if render::paints_black_behind(o.transparent, &o.scene) {
        painter.rect_filled(rect, 0.0, egui::Color32::BLACK);
    }
    let textures = o.textures.get_or_insert_with(|| TextureCache::new(storage::paths().media()));
    let slide = render::fit_16_9(rect);
    render::draw_scene_live(&painter, slide, &o.scene, textures);
    if let Some((alert, since)) = &o.alert
        && render::draw_alert(&painter, slide, alert, since.elapsed().as_secs_f32())
    {
        // ~60 frames per second while it runs
        vctx.request_repaint_after(std::time::Duration::from_millis(16));
    }
    if o.clock.show {
        let secs = crate::clock::now_secs();
        crate::clock::draw(&painter, rect, slide.width(), &o.clock, secs);
        vctx.request_repaint_after(crate::clock::repaint_in(&o.clock, secs));
    }
    let mut wake_main = false;
    // double click toggles fullscreen, Esc leaves it (or cancels the question about it)
    let resp = ui.interact(rect, egui::Id::unique("output_area"), egui::Sense::click());
    if resp.double_clicked() {
        o.fullscreen = Some(true);
        wake_main = true;
    }
    if vctx.input(|i| i.key_pressed(Key::Escape)) {
        o.escape = true;
        wake_main = true;
    }
    if !root && vctx.input(|i| i.viewport().close_requested()) {
        o.close = true;
        wake_main = true;
    }
    if o.keys_enabled && vctx.input(|i| i.focused) {
        let focus = shortcuts::KeyFocus::of(&vctx, false);
        let actions = o.shortcuts.triggered(&vctx, focus, |a| {
            a.is_navigation() || matches!(a, Action::ToggleOutput | Action::OutputFullscreen | Action::VideoPlayPause)
        });
        if let Some(j) = o.mark_keys.consume(&vctx, shortcuts::KeyFocus::None) {
            o.jump = Some(j);
            wake_main = true;
        }
        if !actions.is_empty() {
            o.actions.extend(actions);
            wake_main = true;
        }
    }
    // Windows Peek (the taskbar previews) must not hide the projected picture
    if o.peek_marked.is_none_or(|t| t.elapsed() > std::time::Duration::from_secs(2)) {
        o.peek_marked = crate::peek::keep_shown(title).then(Instant::now);
        if o.peek_marked.is_none() {
            vctx.request_repaint_after(std::time::Duration::from_millis(200));
        }
    }
    keep_output_fullscreen(o, &vctx);
    place_output_window(o, &vctx);
    if o.hide_cursor {
        // the pointer would show on the projector when moved over to it
        vctx.set_cursor_icon(egui::CursorIcon::None);
    }
    if wake_main && !root {
        // the main window does the work (and sends the new slide back here)
        vctx.request_repaint_of(ViewportId::ROOT);
    } else if wake_main {
        vctx.request_repaint();
    }
}

/// How long the output window has to be out of full screen (or minimized) without the
/// program asking for it before it is put back.
const LOST_FULLSCREEN_AFTER: std::time::Duration = std::time::Duration::from_millis(400);

/// The output should be full screen on another screen, but the desktop took it out of full
/// screen or minimized it (a key of the window manager: Super+↓ on GNOME, a Hyprland bind,
/// Win+D on Windows, …): it is put back, and the main window shows a warning.
fn keep_output_fullscreen(o: &mut OutputShared, vctx: &egui::Context) {
    if !o.guard_fullscreen || o.placing.is_some() {
        o.lost_since = None;
        return;
    }
    let (fullscreen, minimized) = vctx.input(|i| (i.viewport().fullscreen, i.viewport().minimized));
    let lost = fullscreen == Some(false) || minimized == Some(true);
    if !lost {
        o.lost_since = None;
        return;
    }
    let since = *o.lost_since.get_or_insert_with(Instant::now);
    if since.elapsed() < LOST_FULLSCREEN_AFTER {
        vctx.request_repaint_after(LOST_FULLSCREEN_AFTER);
        return;
    }
    crate::log_info(&format!("output window left full screen by itself (fullscreen {fullscreen:?}, minimized {minimized:?}): put back"));
    o.lost_since = None;
    if minimized == Some(true) {
        vctx.send_viewport_cmd(ViewportCommand::Minimized(false));
    }
    o.placing = Some(Placing { next_at: Instant::now(), ..Placing::new(true) });
    o.lost_fullscreen = true;
    vctx.request_repaint_of(ViewportId::ROOT);
}

/// Runs in the output window (each of its frames): the next step of putting it on its
/// screen, once the last step had time to take effect. Every step is written to start.log.
fn place_output_window(o: &mut OutputShared, vctx: &egui::Context) {
    use crate::screens::{self, Seen, Step};
    let ppp = vctx.pixels_per_point();
    let phys = |v: f32| (v * ppp).round();
    let seen = vctx.input(|i| {
        let v = i.viewport();
        Seen {
            outer: v.outer_rect.map(|r| (phys(r.min.x) as i32, phys(r.min.y) as i32, phys(r.width()) as u32, phys(r.height()) as u32)),
            inner_size: v.inner_rect.map(|r| (phys(r.width()) as u32, phys(r.height()) as u32)),
            fullscreen: v.fullscreen,
        }
    });
    // which screen it is on (to remember it)
    if let Some((x, y, w, h)) = seen.outer {
        o.on_screen = o.monitors.iter().position(|m| screens::contains(m, (x + w as i32 / 2, y + h as i32 / 2)));
    }
    let Some(p) = &mut o.placing else { return };
    let now = Instant::now();
    if now < p.next_at {
        vctx.request_repaint_after(p.next_at - now);
        return;
    }
    let target = o.target.as_ref();
    let step = match (target, p.fullscreen) {
        (Some((_, m)), true) => screens::fullscreen_step(m, &seen, p.done),
        // one screen: full screen where it is
        (None, true) => (seen.fullscreen != Some(true) && p.done < 3).then_some(Step::EnterFullscreen),
        (m, false) => screens::windowed_step(m.map(|(_, m)| m), &seen, p.done),
    };
    let what = format!("output window: {seen:?}, target {:?}", target.map(|(i, m)| (i, &m.name, m.pos, m.size)));
    let Some(step) = step else {
        crate::log_info(&format!("{what}: placed after {} steps", p.done));
        o.placing = None;
        return;
    };
    if p.done >= screens::MAX_STEPS {
        crate::log_info(&format!("{what}: not placed after {} steps, giving up", p.done));
        o.placing = None;
        return;
    }
    crate::log_info(&format!("{what}: step {} {step:?}", p.done + 1));
    let cmd = match step {
        Step::SetMonitor => match target {
            Some((i, _)) => ViewportCommand::SetMonitor(*i),
            None => ViewportCommand::Fullscreen(true),
        },
        Step::LeaveFullscreen => ViewportCommand::Fullscreen(false),
        // egui takes points of this window (its own scale) and makes physical pixels of them
        Step::MoveTo((x, y)) => ViewportCommand::OuterPosition(egui::pos2(x as f32 / ppp, y as f32 / ppp)),
        Step::EnterFullscreen => ViewportCommand::Fullscreen(true),
    };
    vctx.send_viewport_cmd(cmd);
    p.done += 1;
    // a move to a screen with another scale, or a full screen switch, takes a moment
    p.next_at = now + std::time::Duration::from_millis(250);
    vctx.request_repaint_after(std::time::Duration::from_millis(260));
}

/// Keys that jump to marked song parts (and number keys to verses).
#[derive(Default)]
struct MarkKeys {
    marks: Vec<(String, KeyBinding)>,
    numbers: bool,
}

impl MarkKeys {
    /// The mark key pressed this frame; keys the focused widget gets (`focus`) are left to it.
    fn consume(&self, ctx: &egui::Context, focus: shortcuts::KeyFocus) -> Option<(String, Option<u32>)> {
        if focus == shortcuts::KeyFocus::Text {
            return None;
        }
        let mut jump = None;
        ctx.input_mut(|i| {
            for (code, key) in &self.marks {
                if !focus.takes(key) && key.consume(i) {
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

impl App {
    /// Sends commands to this program (the command line, the MCP server).
    pub fn remote_sender(&self, ctx: &egui::Context) -> crate::remote::Sender {
        crate::remote::Sender::new(self.remote_tx.clone(), ctx.clone())
    }

    /// Runs the commands that came from the command line / the MCP server.
    fn poll_remote(&mut self, ctx: &egui::Context) {
        while let Ok(req) = self.remote_rx.try_recv() {
            if std::time::Instant::now() > req.deadline {
                // its sender stopped waiting: it is not done later behind its back
                continue;
            }
            let out = self.run_command(&req.name, &req.args, ctx);
            let _ = req.reply.send(out);
        }
    }

    /// Starts / stops the MCP server as the settings say.
    fn sync_mcp(&mut self) {
        let want = (self.settings.mcp_enabled || self.mcp_forced).then_some(self.settings.mcp_port);
        if self.mcp.as_ref().map(|s| s.port) == want {
            return;
        }
        if want.is_none() {
            self.mcp = None;
            self.mcp_error = None;
            return;
        }
        // a port that failed is not tried again on every frame (only after a change)
        if self.mcp_error.as_ref().is_some_and(|e| e.starts_with(&format!("127.0.0.1:{}:", self.settings.mcp_port))) {
            return;
        }
        self.mcp = None;
        if self.settings.mcp_port < 1024 {
            self.mcp_error = Some(format!("127.0.0.1:{}: the port must be 1024–65535", self.settings.mcp_port));
            return;
        }
        let ctx = self.remote_ctx.clone();
        match ctx.map(|c| crate::remote::mcp::Server::start(self.settings.mcp_port, self.remote_sender(&c))) {
            Some(Ok(s)) => {
                self.mcp = Some(s);
                self.mcp_error = None;
            }
            Some(Err(e)) => {
                crate::log_error(&format!("MCP server: {e}"));
                self.mcp_error = Some(e);
            }
            None => {}
        }
    }

    /// The desktop's keys are off while the output is full screen on another screen.
    fn sync_desktop_guard(&mut self) {
        let want = self.settings.guard_desktop_keys && self.output_on_other_screen();
        if let Some(g) = &mut self.desktop_guard {
            g.set(want);
        }
    }

    #[cfg(test)]
    pub fn desktop_guard_engaged(&self) -> bool {
        self.desktop_guard.as_ref().is_some_and(|g| g.engaged())
    }

    #[cfg(test)]
    pub fn mcp_port(&self) -> Option<u16> {
        self.mcp.as_ref().map(|s| s.port)
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        if self.root_window.is_none() {
            self.root_window = frame.winit_window().cloned();
        }
        self.frame(ui);
    }

    fn clear_color(&self, visuals: &egui::Visuals) -> [f32; 4] {
        if self.transparent_windows {
            // the output window lets what is behind it show (the main window paints its own
            // background, see `frame`)
            [0.0; 4]
        } else {
            visuals.panel_fill.to_normalized_gamma_f32()
        }
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        if let Some(g) = &mut self.desktop_guard {
            g.set(false);
        }
    }

    fn raw_input_hook(&mut self, ctx: &egui::Context, raw_input: &mut egui::RawInput) {
        self.menu_arrows_as_tab(ctx, raw_input);
        self.tab_from_current_tab(ctx, raw_input);
    }
}

impl App {
    /// One frame of the whole UI.
    pub fn frame(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        self.limit_frame_rate();
        if !crate::start_guard::frame() {
            // the start is confirmed a few frames in (see start_guard)
            ctx.request_repaint();
        }
        if self.transparent_windows && !self.cli {
            // the window is transparent (for the output's sake): the program is not
            let fill = ctx.global_style().visuals.panel_fill;
            ctx.layer_painter(egui::LayerId::background()).rect_filled(ui.max_rect(), 0.0, fill);
        }
        self.retry_placement(&ctx);
        if let Some(pos) = self.pending_main_position.take() {
            ctx.send_viewport_cmd(ViewportCommand::OuterPosition(pos));
        }
        if self.poll_loading() {
            // the projector goes black right away; the program shows after loading
            self.show_output(&ctx);
            if self.cli {
                // `--cli`: this window is the projector, which stays black meanwhile
                ui.painter().rect_filled(ui.max_rect(), 0.0, egui::Color32::BLACK);
            } else if let Some(loading) = &mut self.loading {
                loading.show(ui);
            }
            return;
        }

        ctx.input_mut(|i| shortcuts::digits_by_position(&mut i.events));

        if !self.startup_commands.is_empty() {
            // the command line's options, now that the library is there
            for (name, args) in std::mem::take(&mut self.startup_commands) {
                match self.run_command(&name, &args, &ctx) {
                    Ok(out) if self.cli => println!("{name}: {}", out.lines().next().unwrap_or_default()),
                    Ok(_) => {}
                    Err(e) => {
                        eprintln!("oh-presenter-rs: {name}: {e}");
                        if !self.cli {
                            self.error(format!("{name}: {e}"));
                        }
                    }
                }
            }
        }
        if self.cli {
            // no program window to ask in: closing the output ends the program, and messages
            // go to the command line
            if ctx.input(|i| i.viewport().close_requested()) {
                self.quit_confirmed = true;
            }
            if let Some(d) = self.dialog.take() {
                match d {
                    Dialog::Message(m) => eprintln!("{m}"),
                    other => crate::log_info(&format!("--cli: dialog {other:?} dropped")),
                }
            }
        }
        // closing the main window always asks first (and offers to save unsaved work)
        if ctx.input(|i| i.viewport().close_requested()) && !self.quit_confirmed {
            ctx.send_viewport_cmd(ViewportCommand::CancelClose);
            if self.dialog.is_none() {
                self.request(Then::Quit);
            }
        }
        if self.quit_confirmed {
            // the desktop's keys come back before the program ends
            if let Some(g) = &mut self.desktop_guard {
                g.set(false);
            }
            ctx.send_viewport_cmd(ViewportCommand::Close);
        }
        self.poll_remote(&ctx);
        self.sync_mcp();

        self.poll_file_dialog(&ctx);
        self.poll_legacy_scan(&ctx);
        self.poll_indexing(&ctx);
        self.poll_alert(&ctx);
        if let Some(server) = &self.server {
            server.clock(&self.settings.clock);
        }
        self.poll_video_import(&ctx);
        self.sync_video_audio();
        if self.tab != Tab::Service {
            // the video keys follow a click in the Service tab, made again after coming back
            self.service_area = None;
        }
        if self.tab != Tab::Settings {
            // a key capture started in Settings must not keep swallowing keys elsewhere
            self.capture = None;
        }
        self.show_output(&ctx);
        self.sync_desktop_guard();
        if self.cli {
            // `--cli`: this window is the output; nothing else is shown
            let shared = self.output.clone();
            let mut o = shared.lock().unwrap_or_else(|e| e.into_inner());
            egui::CentralPanel::no_frame().show(ui, |ui| output_frame(ui, &mut o, &t("output.title"), true));
            return;
        }
        if ctx.input(|i| i.focused) {
            self.video_keys(&ctx);
            self.service_column_keys(&ctx);
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
impl App {
    /// Panel ids of the Service tab's left and right column.
    pub fn service_panel_ids(&self) -> Option<(egui::Id, egui::Id)> {
        self.service_cols.panels()
    }
    /// The id of the live video's preview (it takes Space / ← →).
    pub fn video_preview_id_for_test() -> egui::Id {
        Self::video_preview_id()
    }

    /// As if the output window reported a double click (`Some(true)`), Esc (`Some(false)`)
    /// or its close button; handled on the next frame.
    pub fn output_reports(&self, fullscreen: Option<bool>, close: bool) {
        let mut o = self.output.lock().unwrap_or_else(|e| e.into_inner());
        match fullscreen {
            Some(false) => o.escape = true,
            f => o.fullscreen = f,
        }
        o.close = close;
    }

    /// The id of row `i` of the service item list (last frame).
    pub fn service_row_id_for_test(&self, i: usize) -> Option<egui::Id> {
        self.service_row_ids.get(i).copied()
    }

    /// The output window is where it should be (no placing steps left).
    pub fn output_placing_done(&self) {
        self.output.lock().unwrap_or_else(|e| e.into_inner()).placing = None;
    }

    /// Whether the output window is being put (back) on its screen.
    pub fn output_placing(&self) -> bool {
        self.output.lock().unwrap_or_else(|e| e.into_inner()).placing.is_some()
    }

    /// Whether the output window hides the mouse pointer (as set on the last frame).
    pub fn output_hides_cursor(&self) -> bool {
        self.output.lock().unwrap_or_else(|e| e.into_inner()).hide_cursor
    }

    /// The screens: output full screen on `output`, the program on `main`.
    pub fn set_screens(&mut self, main: Option<usize>, output: Option<usize>) {
        self.main_monitor = main;
        self.output_monitor = output;
    }
}

#[cfg(test)]
pub fn settings_tab_capture(a: Action) -> settings_tab::Capture {
    settings_tab::Capture::Action(a)
}

#[cfg(test)]
pub fn startup_load(progress: &mut dyn FnMut(f32)) -> startup::Loaded {
    startup::load_for_test(progress)
}

#[cfg(test)]
pub use startup::fraction as startup_fraction;

#[cfg(test)]
pub use service_tab::ServiceArea;
