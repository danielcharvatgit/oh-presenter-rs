//! The output window (projector): a second browser window (`index.html#output`) showing
//! what is live. The main window sends it the live slide over a `BroadcastChannel` (also
//! offline); pictures it reads from IndexedDB itself. Double click or F: full screen.

use crate::model::{Alert, Scene};
use crate::render::{self, TextureCache};
use crate::shortcuts::{Action, Shortcuts};
use eframe::egui::{self, Color32};
use serde::{Deserialize, Serialize};
use std::cell::RefCell;
use std::rc::Rc;

/// What the main window sends to the output window.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct OutputMsg {
    pub scene: Scene,
    /// The alert running across, and how many seconds it has run.
    pub alert: Option<(Alert, f32)>,
    /// The pictures the scene uses (to load from IndexedDB).
    pub media: Vec<String>,
    /// The output window's full screen key.
    pub fullscreen_keys: Vec<crate::shortcuts::KeyBinding>,
    /// The language of the main window (for the output window's few texts).
    #[serde(default)]
    pub language: String,
}

/// What the output window sends to the main window.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum FromOutput {
    /// Just opened (or reloaded): send the live slide.
    Hello,
    /// A key pressed in the output window (slide keys work there too).
    Action(Action),
}

pub struct OutputApp {
    msg: Option<OutputMsg>,
    /// When the alert started, on this page's clock (ms).
    alert_start: f64,
    tex: TextureCache,
    loaded: Rc<RefCell<crate::service_file::MediaFiles>>,
    requested: std::collections::HashSet<String>,
    opened: f64,
    shortcuts: Shortcuts,
}

impl OutputApp {
    pub fn new(ctx: &egui::Context) -> Self {
        crate::app::set_theme(ctx, true);
        crate::js::output_post(&serde_json::to_string(&FromOutput::Hello).unwrap());
        Self {
            msg: None,
            alert_start: 0.0,
            tex: TextureCache::default(),
            loaded: Rc::default(),
            requested: Default::default(),
            opened: crate::js::now_ms(),
            shortcuts: Shortcuts::default(),
        }
    }

    fn receive(&mut self) {
        for text in crate::js::take_output_messages() {
            let Ok(msg) = serde_json::from_str::<OutputMsg>(&text) else { continue };
            if let Some((_, elapsed)) = &msg.alert {
                let same = self.msg.as_ref().and_then(|m| m.alert.as_ref()).map(|a| &a.0) == msg.alert.as_ref().map(|a| &a.0);
                if !same {
                    self.alert_start = crate::js::now_ms() - *elapsed as f64 * 1000.0;
                }
            }
            for m in &msg.media {
                if self.requested.insert(m.clone()) {
                    let (key, name, slot) = (format!("media:{m}"), m.clone(), self.loaded.clone());
                    crate::js::spawn(async move {
                        if let Ok(Some(bytes)) = crate::js::db_get_bytes(&key).await {
                            slot.borrow_mut().push((name, bytes));
                        }
                    });
                }
            }
            self.shortcuts.map.insert(Action::OutputFullscreen, msg.fullscreen_keys.clone());
            if !msg.language.is_empty() && msg.language != crate::i18n::current_language() {
                crate::i18n::set_language(&msg.language);
            }
            self.msg = Some(msg);
        }
        for (name, bytes) in self.loaded.borrow_mut().drain(..) {
            self.tex.add_media(&name, bytes);
        }
    }

    pub fn frame(&mut self, ui: &mut egui::Ui) {
        self.receive();
        let ctx = ui.ctx().clone();
        // the keys of the main window work here too (they are sent there)
        let actions = self.shortcuts.triggered(&ctx, |a| a.is_navigation() || a == Action::OutputFullscreen);
        for a in actions {
            if a == Action::OutputFullscreen {
                crate::js::toggle_fullscreen();
            } else {
                crate::js::output_post(&serde_json::to_string(&FromOutput::Action(a)).unwrap());
            }
        }
        egui::CentralPanel::no_frame().show(ui, |ui| {
            let full = ui.max_rect();
            let r = ui.interact(full, egui::Id::unique("output"), egui::Sense::click());
            if r.double_clicked() {
                crate::js::toggle_fullscreen();
            }
            let painter = ui.painter();
            painter.rect_filled(full, 0.0, Color32::BLACK);
            let rect = render::fit_16_9(full);
            let mut alert_runs = false;
            match &self.msg {
                Some(m) => {
                    render::draw_scene(painter, rect, &m.scene, &mut self.tex);
                    if let Some((alert, _)) = &m.alert {
                        let elapsed = ((crate::js::now_ms() - self.alert_start) / 1000.0) as f32;
                        alert_runs = render::draw_alert(painter, rect, alert, elapsed);
                    }
                }
                None => {
                    painter.text(
                        rect.center(),
                        egui::Align2::CENTER_CENTER,
                        crate::i18n::t("output.waiting"),
                        egui::FontId::proportional(18.0),
                        Color32::GRAY,
                    );
                }
            }
            // a hint for the first seconds, and no mouse pointer over the slides in full screen
            if crate::js::now_ms() - self.opened < 6000.0 && !crate::js::is_fullscreen() {
                painter.text(
                    full.left_top() + egui::vec2(10.0, 10.0),
                    egui::Align2::LEFT_TOP,
                    crate::i18n::t("output.hint"),
                    egui::FontId::proportional(14.0),
                    Color32::from_gray(160),
                );
            }
            if crate::js::is_fullscreen() {
                ui.ctx().set_cursor_icon(egui::CursorIcon::None);
            }
            if alert_runs {
                ui.ctx().request_repaint();
            }
        });
        // the main window's messages arrive between frames
        ctx.request_repaint_after(std::time::Duration::from_millis(100));
    }
}

impl eframe::App for OutputApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.frame(ui);
    }

    fn raw_input_hook(&mut self, _ctx: &egui::Context, raw: &mut egui::RawInput) {
        crate::shortcuts::digits_by_position(&mut raw.events);
    }
}
