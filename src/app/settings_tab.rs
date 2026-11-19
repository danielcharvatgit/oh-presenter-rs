use super::{App, WriteKind};
use crate::i18n::{t, tf};
use crate::model::Rgba;
use crate::settings::SlideMark;
use crate::shortcuts::{Action, Shortcuts, capture_key};
use crate::storage::paths;
use eframe::egui::{self, Key, RichText};

/// Waiting for a key press to bind.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Capture {
    Action(Action),
    Mark(usize),
}

fn color_row(ui: &mut egui::Ui, label: &str, c: &mut Rgba) -> bool {
    ui.label(label);
    let mut c32 = c.to_color32();
    let changed = ui.color_edit_button_srgba(&mut c32).changed();
    if changed {
        *c = Rgba::from_color32(c32);
    }
    ui.end_row();
    changed
}

fn settings_about(ui: &mut egui::Ui) {
    ui.heading(t("settings.about"));
    ui.label(RichText::new("oh! Presenter").size(22.0).strong());
    ui.weak(tf("settings.about_version", &[("version", env!("CARGO_PKG_VERSION"))]));
    ui.add_space(4.0);
    ui.label(t("settings.about_text"));
    ui.label(t("settings.about_formats"));
    ui.weak(t("settings.about_rewrite"));
    ui.weak(t("settings.about_license"));
    ui.add_space(8.0);
}

impl App {
    pub fn settings_tab(&mut self, ui: &mut egui::Ui) {
        self.capture_keys(ui.ctx());
        self.key_scroll_area(ui, "settings", egui::ScrollArea::vertical().auto_shrink([false, false]), |app, ui| {
            ui.set_max_width(900.0);
            let mut changed = false;
            changed |= app.settings_general(ui);
            ui.separator();
            changed |= app.settings_display(ui);
            ui.separator();
            changed |= app.settings_alerts(ui);
            ui.separator();
            app.settings_network(ui);
            ui.separator();
            app.settings_bible(ui);
            ui.separator();
            changed |= app.settings_marks(ui);
            ui.separator();
            changed |= app.settings_shortcuts(ui);
            ui.separator();
            ui.heading(t("settings.data"));
            ui.label(tf("settings.data_dir", &[("dir", &paths().data.to_string_lossy())]));
            ui.label(tf("settings.config_dir", &[("dir", &paths().config.to_string_lossy())]));
            ui.label(tf("settings.backups_dir", &[("dir", &paths().backups().to_string_lossy())]));
            ui.separator();
            settings_about(ui);
            if changed {
                app.save_settings();
            }
        });
    }

    fn capture_keys(&mut self, ctx: &egui::Context) {
        let Some(c) = self.capture else { return };
        let Some(b) = capture_key(ctx) else { return };
        ctx.input_mut(|i| i.events.clear());
        if b.key == Key::Escape && !b.ctrl && !b.alt && !b.shift {
            self.capture = None;
            return;
        }
        match c {
            Capture::Action(a) => {
                let list = self.settings.shortcuts.map.entry(a).or_default();
                if !list.contains(&b) {
                    list.push(b);
                }
                let conflicts = self.settings.shortcuts.conflicts(&b, a);
                if !conflicts.is_empty() {
                    let names: Vec<String> = conflicts.iter().map(|c| t(&c.label_key())).collect();
                    self.notify(tf("settings.conflict", &[("key", &b.to_string()), ("actions", &names.join(", "))]));
                }
            }
            Capture::Mark(i) => {
                if let Some(m) = self.settings.marks.get_mut(i) {
                    m.key = Some(b);
                }
            }
        }
        self.capture = None;
        self.save_settings();
    }

    fn settings_general(&mut self, ui: &mut egui::Ui) -> bool {
        let mut changed = false;
        ui.heading(t("settings.general"));
        egui::Grid::new("general").num_columns(2).spacing([12.0, 8.0]).show(ui, |ui| {
            ui.label(t("settings.language"));
            ui.horizontal(|ui| {
                let langs = crate::i18n::languages();
                let current = langs.iter().find(|(c, _)| *c == self.settings.language).map(|(_, n)| n.clone()).unwrap_or_default();
                egui::ComboBox::from_id_salt("lang").selected_text(current).show_ui(ui, |ui| {
                    for (code, name) in &langs {
                        if ui.selectable_label(*code == self.settings.language, format!("{name} ({code})")).clicked() {
                            self.settings.language = code.clone();
                            crate::i18n::set_language(code);
                            changed = true;
                        }
                    }
                });
                if ui
                    .button(t("settings.lang_template"))
                    .on_hover_text(tf("settings.lang_hint", &[("dir", &paths().lang().to_string_lossy())]))
                    .clicked()
                {
                    let path = paths().lang().join("new-language.lang.txt");
                    self.write_new(WriteKind::LangTemplate, path);
                }
            });
            ui.end_row();
            ui.label("");
            ui.weak(tf("settings.lang_hint", &[("dir", &paths().lang().to_string_lossy())]));
            ui.end_row();

            ui.label(t("settings.theme"));
            ui.horizontal(|ui| {
                if ui.selectable_label(self.settings.dark_mode, t("settings.dark")).clicked() {
                    self.settings.dark_mode = true;
                    ui.ctx().set_visuals(egui::Visuals::dark());
                    changed = true;
                }
                if ui.selectable_label(!self.settings.dark_mode, t("settings.light")).clicked() {
                    self.settings.dark_mode = false;
                    ui.ctx().set_visuals(egui::Visuals::light());
                    changed = true;
                }
            });
            ui.end_row();
            ui.label(t("service.live_mode"));
            changed |= ui.checkbox(&mut self.settings.live_follows_selection, t("service.live_mode_hint")).changed();
            ui.end_row();
        });
        changed
    }

