//! Importing the files of the old oh-presenter (Electron): offered on the first run
//! when such files are found, and any time from File → Import.

use super::{App, Dialog, FollowFocus, files};
use crate::formats::{self, LegacyScan};
use crate::i18n::{t, tf};
use crate::model::*;
use crate::storage;
use eframe::egui;
use std::path::PathBuf;
use std::sync::mpsc::{Receiver, channel};

pub struct LegacyState {
    /// The search running in the background.
    rx: Option<Receiver<LegacyScan>>,
    /// Asked for from the menu: show the dialog even when nothing was found.
    manual: bool,
    pub scan: LegacyScan,
    /// Checkbox per found file.
    pub selected: Vec<bool>,
}

fn home() -> Option<PathBuf> {
    std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")).map(PathBuf::from)
}

/// Saves a converted old presentation as a new `.ohsvc` next to the original or, where
/// that folder cannot be written (a read-only USB stick, a file the flatpak only got
/// through the file chooser portal), into the Documents folder.
fn save_converted(original: &std::path::Path, service: &Service) -> Result<PathBuf, String> {
    let next_to = storage::unique_path(&original.with_extension(formats::SERVICE_EXT));
    let Err(first) = formats::save_service(&next_to, service) else { return Ok(next_to) };
    let docs = directories::UserDirs::new().and_then(|u| u.document_dir().map(PathBuf::from)).or_else(home);
    let Some(name) = next_to.file_name() else { return Err(first) };
    let Some(dir) = docs else { return Err(first) };
    let target = storage::unique_path(&dir.join(name));
    formats::save_service(&target, service).map_err(|_| first)?;
    Ok(target)
}

impl App {
    /// Starts searching the home folder for old files in the background.
    pub fn start_legacy_scan(&mut self, manual: bool) {
        let Some(home) = home() else {
            if manual {
                self.pick_legacy_files();
            }
            return;
        };
        let (tx, rx) = channel();
        std::thread::spawn(move || {
            let _ = tx.send(formats::scan_legacy(&home, 7, 300_000));
        });
        if manual {
            self.notify(t("legacy.searching"));
        }
        self.legacy = Some(LegacyState { rx: Some(rx), manual, scan: LegacyScan::default(), selected: vec![] });
    }

    /// Offers the files of a finished search (the dialog opens on the next frame).
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn offer_legacy(&mut self, scan: LegacyScan, manual: bool) {
        let selected = vec![true; scan.files.len()];
        self.legacy = Some(LegacyState { rx: None, manual, scan, selected });
    }

