//! The start of the program: the library is loaded on a background thread while the
//! window shows the logo with a progress bar under it (so it is clear the program is
//! starting), and the windows are put on the screens: the program on the primary one,
//! the output window full screen on a second one.

use super::{App, Dialog};
use crate::bible::{self, Bible, BibleMeta, Bookmarks};
use crate::i18n::{t, tf};
use crate::screens;
use crate::storage::{self, Library};
use eframe::egui;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, mpsc};

/// What is read from disk at the start.
pub struct Loaded {
    pub lib: Library,
    pub bibles: Vec<BibleMeta>,
    pub bible: Option<Bible>,
    pub bookmarks: Bookmarks,
}

impl Loaded {
    /// Nothing loaded (yet).
    pub fn empty() -> Self {
        Self { lib: Library { songs: Vec::new(), designs: Vec::new() }, bibles: Vec::new(), bible: None, bookmarks: Bookmarks::default() }
    }

    /// Reads the library, the list of bibles, the chosen bible (`bible`, else the first
    /// one) and the bookmarks. `progress` gets the done part (0.0–1.0) by bytes read.
    pub fn load(bible: Option<String>, progress: &mut dyn FnMut(f32)) -> Self {
        let lib_files = Library::files();
        let bibles = bible::list_bibles();
        let bible_id = bible.or_else(|| bibles.first().map(|b| b.id.clone()));
        let bible_size =
            bible_id.as_ref().map(|id| storage::files_size(&[storage::paths().bibles().join(format!("{id}.json"))])).unwrap_or(0);
        let total = storage::files_size(&lib_files) + bible_size;
        let mut done = 0u64;
        progress(0.0);
        let lib = Library::load_with_progress(&mut |n| {
            done += n;
            progress(fraction(done, total));
        });
        let bible = bible_id.and_then(|id| Bible::load(&id));
        progress(fraction(done + bible_size, total));
        let bookmarks = Bookmarks::load();
        progress(1.0);
        Self { lib, bibles, bible, bookmarks }
    }
}

/// `done` of `total` as 0.0–1.0 (nothing to do = done).
pub fn fraction(done: u64, total: u64) -> f32 {
    if total == 0 { 1.0 } else { (done as f64 / total as f64).clamp(0.0, 1.0) as f32 }
}

/// The loading that runs while the splash screen shows.
pub struct Loading {
    rx: mpsc::Receiver<Loaded>,
    progress: Arc<AtomicU32>,
    logo: Option<egui::TextureHandle>,
}

impl Loading {
    pub fn start(ctx: &egui::Context, bible: Option<String>) -> Self {
        let (tx, rx) = mpsc::channel();
        let progress = Arc::new(AtomicU32::new(0f32.to_bits()));
        let (p, ctx) = (progress.clone(), ctx.clone());
        let bible_here = bible.clone();
        let spawned = std::thread::Builder::new().name("loading".into()).spawn(move || {
            let loaded = Loaded::load(bible, &mut |f| {
                p.store(f.to_bits(), Ordering::Relaxed);
                ctx.request_repaint();
            });
            let _ = tx.send(loaded);
            ctx.request_repaint();
        });
        if let Err(e) = spawned {
            // no thread: load right here (the splash is then simply not seen)
            crate::log_error(&format!("no loading thread: {e}"));
            let (tx, rx2) = mpsc::channel();
            let _ = tx.send(Loaded::load(bible_here, &mut |_| {}));
            return Self { rx: rx2, progress, logo: None };
        }
        Self { rx, progress, logo: None }
    }

    pub fn progress(&self) -> f32 {
        f32::from_bits(self.progress.load(Ordering::Relaxed))
    }

    /// The loaded data once it is there; `Err` when the loading thread died (a panic,
    /// already written to crash.log).
    pub fn poll(&self) -> Option<Result<Loaded, ()>> {
        match self.rx.try_recv() {
            Ok(l) => Some(Ok(l)),
            Err(mpsc::TryRecvError::Empty) => None,
            Err(mpsc::TryRecvError::Disconnected) => Some(Err(())),
        }
    }

    /// The logo with the progress bar under it, in the middle of the window.
    pub fn show(&mut self, ui: &mut egui::Ui) {
        let progress = self.progress();
        let logo = self.logo.get_or_insert_with(|| {
            let image = image::load_from_memory(include_bytes!("../../assets/icon-512.png"))
                .map(|i| {
                    let i = i.to_rgba8();
                    egui::ColorImage::from_rgba_unmultiplied([i.width() as usize, i.height() as usize], i.as_raw())
                })
                .unwrap_or_else(|_| egui::ColorImage::filled([1, 1], egui::Color32::TRANSPARENT));
            ui.ctx().load_texture("startup-logo", image, egui::TextureOptions::LINEAR)
        });
        egui::CentralPanel::default_margins().show(ui, |ui| {
            let side = (ui.available_height() * 0.35).clamp(64.0, 192.0);
            let width = (side * 1.6).max(220.0);
            let height = side + 60.0;
            let top = ((ui.available_height() - height) / 2.0).max(0.0);
            ui.add_space(top);
            ui.vertical_centered(|ui| {
                ui.add(egui::Image::new((logo.id(), egui::vec2(side, side))));
                ui.add_space(16.0);
                ui.add(egui::ProgressBar::new(progress).desired_width(width).animate(true));
                ui.add_space(6.0);
                ui.label(t("startup.loading"));
            });
        });
    }
}

