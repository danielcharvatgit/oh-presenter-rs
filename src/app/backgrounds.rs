//! Background pictures of the service and of service items (Service tab).
//!
//! An item's picture is drawn under its text slides and under its designs with a
//! transparent background; the service's picture under every text slide without one of its
//! own (not under pictures with transparency: they are overlays).

use super::{App, ItemRef, files};
use crate::i18n::{t, tf};
use crate::model::*;
use eframe::egui;
use std::path::Path;

/// Whose background picture.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BackgroundTarget {
    Service,
    /// A service item, by its id (it may move meanwhile).
    Item(String),
}

impl App {
    /// Reads a picture into the media store as a background picture.
    pub fn background_picture(f: &Path) -> Result<BackgroundPicture, String> {
        let (bytes, ext) = crate::formats::read_image_file(f)?;
        let (width, height) = image::ImageReader::new(std::io::Cursor::new(&bytes))
            .with_guessed_format()
            .map_err(|e| e.to_string())?
            .into_dimensions()
            .map_err(|e| e.to_string())?;
        let media = crate::storage::store_media_bytes(&bytes, &ext).map_err(|e| e.to_string())?;
        Ok(BackgroundPicture { media, width, height })
    }

    /// Sets (or with `None` removes) a background picture; what is live shows it right away.
    pub fn set_background(&mut self, target: &BackgroundTarget, pic: Option<BackgroundPicture>) -> Result<(), String> {
        match target {
            BackgroundTarget::Service => self.service.background = pic,
            BackgroundTarget::Item(id) => {
                let it = self.service.items.iter_mut().find(|it| &it.id == id).ok_or("the item is no longer in the service")?;
                it.background = pic;
            }
        }
        self.service_dirty = true;
        self.refresh_live();
        Ok(())
    }

    /// Builds the live slide again (after its look changed). A blanked output stays blank and
    /// a live video plays on (it has no background to change, and set_live would restart it).
    pub fn refresh_live(&mut self) {
        if matches!(self.live, Scene::Video(_)) {
            return;
        }
        let blank = self.blank;
        if let Some((r, (b, c, v))) = self.bible_outside_live() {
            self.bible_item_show_verse(r, b, c, v);
        } else if let Some((r, s)) = self.live_pos
            && let Some(slide) = self.slides_of(r).get(s)
        {
            self.set_live(slide.scene.clone(), Some((r, s)));
        }
        if blank && !self.blank {
            self.blank = true;
            self.broadcast();
        }
    }

    /// Asks for a picture file for `target` (system file dialog).
    pub fn choose_background(&mut self, target: BackgroundTarget) {
        self.open_file_dialog(
            files::Spec::new(files::Mode::PickFile, t("background.choose")).filter(t("file.image_filter"), super::IMAGE_EXTS),
            files::Purpose::Background(target),
        );
    }

    /// The picture file chosen in the dialog.
    pub(super) fn background_file_chosen(&mut self, target: BackgroundTarget, f: &Path) {
        match Self::background_picture(f).and_then(|pic| self.set_background(&target, Some(pic))) {
            Ok(()) => self.notify(t("background.set")),
            Err(e) => self.error(tf("error.open", &[("file", &f.to_string_lossy()), ("err", &e)])),
        }
    }

    /// A row with a background picture's buttons.
    fn background_row(&mut self, ui: &mut egui::Ui, label: String, hint: String, target: BackgroundTarget, has: bool) {
        let mut clear = false;
        ui.horizontal_wrapped(|ui| {
            ui.label(label).on_hover_text(hint);
            let choose = if has { t("background.change") } else { t("background.choose") };
            if ui.button(format!("🖼 {choose}")).clicked() {
                self.choose_background(target.clone());
            }
            if has && ui.button(format!("✖ {}", t("background.remove"))).clicked() {
                clear = true;
            }
        });
        if clear && let Err(e) = self.set_background(&target, None) {
            self.error(e);
        }
    }

    /// The service's background picture (Service tab, under the service list).
    pub(super) fn service_background_options(&mut self, ui: &mut egui::Ui) {
        let has = self.service.background.is_some();
        self.background_row(ui, t("background.service"), t("background.service_hint"), BackgroundTarget::Service, has);
    }

    /// The selected item's background picture (Service tab, over its slides). Videos and
    /// opaque designs cover the whole slide: no picture for them.
    pub(super) fn background_options(&mut self, ui: &mut egui::Ui, r: ItemRef) {
        let ItemRef::Service(i) = r else { return };
        let Some(it) = self.service.items.get(i) else { return };
        let fits = match &it.kind {
            ItemKind::Video(_) => false,
            ItemKind::Design(d) => d.doc.background.0[3] == 0,
            _ => true,
        };
        if !fits && it.background.is_none() {
            return;
        }
        let (id, has) = (it.id.clone(), it.background.is_some());
        self.background_row(ui, t("background.item"), t("background.item_hint"), BackgroundTarget::Item(id), has);
    }
}
