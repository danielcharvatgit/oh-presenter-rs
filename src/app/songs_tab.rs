use super::{App, Dialog, FollowFocus, Then, WriteKind};
use crate::i18n::{t, tf};
use crate::model::*;
use crate::render;
use crate::shortcuts::Action;
use crate::storage::fold;
use eframe::egui::{self, RichText, Sense, Vec2};

impl App {
    pub fn songs_tab(&mut self, ui: &mut egui::Ui) {
        egui::Panel::left("song_library").resizable(true).default_size(260.0).show(ui, |ui| self.song_library(ui));
        egui::Panel::right("song_preview").resizable(true).default_size(260.0).show(ui, |ui| self.song_preview(ui));
        egui::CentralPanel::default_margins().show(ui, |ui| {
            self.key_scroll_area(ui, "song_editor", egui::ScrollArea::vertical().auto_shrink([false, false]), |app, ui| {
                app.song_editor(ui)
            });
        });
    }

    fn song_library(&mut self, ui: &mut egui::Ui) {
        ui.heading(t("songs.library"));
        ui.horizontal(|ui| {
            if ui.button((super::plus_icon(), t("songs.new"))).clicked() {
                self.request(Then::NewSong);
            }
            if ui.button(t("import.songs")).on_hover_text(t("import.songs_hint")).clicked() {
                self.import_songs();
            }
        });
        let r = ui.add(egui::TextEdit::singleline(&mut self.songs.search).hint_text(t("songs.search_hint")).desired_width(f32::INFINITY));
        if self.focus_search {
            r.request_focus();
            self.focus_search = false;
        }
        ui.separator();
        let q = fold(&self.songs.search);
        let mut open = None;
        let mut add = None;
        egui::ScrollArea::vertical().auto_shrink([false, false]).show_following_focus(ui, |ui| {
            let mut shown = 0;
            for s in &self.lib.songs {
                if !q.is_empty() && !fold(&s.title).contains(&q) && !s.text_matches(&q) {
                    continue;
                }
                shown += 1;
                let sel = s.id == self.songs.song.id;
                let r = ui.add_sized([ui.available_width(), 20.0], egui::Button::selectable(sel, &s.title));
                if r.clicked() && !sel {
                    open = Some(s.clone());
                }
                if r.double_clicked() {
                    add = Some(s.clone());
                }
                r.on_hover_text(t("songs.double_click_adds"));
            }
            if shown == 0 {
                ui.weak(if self.lib.songs.is_empty() { t("songs.library_empty") } else { t("songs.nothing_found") });
            }
        });
        if let Some(s) = add {
            self.add_to_service(ItemKind::Song(s));
        }
        if let Some(s) = open {
            self.request(Then::EditSong(s));
        }
    }

