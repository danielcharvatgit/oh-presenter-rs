//! Alerts: a message running across the output (and the network outputs) over
//! whatever is shown — e.g. "The owner of car 1AB 2345 is asked to come out".
//! F7 opens the alert window with the default message; the look is set in Settings.

use super::{App, Dialog};
use crate::i18n::{t, tf};
use crate::model::*;
use crate::render;
use eframe::egui::{self, RichText, Sense, Vec2};
use std::time::Instant;

impl App {
    pub fn open_alert_dialog(&mut self) {
        if self.dialog.is_some() {
            return;
        }
        self.alert_text = match &self.alert {
            // a running alert can be edited and sent again
            Some((a, _)) => a.text.clone(),
            None => self.settings.alert_default_message.clone(),
        };
        self.dialog = Some(Dialog::Alert);
        // the text field gets the focus once, when the window opens; after that Tab
        // moves on to the buttons as usual
        self.alert_focus = true;
    }

    /// Sends an alert to the output window and the network outputs.
    pub fn send_alert(&mut self, text: &str) {
        let text = text.trim();
        if text.is_empty() {
            return;
        }
        let alert = Alert {
            text: text.to_string(),
            style: self.settings.alert_style.clone(),
            duration: self.settings.alert_style.timeout.max(0.0),
        };
        if let Some(server) = &self.server {
            server.alert(Some(&alert));
        }
        self.alert = Some((alert, Instant::now()));
    }

    pub fn stop_alert(&mut self) {
        if self.alert.take().is_some()
            && let Some(server) = &self.server
        {
            server.alert(None);
        }
    }

    /// Ends the alert when its time is up.
    pub fn poll_alert(&mut self, ctx: &egui::Context) {
        let Some((alert, since)) = &self.alert else { return };
        let painter = ctx.layer_painter(egui::LayerId::background());
        match render::alert_run_secs(&painter, alert) {
            Some(end) if since.elapsed().as_secs_f32() >= end => self.stop_alert(),
            // keeps the countdown in the status bar moving
            _ => ctx.request_repaint_after(std::time::Duration::from_millis(500)),
        }
    }

    pub fn alert_dialog(&mut self, ui: &mut egui::Ui, close: &mut bool) {
        ui.heading(t("alert.title"));
        let r = ui.add(egui::TextEdit::singleline(&mut self.alert_text).hint_text(t("alert.hint")).desired_width(440.0));
        if std::mem::take(&mut self.alert_focus) {
            r.request_focus();
        }
        let enter = r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
        let st = &self.settings.alert_style;
        let timeout =
            if st.timeout > 0.0 { tf("alert.runs_for", &[("s", &format!("{:.0}", st.timeout))]) } else { t("alert.runs_until_stopped") };
        ui.weak(format!("{timeout} · {}", t("alert.look_in_settings")));
        ui.add_space(8.0);
        let mut send = enter;
        ui.horizontal(|ui| {
            send |= ui.add_enabled(!self.alert_text.trim().is_empty(), egui::Button::new(format!("📢 {}", t("alert.send")))).clicked();
            if self.alert.is_some() && ui.button(format!("■ {}", t("alert.stop"))).clicked() {
                self.stop_alert();
                *close = true;
            }
            if ui.button(t("cancel")).clicked() {
                *close = true;
            }
        });
        if send && !self.alert_text.trim().is_empty() {
            let text = self.alert_text.clone();
            self.send_alert(&text);
            *close = true;
        }
    }

    /// Status bar: the running alert with a Stop button.
    pub fn alert_status(&mut self, ui: &mut egui::Ui) {
        let Some((alert, since)) = &self.alert else { return };
        let mut text: String = alert.text.chars().take(40).collect();
        if alert.text.chars().count() > 40 {
            text.push('…');
        }
        let left = render::alert_run_secs(ui.painter(), alert).map(|end| (end - since.elapsed().as_secs_f32()).max(0.0));
        let label = match left {
            Some(s) => tf("alert.running_for", &[("text", &text), ("s", &format!("{s:.0}"))]),
            None => tf("alert.running", &[("text", &text)]),
        };
        ui.colored_label(super::LIVE_COLOR, format!("📢 {label}"));
        if ui.small_button(format!("■ {}", t("alert.stop"))).clicked() {
            self.stop_alert();
        }
        ui.separator();
    }

