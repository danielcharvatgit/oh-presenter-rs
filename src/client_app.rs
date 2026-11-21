//! `oh-presenter-rs canvas [host[:port]]` — a network output window, the
//! successor of oh-presenter's separate canvas process.

use crate::i18n::t;
use crate::model::Scene;
use crate::net::{ClientEvent, run_client};
use crate::render::{TextureCache, draw_scene_live, install_fonts};
use eframe::egui::{self, Color32, Key, ViewportCommand};
use std::sync::mpsc::Receiver;

pub struct ClientApp {
    rx: Receiver<ClientEvent>,
    scene: Scene,
    connected: Option<std::net::SocketAddr>,
    textures: TextureCache,
    fullscreen: bool,
    alert: Option<(crate::model::Alert, std::time::Instant)>,
    clock: crate::model::ClockStyle,
}

impl ClientApp {
    pub fn new(cc: &eframe::CreationContext, server: Option<String>, name: String) -> Self {
        install_fonts(&cc.egui_ctx, None);
        Self {
            rx: run_client(server, name, cc.egui_ctx.clone()),
            scene: Scene::Blank,
            connected: None,
            textures: TextureCache::new(crate::storage::paths().client_media()),
            fullscreen: false,
            alert: None,
            clock: Default::default(),
        }
    }
}

impl eframe::App for ClientApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        if !crate::start_guard::frame() {
            ui.ctx().request_repaint();
        }
        let ctx = ui.ctx().clone();
        while let Ok(ev) = self.rx.try_recv() {
            match ev {
                ClientEvent::Connected(a) => self.connected = Some(a),
                ClientEvent::Disconnected => self.connected = None,
                ClientEvent::Scene(s) => self.scene = s,
                ClientEvent::Asset(name) => self.textures.forget(&name),
                ClientEvent::Alert(a) => self.alert = a.map(|a| (a, std::time::Instant::now())),
                ClientEvent::Clock(c) => self.clock = c,
            }
        }
        let toggle = ctx.input(|i| i.key_pressed(Key::F11) || i.pointer.button_double_clicked(egui::PointerButton::Primary));
        let escape = ctx.input(|i| i.key_pressed(Key::Escape));
        if toggle || (escape && self.fullscreen) {
            self.fullscreen = !self.fullscreen && !escape;
            ctx.send_viewport_cmd(ViewportCommand::Fullscreen(self.fullscreen));
        }
        let rect = ui.max_rect();
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 0.0, Color32::BLACK);
        let slide = crate::render::fit_16_9(rect);
        draw_scene_live(&painter, slide, &self.scene, &mut self.textures);
        if let Some((alert, since)) = &self.alert {
            if crate::render::draw_alert(&painter, slide, alert, since.elapsed().as_secs_f32()) {
                // ~60 frames per second while it runs
                ctx.request_repaint_after(std::time::Duration::from_millis(16));
            } else {
                self.alert = None;
            }
        }
        // (repainted every 500 ms anyway, see below)
        crate::clock::draw(&painter, rect, slide.width(), &self.clock, crate::clock::now_secs());
        if self.connected.is_none() {
            let text = t("client.searching");
            let pos = rect.right_bottom() - egui::vec2(16.0, 16.0);
            let galley = painter.layout_no_wrap(text, egui::FontId::proportional(18.0), Color32::WHITE);
            let r = egui::Align2::RIGHT_BOTTOM.anchor_size(pos, galley.size()).expand(8.0);
            painter.rect_filled(r, 4.0, Color32::from_black_alpha(160));
            painter.galley(r.min + egui::vec2(8.0, 8.0), galley, Color32::WHITE);
        }
        ctx.request_repaint_after(std::time::Duration::from_millis(500));
    }
}