    pub fn poll_legacy_scan(&mut self, ctx: &egui::Context) {
        let Some(state) = &mut self.legacy else { return };
        if let Some(rx) = &state.rx {
            match rx.try_recv() {
                Ok(scan) => {
                    state.selected = vec![true; scan.files.len()];
                    state.scan = scan;
                    state.rx = None;
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => {
                    ctx.request_repaint_after(std::time::Duration::from_millis(200));
                    return;
                }
                Err(_) => state.rx = None,
            }
        }
        let found = !state.scan.files.is_empty() || state.scan.old_app_dir.is_some();
        if !found && !state.manual {
            // nothing of the old app on this computer: do not search again on the next start
            self.legacy = None;
            self.settings.legacy_import_answered = true;
            self.save_settings();
            return;
        }
        // wait until no other dialog is open
        if self.dialog.is_none() {
            self.dialog = Some(Dialog::LegacyImport);
        } else if !matches!(self.dialog, Some(Dialog::LegacyImport)) {
            ctx.request_repaint_after(std::time::Duration::from_millis(300));
        }
    }

    pub fn legacy_import_dialog(&mut self, ui: &mut egui::Ui, close: &mut bool) {
        let Some(state) = &mut self.legacy else {
            *close = true;
            return;
        };
        ui.heading(t("legacy.title"));
        ui.label(t("legacy.text"));
        if let Some(dir) = &state.scan.old_app_dir {
            ui.weak(tf("legacy.old_dir", &[("dir", &dir.to_string_lossy())]));
        }
        ui.add_space(6.0);
        if state.scan.files.is_empty() {
            ui.label(t("legacy.none_found"));
        } else {
            ui.horizontal(|ui| {
                ui.label(tf("legacy.found", &[("n", &state.scan.files.len().to_string())]));
                if ui.small_button(t("legacy.select_all")).clicked() {
                    state.selected.iter_mut().for_each(|s| *s = true);
                }
                if ui.small_button(t("legacy.select_none")).clicked() {
                    state.selected.iter_mut().for_each(|s| *s = false);
                }
            });
            egui::ScrollArea::vertical().max_height(260.0).show_following_focus(ui, |ui| {
                for (f, sel) in state.scan.files.iter().zip(state.selected.iter_mut()) {
                    let kind = match f.extension().map(|e| e.to_string_lossy().to_lowercase()).as_deref() {
                        Some("ohdata") => t("legacy.kind_songs"),
                        Some("ohcanvas") => t("legacy.kind_design"),
                        _ => t("legacy.kind_presentation"),
                    };
                    ui.checkbox(sel, format!("{kind}: {}", f.display()));
                }
            });
            ui.weak(t("legacy.ohpres_note"));
        }
        ui.add_space(8.0);
        let chosen: Vec<PathBuf> = state.scan.files.iter().zip(&state.selected).filter(|(_, s)| **s).map(|(f, _)| f.clone()).collect();
        let mut action = None;
        ui.horizontal_wrapped(|ui| {
            if !chosen.is_empty() && ui.button(tf("legacy.import_selected", &[("n", &chosen.len().to_string())])).clicked() {
                action = Some(1);
            }
            if ui.button(t("legacy.choose")).clicked() {
                action = Some(2);
            }
            if ui.button(t("legacy.later")).on_hover_text(t("legacy.later_hint")).clicked() {
                action = Some(3);
            }
            if ui.button(t("legacy.never")).on_hover_text(t("legacy.never_hint")).clicked() {
                action = Some(4);
            }
        });
        let Some(action) = action else { return };
        *close = true;
        self.legacy = None;
        if action != 3 {
            self.settings.legacy_import_answered = true;
            self.save_settings();
        }
        match action {
            1 => self.import_legacy_files(chosen),
            2 => self.pick_legacy_files(),
            _ => {}
        }
    }

    fn pick_legacy_files(&mut self) {
        self.open_file_dialog(
            files::Spec::new(files::Mode::PickFiles, t("legacy.menu")).filter(t("legacy.filter"), formats::LEGACY_EXTS),
            files::Purpose::ImportLegacy,
        );
    }

    /// Imports old files: songs and designs go into the library (identical ones are
    /// skipped), presentations are also converted into a new `.ohsvc` next to the original.
    pub fn import_legacy_files(&mut self, chosen: Vec<PathBuf>) {
        let (mut songs, mut designs, mut services, mut errors) = (0, 0, Vec::new(), Vec::new());
        for f in chosen {
            let ext = f.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
            let result: Result<(), String> = match ext.as_str() {
                "ohdata" => formats::import_ohdata(&f).map(|list| {
                    for s in list {
                        songs += self.legacy_add_song(s, &mut errors);
                    }
                }),
                "ohcanvas" => formats::import_ohcanvas(&f).map(|d| designs += self.legacy_add_design(d, &mut errors)),
                "ohpres" => formats::import_ohpres(&f).and_then(|mut service| {
                    for item in &mut service.items {
                        match &mut item.kind {
                            ItemKind::Song(s) => songs += self.legacy_add_song(s.clone(), &mut errors),
                            ItemKind::Design(d) => designs += self.legacy_add_design(d.clone(), &mut errors),
                            _ => {}
                        }
                    }
                    let target = save_converted(&f, &service)?;
                    self.settings.add_recent(&target.to_string_lossy());
                    services.push(target);
                    Ok(())
                }),
                _ => Err(t("legacy.unknown_file")),
            };
            if let Err(e) = result {
                errors.push(format!("{}: {e}", f.display()));
            }
        }
        self.save_settings();
        let mut msg = tf(
            "legacy.done",
            &[("songs", &songs.to_string()), ("designs", &designs.to_string()), ("services", &services.len().to_string())],
        );
        for s in &services {
            msg.push_str(&format!("\n  {}", s.display()));
        }
        if !errors.is_empty() {
            msg.push_str(&format!("\n\n{}", errors.join("\n")));
        }
        self.dialog = Some(Dialog::Message(msg));
    }

    /// Adds a song unless the library has one with the same title and text. Returns 1 if added.
    fn legacy_add_song(&mut self, song: Song, errors: &mut Vec<String>) -> usize {
        if self.lib.songs.iter().any(|s| s.title == song.title && s.parts == song.parts) {
            return 0;
        }
        match self.lib.import_song(song) {
            Ok(added) => added as usize,
            Err(e) => {
                errors.push(e.to_string());
                0
            }
        }
    }

    fn legacy_add_design(&mut self, design: Design, errors: &mut Vec<String>) -> usize {
        // old objects without an id get a new random one on every import, so compare without ids
        let without_ids = |doc: &CanvasDoc| {
            let mut doc = doc.clone();
            doc.objects.iter_mut().for_each(|o| o.id.clear());
            doc
        };
        let doc = without_ids(&design.doc);
        if self.lib.designs.iter().any(|d| d.name == design.name && without_ids(&d.doc) == doc) {
            return 0;
        }
        match self.lib.import_design(design) {
            Ok(added) => added as usize,
            Err(e) => {
                errors.push(e.to_string());
                0
            }
        }
    }
}