impl App {
    /// Takes over what the loading thread read, once it is done; `true` while still loading.
    pub(super) fn poll_loading(&mut self) -> bool {
        let Some(loading) = &self.loading else { return false };
        let Some(result) = loading.poll() else { return true };
        self.loading = None;
        match result {
            Ok(l) => self.apply_loaded(l),
            Err(()) => self.dialog = Some(Dialog::Message(t("startup.load_failed"))),
        }
        if !self.settings.legacy_import_answered && !self.cli {
            // first run (until answered): look for files of the old oh-presenter
            self.start_legacy_scan(false);
        }
        false
    }

    pub(super) fn apply_loaded(&mut self, l: Loaded) {
        self.lib = l.lib;
        self.bible.bibles = l.bibles;
        self.bible.current = l.bible;
        self.bible.bookmarks = l.bookmarks;
    }

    /// The screens and the one `window` is on (by its monitor, else by its position).
    fn detect_screens(window: &winit::window::Window) -> (Vec<screens::Monitor>, Option<usize>) {
        let (monitors, mut current) = screens::detect(window);
        if current.is_none()
            && let Ok(p) = window.outer_position()
        {
            current = monitors.iter().position(|m| screens::contains(m, (p.x, p.y)));
        }
        (monitors, current)
    }

    /// Puts the windows on the screens (see [`screens::plan`]), called when the app is
    /// created. Wayland has no primary screen and tells the window's screen only once the
    /// window is shown: then this waits for a later frame ([`Self::retry_placement`]).
    pub(super) fn place_windows(&mut self, window: &winit::window::Window) {
        if !self.try_place_windows(window, true) {
            self.placement_tries = 60;
        }
    }

    /// On the first frames: places the windows once the main window's screen is known.
    pub(super) fn retry_placement(&mut self, ctx: &egui::Context) {
        if self.placement_tries == 0 {
            return;
        }
        let Some(window) = self.root_window.clone() else { return };
        self.placement_tries -= 1;
        // the last try places the windows even without knowing the screen
        let can_wait = self.placement_tries > 0;
        if self.try_place_windows(&window, can_wait) {
            self.placement_tries = 0;
        } else {
            ctx.request_repaint_after(std::time::Duration::from_millis(50));
        }
    }

    /// `false` (and nothing done) when `can_wait` and it is not known which screen is the
    /// program's: no primary screen and the main window's screen unknown.
    fn try_place_windows(&mut self, window: &winit::window::Window, can_wait: bool) -> bool {
        let (monitors, current) = Self::detect_screens(window);
        if can_wait && screens::must_wait(&monitors, current) {
            return false;
        }
        let plan = screens::plan(&monitors, current);
        crate::log_info(&format!(
            "screens: {} (main window on {:?}) -> program on {:?}, output on {:?}",
            monitors
                .iter()
                .map(|m| format!("{} {:?} {:?}{}", m.name, m.pos, m.size, if m.primary { " primary" } else { "" }))
                .collect::<Vec<_>>()
                .join(", "),
            current,
            plan.main,
            plan.output
        ));
        // move the program to the primary screen when the system opened it elsewhere
        // (not possible on Wayland, which also has no primary screen)
        if let (Some(main), Some(cur)) = (plan.main, current)
            && main != cur
            && !self.cli
        {
            let size = window.outer_size();
            let (x, y) = screens::centered_on(&monitors[main], (size.width, size.height));
            let scale = window.scale_factor() as f32;
            self.pending_main_position = Some(egui::pos2(x as f32 / scale, y as f32 / scale));
        }
        // the screen the output was last shown on, if it is still there (else the plan's)
        self.output_monitor = screens::output_screen(&monitors, plan.main, self.settings.output_screen.as_ref()).or(plan.output);
        self.main_monitor = plan.main;
        self.monitors.clone_from(&monitors);
        self.screen_names = monitors.iter().map(|m| if m.name.is_empty() { "?".to_string() } else { m.name.clone() }).collect();
        if self.cli {
            // `--cli`: this window is the output; there is no program window, so the output
            // may also go on the screen it opened on (`main` is only the screen it starts on)
            self.output_open = true;
        } else if self.settings.output_on_second_screen && self.output_monitor.is_some() {
            self.output_open = true;
            self.output_fullscreen = true;
        }
        true
    }

    /// Looks at the screens again (before the output window goes full screen): the
    /// output screen is the first one that is not the main window's.
    pub(super) fn refresh_output_monitor(&mut self) {
        let Some(window) = self.root_window.clone() else { return };
        let (monitors, current) = Self::detect_screens(&window);
        if monitors.is_empty() {
            return;
        }
        // now the program's screen is the one its window is on (it may have been moved);
        // the output goes on another one: where it was last shown, else the first other one
        let main = current.or(screens::plan(&monitors, current).main);
        let output = screens::output_screen(&monitors, main, self.settings.output_screen.as_ref());
        self.main_monitor = main;
        self.monitors.clone_from(&monitors);
        if output != self.output_monitor {
            crate::log_info(&format!("screens changed: output on {output:?} (was {:?})", self.output_monitor));
        }
        self.output_monitor = output;
        self.screen_names = monitors.iter().map(|m| if m.name.is_empty() { "?".to_string() } else { m.name.clone() }).collect();
    }

    /// The screens found at the start, for Settings.
    pub(super) fn screens_text(&self) -> String {
        let list = if self.screen_names.is_empty() { "?".to_string() } else { self.screen_names.join(", ") };
        let output = self.output_monitor.and_then(|i| self.screen_names.get(i)).cloned().unwrap_or_else(|| "–".into());
        tf("settings.screens_found", &[("list", &list), ("output", &output)])
    }
}

#[cfg(test)]
pub fn load_for_test(progress: &mut dyn FnMut(f32)) -> Loaded {
    Loaded::load(None, progress)
}