    /// Settings → Alerts.
    pub fn settings_alerts(&mut self, ui: &mut egui::Ui) -> bool {
        let mut changed = false;
        ui.heading(t("settings.alerts"));
        ui.weak(t("settings.alerts_hint"));
        let st = &mut self.settings.alert_style;
        egui::Grid::new("alerts").num_columns(2).spacing([12.0, 8.0]).show(ui, |ui| {
            ui.label(t("settings.alert_default"));
            changed |= ui
                .add(egui::TextEdit::singleline(&mut self.settings.alert_default_message).hint_text(t("alert.hint")).desired_width(360.0))
                .changed();
            ui.end_row();
            ui.label(t("settings.alert_size"));
            changed |= ui.add(egui::Slider::new(&mut st.font_size, 16.0..=140.0)).changed();
            ui.end_row();
            for (key, color) in [("settings.alert_color", &mut st.color), ("settings.alert_background", &mut st.background)] {
                ui.label(t(key));
                let mut c = color.to_color32();
                if ui.color_edit_button_srgba(&mut c).changed() {
                    *color = Rgba::from_color32(c);
                    changed = true;
                }
                ui.end_row();
            }
            ui.label(t("settings.alert_font"));
            ui.horizontal(|ui| {
                for (f, key) in [
                    (AlertFont::Sans, "settings.font_sans"),
                    (AlertFont::Serif, "settings.font_serif"),
                    (AlertFont::Monospace, "settings.font_mono"),
                ] {
                    changed |= ui.radio_value(&mut st.font, f, t(key)).changed();
                }
            });
            ui.end_row();
            ui.label(t("settings.alert_position"));
            ui.horizontal(|ui| {
                for (v, key) in [
                    (VAlign::Top, "settings.align_top"),
                    (VAlign::Middle, "settings.align_middle"),
                    (VAlign::Bottom, "settings.align_bottom"),
                ] {
                    changed |= ui.radio_value(&mut st.valign, v, t(key)).changed();
                }
            });
            ui.end_row();
            ui.label(t("settings.alert_direction"));
            ui.horizontal(|ui| {
                changed |= ui.radio_value(&mut st.direction, AlertDirection::RightToLeft, t("settings.rtl")).changed();
                changed |= ui.radio_value(&mut st.direction, AlertDirection::LeftToRight, t("settings.ltr")).changed();
            });
            ui.end_row();
            ui.label(t("settings.alert_speed"));
            changed |= ui.add(egui::Slider::new(&mut st.speed, 40.0..=800.0).suffix(" px/s")).changed();
            ui.end_row();
            ui.label(t("settings.alert_timeout"));
            ui.horizontal(|ui| {
                changed |= ui.add(egui::DragValue::new(&mut st.timeout).range(0.0..=3600.0).speed(1.0).suffix(" s")).changed();
                ui.weak(t("settings.alert_timeout_hint"));
            });
            ui.end_row();
        });
        // live preview over a sample slide
        let w = ui.available_width().min(480.0);
        let (rect, _) = ui.allocate_exact_size(Vec2::new(w, w * CANVAS_H / CANVAS_W), Sense::hover());
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 0.0, self.settings.text_style.background.to_color32());
        painter.text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            t("settings.alert_preview_slide"),
            egui::FontId::proportional(18.0 * w / 480.0),
            self.settings.text_style.color.to_color32().gamma_multiply(0.5),
        );
        let sample = if self.settings.alert_default_message.trim().is_empty() {
            t("settings.alert_preview_text")
        } else {
            self.settings.alert_default_message.clone()
        };
        let preview = Alert { text: sample, style: self.settings.alert_style.clone(), duration: 0.0 };
        let secs = ui.input(|i| i.time) as f32;
        render::draw_alert(&painter, rect, &preview, secs);
        if ui.is_rect_visible(rect) {
            // animate only while the preview is on screen
            ui.ctx().request_repaint_after(std::time::Duration::from_millis(33));
        }
        ui.horizontal(|ui| {
            if ui.button(format!("📢 {}", t("settings.alert_test"))).clicked() {
                let text = preview.text.clone();
                self.send_alert(&text);
            }
            if self.alert.is_some() && ui.button(format!("■ {}", t("alert.stop"))).clicked() {
                self.stop_alert();
            }
            ui.weak(RichText::new(t("settings.alert_shortcut")).small());
        });
        changed
    }
}