    fn song_editor(&mut self, ui: &mut egui::Ui) {
        let sc = self.settings.shortcuts.clone();
        let mut changed = false;
        ui.horizontal_wrapped(|ui| {
            let save = if self.songs.dirty { format!("💾 {} *", t("songs.save")) } else { format!("💾 {}", t("songs.save")) };
            if ui.button(save).on_hover_text(format!("{}\n{}", t("songs.save_hint"), sc.describe(Action::Save))).clicked() {
                self.save_song(false, false);
            }
            if ui
                .button(t("songs.save_as_new"))
                .on_hover_text(format!("{}\n{}", t("songs.save_as_new_hint"), sc.describe(Action::SaveAs)))
                .clicked()
            {
                self.save_song(true, false);
            }
            if ui.button((super::plus_icon(), t("songs.add_to_service"))).clicked() {
                self.add_to_service(ItemKind::Song(self.songs.song.clone()));
            }
            if ui.button(t("songs.export")).on_hover_text(t("songs.export_hint")).clicked() {
                let name = format!("{}.xml", crate::storage::safe_file_name(&self.songs.song.title));
                self.export_to(WriteKind::SongOpenLyrics(self.songs.song.clone()), &name, "OpenLyrics", "xml");
            }
            if self.songs.in_library && ui.button(format!("✖ {}", t("songs.delete"))).clicked() {
                self.dialog = Some(Dialog::DeleteSong(self.songs.song.id.clone()));
            }
        });
        ui.weak(if self.songs.in_library { t("songs.in_library") } else { t("songs.not_in_library") });
        ui.separator();

        let song = &mut self.songs.song;
        egui::Grid::new("song_meta").num_columns(2).spacing([8.0, 6.0]).show(ui, |ui| {
            for (key, value) in [
                ("songs.title", &mut song.title),
                ("songs.author", &mut song.author),
                ("songs.copyright", &mut song.copyright),
                ("songs.ccli", &mut song.ccli),
            ] {
                ui.label(t(key));
                changed |= ui.add(egui::TextEdit::singleline(value).desired_width(f32::INFINITY)).changed();
                ui.end_row();
            }
            ui.label(t("songs.order"));
            ui.vertical(|ui| {
                changed |= ui
                    .add(egui::TextEdit::singleline(&mut song.order).hint_text(t("songs.order_hint")).desired_width(f32::INFINITY))
                    .changed();
                ui.horizontal_wrapped(|ui| {
                    ui.weak(t("songs.order_add"));
                    let codes: Vec<String> = song.parts.iter().map(|p| p.code()).collect();
                    for c in codes {
                        if ui.small_button(&c).clicked() {
                            if !song.order.trim().is_empty() {
                                song.order.push(' ');
                            }
                            song.order.push_str(&c);
                            changed = true;
                        }
                    }
                    if !song.order.is_empty() && ui.small_button("⌫").on_hover_text(t("songs.order_remove_last")).clicked() {
                        let mut v: Vec<&str> = song.order.split_whitespace().collect();
                        v.pop();
                        song.order = v.join(" ");
                        changed = true;
                    }
                });
            });
            ui.end_row();
        });
        ui.separator();
        let mut remove_column = None;
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new(t("songs.columns")).strong()).on_hover_text(t("songs.columns_hint"));
            for (i, col) in song.columns.iter_mut().enumerate() {
                ui.push_id(("col", i), |ui| {
                    ui.group(|ui| {
                        changed |= ui.add(egui::TextEdit::singleline(&mut col.name).desired_width(90.0)).changed();
                        changed |= ui.checkbox(&mut col.show, t("songs.column_show")).on_hover_text(t("songs.column_show_hint")).changed();
                        if ui.small_button("✖").on_hover_text(t("songs.column_remove")).clicked() {
                            remove_column = Some(i);
                        }
                    });
                });
            }
            ui.add(egui::TextEdit::singleline(&mut self.songs.new_column).hint_text(t("songs.column_name_hint")).desired_width(110.0));
            if ui.button((super::plus_icon(), t("songs.column_add"))).on_hover_text(t("songs.columns_hint")).clicked() {
                let n = song.column_count() + 1;
                let name = self.songs.new_column.trim().to_string();
                let name = if name.is_empty() { tf("songs.column_n", &[("n", &n.to_string())]) } else { name };
                song.add_column(name, tf("songs.column_n", &[("n", "1")]));
                self.songs.new_column.clear();
                changed = true;
            }
            if song.shown_columns().len() > 1 {
                ui.separator();
                ui.label(t("songs.column_layout"));
                changed |= ui.radio_value(&mut song.column_layout, ColumnLayout::SideBySide, t("songs.layout_side")).changed();
                changed |= ui.radio_value(&mut song.column_layout, ColumnLayout::Stacked, t("songs.layout_stacked")).changed();
            }
        });
        if let Some(c) = remove_column {
            if song.parts.iter().any(|p| !p.column_text(c).trim().is_empty()) {
                self.dialog = Some(Dialog::RemoveSongColumn(c));
            } else {
                song.remove_column(c);
                changed = true;
            }
        }
        let columns = song.column_count();
        let names: Vec<String> = (0..columns).map(|c| song.columns.get(c).map(|c| c.name.clone()).unwrap_or_default()).collect();
        ui.separator();
        ui.horizontal(|ui| {
            ui.label(RichText::new(t("songs.parts")).strong());
            ui.weak(t("songs.split_hint"));
        });

        let marks = self.settings.marks.clone();
        let mut remove = None;
        let mut swap = None;
        let n = song.parts.len();
        for (i, part) in song.parts.iter_mut().enumerate() {
            ui.push_id(i, |ui| {
                ui.horizontal(|ui| {
                    let current = marks.iter().find(|m| m.code == part.tag).map(|m| m.display_name()).unwrap_or_else(|| part.tag.clone());
                    egui::ComboBox::from_id_salt("tag").selected_text(current).show_ui(ui, |ui| {
                        for m in &marks {
                            if ui.selectable_value(&mut part.tag, m.code.clone(), format!("{} ({})", m.display_name(), m.code)).changed() {
                                changed = true;
                            }
                        }
                    });
                    ui.label(t("songs.number"));
                    changed |= ui.add(egui::DragValue::new(&mut part.number).range(0..=99)).changed();
                    ui.weak(format!("= {}", part.code()));
                    if ui.small_button("⬆").clicked() && i > 0 {
                        swap = Some((i, i - 1));
                    }
                    if ui.small_button("⬇").clicked() && i + 1 < n {
                        swap = Some((i, i + 1));
                    }
                    if ui.small_button("✖").on_hover_text(t("songs.remove_part")).clicked() {
                        remove = Some(i);
                    }
                });
                if columns == 1 {
                    changed |= ui.add(egui::TextEdit::multiline(&mut part.text).desired_rows(4).desired_width(f32::INFINITY)).changed();
                } else {
                    ui.columns(columns, |cols| {
                        for (c, ui) in cols.iter_mut().enumerate() {
                            ui.weak(&names[c]);
                            let edit = egui::TextEdit::multiline(part.column_text_mut(c))
                                .hint_text(&names[c])
                                .desired_rows(4)
                                .desired_width(f32::INFINITY);
                            changed |= ui.add(edit).changed();
                        }
                    });
                }
                ui.add_space(6.0);
            });
        }
        if let Some((a, b)) = swap {
            song.parts.swap(a, b);
            changed = true;
        }
        if let Some(i) = remove {
            song.parts.remove(i);
            changed = true;
        }
        ui.horizontal_wrapped(|ui| {
            ui.label(t("songs.add_part"));
            for m in &marks {
                if ui.button(m.display_name()).clicked() {
                    let number = song.next_number(&m.code);
                    let number = if m.code != "V" && number == 1 { 0 } else { number };
                    song.parts.push(SongPart { tag: m.code.clone(), number, text: String::new(), translations: Vec::new() });
                    changed = true;
                }
            }
        });
        if changed {
            self.songs.dirty = true;
        }
    }

    fn song_preview(&mut self, ui: &mut egui::Ui) {
        ui.heading(t("songs.preview"));
        let item = ServiceItem::new(ItemKind::Song(self.songs.song.clone()));
        let slides = self.item_slides(&item);
        let w = ui.available_width();
        let size = Vec2::new(w, w * CANVAS_H / CANVAS_W);
        egui::ScrollArea::vertical().auto_shrink([false, false]).show_following_focus(ui, |ui| {
            for (i, s) in slides.iter().enumerate() {
                ui.label(format!("{}  {}", i + 1, self.slide_label(s)));
                let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
                render::draw_scene(ui.painter(), rect, &s.scene, &mut self.textures);
                ui.add_space(4.0);
            }
            if slides.is_empty() {
                ui.weak(t("service.no_slides"));
            }
        });
    }
}
