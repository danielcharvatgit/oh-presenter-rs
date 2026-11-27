//! System file dialogs that do not block the user interface.
//!
//! A native file dialog (`rfd`) is modal and blocks the thread that opens it.
//! Opened from inside a frame it stops the whole event loop: the window is no
//! longer repainted, the desktop reports it as "not responding", the output
//! window freezes and on Linux (xdg portal) the dialog can even end up behind
//! the frozen window. So the dialog runs on its own thread and the app polls
//! for the answer every frame; until then the UI shows a small "waiting" modal
//! and keeps drawing (including the output window).
//!
//! macOS only allows dialogs on the main thread, so there the dialog still runs
//! synchronously.

use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, TryRecvError};

use super::{Then, WriteKind};

/// What kind of dialog to show.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    PickFile,
    PickFiles,
    SaveFile,
    PickFolder,
}

/// Everything needed to build the dialog (plain data, so it can be sent to a thread).
#[derive(Clone, Debug)]
pub struct Spec {
    pub mode: Mode,
    pub title: String,
    pub filter: Option<(String, Vec<String>)>,
    pub file_name: Option<String>,
}

impl Spec {
    pub fn new(mode: Mode, title: String) -> Self {
        Self { mode, title, filter: None, file_name: None }
    }

    pub fn filter(mut self, name: impl Into<String>, exts: &[&str]) -> Self {
        self.filter = Some((name.into(), exts.iter().map(|e| e.to_string()).collect()));
        self
    }

    pub fn file_name(mut self, name: impl Into<String>) -> Self {
        self.file_name = Some(name.into());
        self
    }

    /// Shows the dialog and waits for it (blocks the calling thread).
    fn run(self) -> Vec<PathBuf> {
        let mut d = rfd::FileDialog::new().set_title(&self.title);
        if let Some((name, exts)) = &self.filter {
            d = d.add_filter(name, exts);
            // "all files" too, so a file with an unusual extension can still be chosen
            if self.mode != Mode::SaveFile {
                d = d.add_filter(crate::i18n::t("file.all_filter"), &["*"]);
            }
        }
        if let Some(n) = &self.file_name {
            d = d.set_file_name(n);
        }
        match self.mode {
            Mode::PickFile => d.pick_file().into_iter().collect(),
            Mode::PickFiles => d.pick_files().unwrap_or_default(),
            Mode::SaveFile => d.save_file().into_iter().collect(),
            Mode::PickFolder => d.pick_folder().into_iter().collect(),
        }
    }
}

/// What to do with the chosen file(s).
#[derive(Clone, Debug)]
pub enum Purpose {
    OpenService,
    /// Save the service into the chosen file, then (if saved) do `then`.
    SaveServiceAs {
        then: Option<Then>,
    },
    /// Export to a new file with this extension.
    Export {
        kind: WriteKind,
        ext: String,
    },
    ExportAllSongs,
    ImportSongs,
    ImportDesigns,
    ImportImages,
    ImportVideos,
    ImportBackup,
    ImportBible,
    /// Files of the old oh-presenter (`.ohdata`, `.ohcanvas`, `.ohpres`).
    ImportLegacy,
    /// Add an image object to the design in the designer.
    DesignerAddImage,
    /// Replace the picture of the image object with this id.
    DesignerReplaceImage(String),
    /// A background picture of the service or of a service item.
    Background(super::backgrounds::BackgroundTarget),
}

/// A dialog that is open right now.
pub struct Pending {
    pub purpose: Purpose,
    rx: Receiver<Vec<PathBuf>>,
}

impl Pending {
    /// Opens the dialog (on another thread where the platform allows it).
    pub fn open(spec: Spec, purpose: Purpose) -> Self {
        let (tx, rx) = mpsc::channel();
        if cfg!(target_os = "macos") {
            let _ = tx.send(spec.run());
        } else {
            let spawned = std::thread::Builder::new().name("file-dialog".into()).spawn({
                let tx = tx.clone();
                let spec = spec.clone();
                move || {
                    let _ = tx.send(spec.run());
                }
            });
            if spawned.is_err() {
                let _ = tx.send(spec.run());
            }
        }
        Self { purpose, rx }
    }

    /// A dialog the user already answered (`None` = still open), for tests.
    #[cfg(test)]
    pub fn answered(purpose: Purpose, files: Option<Vec<PathBuf>>) -> (Self, mpsc::Sender<Vec<PathBuf>>) {
        let (tx, rx) = mpsc::channel();
        if let Some(f) = files {
            let _ = tx.send(f);
        }
        (Self { purpose, rx }, tx)
    }

    /// `Some(files)` once the dialog was closed (empty = cancelled).
    pub fn poll(&self) -> Option<Vec<PathBuf>> {
        match self.rx.try_recv() {
            Ok(files) => Some(files),
            Err(TryRecvError::Empty) => None,
            // the dialog thread died (e.g. a panic in the backend): treat as cancelled
            Err(TryRecvError::Disconnected) => Some(Vec::new()),
        }
    }
}