    fn settings_display(&mut self, ui: &mut egui::Ui) -> bool {
        let mut changed = false;
        ui.heading(t("settings.display"));
        let style = &mut self.settings.text_style;
        egui::Grid::new("display").num_columns(2).spacing([12.0, 8.0]).show(ui, |ui| {
            changed |= color_row(ui, &t("settings.text_color"), &mut style.color);
            changed |= color_row(ui, &t("settings.bg_color"), &mut style.background);
            ui.label(t("settings.max_font"));
            changed |= ui.add(egui::Slider::new(&mut style.max_font_size, 16.0..=160.0)).changed();
            ui.end_row();
            ui.label(t("settings.footer_size"));
            changed |= ui.add(egui::Slider::new(&mut style.footer_size, 8.0..=60.0)).changed();
            ui.end_row();
            ui.label(t("settings.footer"));
            changed |= ui.checkbox(&mut style.show_footer, t("settings.show_footer")).changed();
            ui.end_row();
            ui.label(t("settings.shadow"));
            changed |= ui.checkbox(&mut style.shadow, t("settings.show_shadow")).changed();
            ui.end_row();
            ui.label(t("settings.bg_design"));
            let current = self
                .settings
                .text_background_design
                .as_ref()
                .and_then(|id| self.lib.design(id))
                .map(|d| d.name.clone())
                .unwrap_or_else(|| t("settings.none"));
            egui::ComboBox::from_id_salt("bg_design").selected_text(current).show_ui(ui, |ui| {
                if ui.selectable_label(self.settings.text_background_design.is_none(), t("settings.none")).clicked() {
                    self.settings.text_background_design = None;
                    changed = true;
                }
                for d in &self.lib.designs {
                    if ui.selectable_label(self.settings.text_background_design.as_ref() == Some(&d.id), &d.name).clicked() {
                        self.settings.text_background_design = Some(d.id.clone());
                        changed = true;
                    }
                }
            });
            ui.end_row();
            ui.label(t("settings.verses_per_slide"));
            changed |= ui.add(egui::DragValue::new(&mut self.settings.verses_per_slide).range(1..=10)).changed();
            ui.end_row();
            ui.label(t("settings.output_window"));
            changed |= ui
                .checkbox(&mut self.settings.output_always_on_top, t("settings.always_on_top"))
                .on_hover_text(t("settings.always_on_top_hint"))
                .changed();
            ui.end_row();
        });
        if changed {
            // the live text slide picks up the new style right away
            if let Some((item, slide)) = self.live_pos
                && let Some(s) = self.slides_of(item).get(slide)
                && matches!(s.scene, crate::model::Scene::Text(_))
            {
                self.live = s.scene.clone();
                self.broadcast();
            }
        }
        changed
    }

    fn settings_network(&mut self, ui: &mut egui::Ui) {
        ui.heading(t("settings.network"));
        let mut changed = false;
        ui.horizontal(|ui| {
            changed |= ui.checkbox(&mut self.settings.network_enabled, t("settings.network_enabled")).changed();
            ui.label(t("settings.port"));
            changed |= ui.add(egui::DragValue::new(&mut self.settings.network_port).range(1024..=65535)).changed();
        });
        if changed {
            self.save_settings();
            ui.weak(t("settings.restart"));
        }
        ui.weak(t("settings.network_hint"));
        if ui.button(t("output.start_client")).on_hover_text(t("output.start_client_hint")).clicked() {
            self.start_local_client();
        }
        match &self.server {
            Some(s) if s.error.is_some() => {
                ui.colored_label(super::LIVE_COLOR, tf("net.error", &[("err", s.error.as_deref().unwrap_or(""))]));
            }
            Some(s) => {
                let clients = s.clients();
                egui::Grid::new("clients").striped(true).num_columns(3).show(ui, |ui| {
                    ui.label(RichText::new(t("settings.client_name")).strong());
                    ui.label(RichText::new("id").strong());
                    ui.label(RichText::new(t("settings.client_addr")).strong());
                    ui.end_row();
                    for c in clients {
                        ui.label(&c.display_name);
                        ui.label(&c.id);
                        ui.label(c.addr.to_string());
                        ui.end_row();
                    }
                });
            }
            None => {
                ui.weak(t("settings.network_off"));
            }
        }
    }

    fn settings_marks(&mut self, ui: &mut egui::Ui) -> bool {
        let mut changed = false;
        ui.heading(t("settings.marks"));
        ui.weak(t("settings.marks_hint"));
        changed |= ui.checkbox(&mut self.settings.number_keys_jump_to_verse, t("settings.number_keys")).changed();
        let mut remove = None;
        egui::Grid::new("marks").striped(true).num_columns(4).spacing([12.0, 6.0]).show(ui, |ui| {
            ui.label(RichText::new(t("settings.mark_code")).strong());
            ui.label(RichText::new(t("settings.mark_name")).strong());
            ui.label(RichText::new(t("settings.mark_key")).strong());
            ui.label("");
            ui.end_row();
            for (i, m) in self.settings.marks.iter_mut().enumerate() {
                ui.label(&m.code);
                if m.builtin {
                    ui.label(m.display_name());
                } else {
                    changed |= ui.text_edit_singleline(&mut m.name).changed();
                }
                ui.horizontal(|ui| {
                    let waiting = self.capture == Some(Capture::Mark(i));
                    let label =
                        if waiting { t("settings.press_key") } else { m.key.map(|k| k.to_string()).unwrap_or_else(|| "—".into()) };
                    if ui.button(label).on_hover_text(t("settings.click_to_rebind")).clicked() {
                        self.capture = Some(Capture::Mark(i));
                    }
                    if m.key.is_some() && ui.small_button("✖").on_hover_text(t("settings.unbind")).clicked() {
                        m.key = None;
                        changed = true;
                    }
                });
                if !m.builtin && ui.small_button("✖").clicked() {
                    remove = Some(i);
                } else {
                    ui.label("");
                }
                ui.end_row();
            }
        });
        if let Some(i) = remove {
            self.settings.marks.remove(i);
            changed = true;
        }
        ui.horizontal(|ui| {
            ui.label(t("settings.new_mark"));
            ui.add(egui::TextEdit::singleline(&mut self.new_mark.0).hint_text(t("settings.mark_code")).desired_width(60.0));
            ui.add(egui::TextEdit::singleline(&mut self.new_mark.1).hint_text(t("settings.mark_name")).desired_width(160.0));
            let code: String = self.new_mark.0.trim().to_uppercase().chars().filter(|c| c.is_alphanumeric()).collect();
            let code_ok = !code.is_empty() && !code.chars().any(|c| c.is_ascii_digit()) && self.settings.mark(&code).is_none();
            if ui
                .add_enabled(code_ok && !self.new_mark.1.trim().is_empty(), egui::Button::new((super::plus_icon(), t("settings.add"))))
                .clicked()
            {
                self.settings.marks.push(SlideMark { code, name: self.new_mark.1.trim().to_string(), key: None, builtin: false });
                self.new_mark = Default::default();
                changed = true;
            }
        });
        ui.weak(t("settings.mark_code_hint"));
        changed
    }

    fn settings_shortcuts(&mut self, ui: &mut egui::Ui) -> bool {
        let mut changed = false;
        ui.horizontal(|ui| {
            ui.heading(t("settings.shortcuts"));
            if ui.button(t("settings.reset_shortcuts")).clicked() {
                self.settings.shortcuts = Shortcuts::default();
                changed = true;
            }
        });
        ui.weak(t("settings.shortcuts_hint"));
        egui::Grid::new("shortcuts").striped(true).num_columns(2).spacing([12.0, 4.0]).show(ui, |ui| {
            for a in Action::ALL {
                ui.label(t(&a.label_key()));
                ui.horizontal(|ui| {
                    let list = self.settings.shortcuts.bindings(*a);
                    for (j, b) in list.iter().enumerate() {
                        let conflict = !self.settings.shortcuts.conflicts(b, *a).is_empty();
                        let mut text = RichText::new(format!("{b}  ✖"));
                        if conflict {
                            text = text.color(egui::Color32::from_rgb(230, 120, 60));
                        }
                        if ui
                            .small_button(text)
                            .on_hover_text(if conflict { t("settings.conflict_short") } else { t("settings.unbind") })
                            .clicked()
                        {
                            let mut l = list.clone();
                            l.remove(j);
                            self.settings.shortcuts.map.insert(*a, l);
                            changed = true;
                        }
                    }
                    let waiting = self.capture == Some(Capture::Action(*a));
                    if ui
                        .small_button(if waiting { egui::RichText::new(t("settings.press_key")) } else { super::plus_icon() })
                        .on_hover_text(t("settings.add_binding"))
                        .clicked()
                    {
                        self.capture = Some(Capture::Action(*a));
                    }
                    if self.settings.shortcuts.bindings(*a) != a.defaults()
                        && ui.small_button("↺").on_hover_text(t("settings.reset_one")).clicked()
                    {
                        self.settings.shortcuts.map.insert(*a, a.defaults());
                        changed = true;
                    }
                });
                ui.end_row();
            }
        });
        ui.weak(t("settings.designer_keys"));
        changed
    }
}
