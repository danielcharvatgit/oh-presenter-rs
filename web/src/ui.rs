//! What the web edition draws: the top bar with the tabs, the Service, Songs, Bible and
//! Settings tabs, the dialogs and the status bar.

use crate::app::{Dialog, ItemRef, Purpose, TABS, Tab, WebApp};
use crate::i18n::{t, tf};
use crate::model::*;
use crate::render;
use crate::shortcuts::{Action, KeyBinding};
use eframe::egui::{self, Color32, RichText, Sense, Vec2};

const LIVE: Color32 = Color32::from_rgb(220, 60, 60);
const THUMB_W: f32 = 176.0;

fn thumb(ui: &mut egui::Ui, app: &mut WebApp, scene: &Scene, w: f32, selected: bool, live: bool) -> egui::Response {
    let (rect, r) = ui.allocate_exact_size(Vec2::new(w, w * CANVAS_H / CANVAS_W), Sense::click());
    if ui.is_rect_visible(rect) {
        render::draw_scene(ui.painter(), rect, scene, &mut app.tex);
        let stroke = if live {
            egui::Stroke::new(3.0, LIVE)
        } else if selected {
            egui::Stroke::new(3.0, ui.visuals().selection.bg_fill)
        } else {
            egui::Stroke::new(1.0, Color32::from_gray(90))
        };
        ui.painter().rect_stroke(rect, 2.0, stroke, egui::StrokeKind::Outside);
    }
    r
}

fn color_row(ui: &mut egui::Ui, label: &str, c: &mut Rgba) {
    ui.label(label);
    let mut c32 = c.to_color32();
    if ui.color_edit_button_srgba(&mut c32).changed() {
        *c = Rgba::from_color32(c32);
    }
    ui.end_row();
}

impl WebApp {
    pub fn frame(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        self.take_loaded();
        self.run_tools(&ctx);
        self.handle_keys(&ctx);
        egui::Panel::top("bar").show(ui, |ui| self.top_bar(ui));
        egui::Panel::bottom("status").show(ui, |ui| self.status_bar(ui));
        egui::CentralPanel::default_margins().show(ui, |ui| match self.tab {
            Tab::Service => self.service_tab(ui),
            Tab::Songs => self.songs_tab(ui),
            Tab::Bible => self.bible_tab(ui),
            Tab::Settings => self.settings_tab(ui),
        });
        self.dialogs(&ctx);
        self.sync_output();
        self.save_changes();
        // tool calls and the output window's messages arrive between frames
        ctx.request_repaint_after(std::time::Duration::from_millis(150));
    }

    fn top_bar(&mut self, ui: &mut egui::Ui) {
        let sc = self.settings.shortcuts.clone();
        ui.horizontal_wrapped(|ui| {
            ui.strong("oh! Presenter");
            ui.separator();
            for (tab, key, action) in TABS {
                let r = ui.selectable_label(self.tab == tab, t(key)).on_hover_text(sc.describe(action));
                if r.clicked() {
                    self.tab = tab;
                }
            }
            ui.separator();
            let open = crate::js::output_open();
            let label = if open { t("output.close") } else { t("output.open") };
            if ui
                .button(format!("🖵 {label}"))
                .on_hover_text(format!("{}\n{}", t("output.hint_main"), sc.describe(Action::ToggleOutput)))
                .clicked()
            {
                self.toggle_output();
            }
            let blank = RichText::new(t("output.blank"));
            if ui
                .selectable_label(self.blank, if self.blank { blank.color(LIVE) } else { blank })
                .on_hover_text(sc.describe(Action::Blank))
                .clicked()
            {
                self.blank = !self.blank;
            }
            let freeze = RichText::new(t("output.freeze"));
            let frozen = self.frozen.is_some();
            if ui
                .selectable_label(frozen, if frozen { freeze.color(LIVE) } else { freeze })
                .on_hover_text(format!("{}\n{}", t("output.freeze_hint"), sc.describe(Action::Freeze)))
                .clicked()
            {
                self.set_freeze(!frozen);
            }
            if ui.button(t("alert.button")).on_hover_text(sc.describe(Action::Alert)).clicked() {
                self.handle_actions(vec![Action::Alert]);
            }
            ui.checkbox(&mut self.settings.live_follows_selection, t("service.live_mode")).on_hover_text(format!(
                "{}\n{}",
                t("service.live_mode_hint"),
                sc.describe(Action::ToggleLive)
            ));
        });
    }

    fn status_bar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal_wrapped(|ui| {
            ui.label(if self.settings.live_follows_selection { t("status.live_on") } else { t("status.live_off") });
            if self.blank {
                ui.separator();
                ui.colored_label(LIVE, t("status.blank"));
            }
            if self.frozen.is_some() {
                ui.separator();
                ui.colored_label(LIVE, t("status.frozen"));
            }
            if let Some((a, _)) = &self.alert {
                ui.separator();
                ui.colored_label(LIVE, tf("status.alert", &[("text", &a.text)]));
            }
            ui.separator();
            ui.label(if self.tools > 0 { tf("status.webmcp", &[("n", &self.tools.to_string())]) } else { t("status.no_webmcp") })
                .on_hover_text(t("status.webmcp_hint"));
            if let Some((text, since)) = &self.notice {
                if crate::js::now_ms() - since < 6000.0 {
                    ui.separator();
                    ui.label(text);
                } else {
                    self.notice = None;
                }
            }
        });
    }

    // -----------------------------------------------------------------------
    // Service

    fn service_tab(&mut self, ui: &mut egui::Ui) {
        egui::Panel::left("items").resizable(true).default_size(260.0).show(ui, |ui| self.item_list(ui));
        egui::Panel::right("live").resizable(true).default_size(340.0).show(ui, |ui| self.live_panel(ui));
        egui::CentralPanel::default_margins().show(ui, |ui| self.slides_grid(ui));
    }

    fn item_list(&mut self, ui: &mut egui::Ui) {
        let sc = self.settings.shortcuts.clone();
        ui.horizontal_wrapped(|ui| {
            ui.heading(t("service.title"));
            if !self.service_name.is_empty() {
                ui.weak(&self.service_name);
            }
        });
        ui.horizontal_wrapped(|ui| {
            if ui.button(t("service.new")).clicked() {
                self.dialog = Some(Dialog::NewService);
            }
            if ui.button(t("service.open")).on_hover_text(t("service.open_hint")).clicked() {
                self.choose_file(Purpose::Service);
            }
            if ui.button(t("service.save")).on_hover_text(t("service.save_hint")).clicked() {
                self.save_service_file();
            }
        });
        ui.horizontal_wrapped(|ui| {
            ui.menu_button(format!("➕ {}", t("service.add_song")), |ui| {
                egui::ScrollArea::vertical().max_height(360.0).show(ui, |ui| {
                    if self.songs.is_empty() {
                        ui.weak(t("songs.library_empty"));
                    }
                    let mut add = None;
                    for s in &self.songs {
                        if ui.button(&s.title).clicked() {
                            add = Some(s.clone());
                        }
                    }
                    if let Some(s) = add {
                        self.add_to_service(ItemKind::Song(s));
                        ui.close();
                    }
                });
            });
            if ui.button(format!("➕ {}", t("service.add_text"))).clicked() {
                self.dialog = Some(Dialog::AddText { title: String::new(), text: String::new() });
            }
            if ui.button(format!("➕ {}", t("service.add_image"))).clicked() {
                self.choose_file(Purpose::Image);
            }
        });
        ui.separator();
        if let Some(q) = self.quick.clone() {
            ui.horizontal(|ui| {
                let sel = self.sel_item == Some(ItemRef::Quick);
                if ui.selectable_label(sel, format!("⚡ {}", q.title())).on_hover_text(t("service.quick_hint")).clicked() {
                    self.select(ItemRef::Quick, 0);
                }
                let hint = format!("{}\n{}", t("service.add_quick"), sc.describe(Action::AddToService));
                if ui.small_button("➕").on_hover_text(hint).clicked() {
                    let _ = self.add_selected_to_service();
                }
            });
            ui.separator();
        }
        let mut clicked = None;
        egui::ScrollArea::vertical().id_salt("item_list").auto_shrink([false, false]).max_height(ui.available_height() - 40.0).show(
            ui,
            |ui| {
                if self.service.items.is_empty() {
                    ui.weak(t("service.empty"));
                }
                // full width, text on the left
                ui.set_width(ui.available_width());
                ui.with_layout(egui::Layout::top_down_justified(egui::Align::LEFT), |ui| {
                    for (i, it) in self.service.items.iter().enumerate() {
                        let sel = self.sel_item == Some(ItemRef::Service(i));
                        let live = matches!(self.live_pos, Some((ItemRef::Service(l), _)) if l == i);
                        let mut text = RichText::new(format!("{} {}", it.icon(), it.title()));
                        if live {
                            text = text.color(LIVE);
                        }
                        if ui.selectable_label(sel, text).clicked() {
                            clicked = Some(i);
                        }
                    }
                });
            },
        );
        if let Some(i) = clicked {
            self.select(ItemRef::Service(i), 0);
        }
        ui.horizontal_wrapped(|ui| {
            let any = matches!(self.sel_item, Some(ItemRef::Service(_)));
            ui.add_enabled_ui(any, |ui| {
                if ui.small_button("⬆").on_hover_text(t("service.move_up")).clicked() {
                    self.move_item(true);
                }
                if ui.small_button("⬇").on_hover_text(t("service.move_down")).clicked() {
                    self.move_item(false);
                }
                if ui.small_button(t("service.rename")).on_hover_text(sc.describe(Action::RenameItem)).clicked() {
                    self.handle_actions(vec![Action::RenameItem]);
                }
                if ui.small_button(format!("✖ {}", t("service.remove"))).on_hover_text(sc.describe(Action::RemoveItem)).clicked() {
                    self.remove_item();
                }
            });
        });
    }

    fn slides_grid(&mut self, ui: &mut egui::Ui) {
        let Some(r) = self.sel_item else {
            ui.weak(t("service.select_item"));
            return;
        };
        let slides = self.slides_of(r);
        if let Some(item) = self.item(r) {
            ui.heading(item.title());
        }
        let mut select = None;
        let mut live = None;
        egui::ScrollArea::vertical().id_salt("slides").auto_shrink([false, false]).show(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                for (i, s) in slides.iter().enumerate() {
                    ui.vertical(|ui| {
                        ui.set_width(THUMB_W);
                        let is_live = self.live_pos == Some((r, i));
                        let resp = thumb(ui, self, &s.scene, THUMB_W, i == self.sel_slide, is_live);
                        if resp.clicked() {
                            select = Some(i);
                        }
                        if resp.double_clicked() {
                            live = Some(i);
                        }
                        let label = if s.label == (i + 1).to_string() { s.label.clone() } else { format!("{}  {}", i + 1, s.label) };
                        ui.label(RichText::new(label).small());
                    });
                }
            });
        });
        if let Some(i) = select {
            self.select(r, i);
        }
        if let Some(i) = live {
            self.sel_slide = i;
            self.go_live();
        }
    }

    fn live_panel(&mut self, ui: &mut egui::Ui) {
        let sc = self.settings.shortcuts.clone();
        ui.heading(t("service.live"));
        let w = ui.available_width().min(420.0);
        let shown = self.shown();
        let (rect, _) = ui.allocate_exact_size(Vec2::new(w, w * CANVAS_H / CANVAS_W), Sense::hover());
        render::draw_scene(ui.painter(), rect, &shown, &mut self.tex);
        if let Some((alert, since)) = self.alert.clone() {
            let elapsed = ((crate::js::now_ms() - since) / 1000.0) as f32;
            if render::draw_alert(ui.painter(), rect, &alert, elapsed) {
                ui.ctx().request_repaint();
            } else {
                self.alert = None;
            }
        }
        ui.painter().rect_stroke(rect, 2.0, egui::Stroke::new(2.0, LIVE), egui::StrokeKind::Outside);
        ui.add_space(6.0);
        ui.horizontal_wrapped(|ui| {
            if ui.button(format!("▶ {}", t("service.go_live"))).on_hover_text(sc.describe(Action::GoLive)).clicked() {
                self.go_live();
            }
            if ui.button("⏮").on_hover_text(sc.describe(Action::PrevSlide)).clicked() {
                self.step_slide(false);
            }
            if ui.button("⏭").on_hover_text(sc.describe(Action::NextSlide)).clicked() {
                self.step_slide(true);
            }
        });
        ui.separator();
        ui.weak(t("service.keys_hint"));
    }

    // -----------------------------------------------------------------------
    // Songs

    fn songs_tab(&mut self, ui: &mut egui::Ui) {
        egui::Panel::left("library").resizable(true).default_size(260.0).show(ui, |ui| self.song_library(ui));
        egui::CentralPanel::default_margins().show(ui, |ui| self.song_editor(ui));
    }

    fn song_library(&mut self, ui: &mut egui::Ui) {
        ui.heading(t("songs.library"));
        ui.horizontal_wrapped(|ui| {
            if ui.button(format!("➕ {}", t("songs.new"))).clicked() {
                self.song = Song::new();
            }
            if ui.button(t("songs.import")).on_hover_text(t("songs.import_hint")).clicked() {
                self.choose_file(Purpose::Song);
            }
        });
        let r = ui.add(egui::TextEdit::singleline(&mut self.song_search).hint_text(t("songs.search")).desired_width(f32::INFINITY));
        if self.focus_song_search {
            r.request_focus();
            self.focus_song_search = false;
        }
        let q = crate::storage::fold(&self.song_search);
        let mut open = None;
        let mut add = None;
        egui::ScrollArea::vertical().id_salt("songs").auto_shrink([false, false]).show(ui, |ui| {
            ui.with_layout(egui::Layout::top_down_justified(egui::Align::LEFT), |ui| {
                let mut shown = 0;
                for s in &self.songs {
                    if !q.is_empty() && !crate::storage::fold(&s.title).contains(&q) && !s.text_matches(&q) {
                        continue;
                    }
                    shown += 1;
                    let r = ui.selectable_label(s.id == self.song.id, &s.title);
                    if r.clicked() {
                        open = Some(s.clone());
                    }
                    if r.double_clicked() {
                        add = Some(s.clone());
                    }
                    r.on_hover_text(t("songs.double_click_adds"));
                }
                if shown == 0 {
                    ui.weak(if self.songs.is_empty() { t("songs.library_empty") } else { t("songs.nothing_found") });
                }
            });
        });
        if let Some(s) = open {
            self.song = s;
        }
        if let Some(s) = add {
            self.add_to_service(ItemKind::Song(s));
        }
    }

    fn song_editor(&mut self, ui: &mut egui::Ui) {
        let sc = self.settings.shortcuts.clone();
        ui.horizontal_wrapped(|ui| {
            let save = if self.song_changed() { format!("💾 {} *", t("songs.save")) } else { format!("💾 {}", t("songs.save")) };
            if ui.button(save).clicked() {
                self.save_song();
            }
            if ui.button(format!("➕ {}", t("songs.add_to_service"))).on_hover_text(sc.describe(Action::AddToService)).clicked() {
                let r = self.add_selected_to_service();
                if let Err(e) = r {
                    self.notify(e);
                }
            }
            if ui.button(t("songs.export")).on_hover_text(t("songs.export_hint")).clicked() {
                let xml = crate::song_formats::song_to_openlyrics(&self.song);
                let name = format!("{}.xml", if self.song.title.is_empty() { "song" } else { &self.song.title });
                crate::js::save_file(&name, xml.as_bytes(), "application/xml");
            }
            if self.song_in_library() && ui.button(format!("✖ {}", t("songs.delete"))).clicked() {
                self.dialog = Some(Dialog::DeleteSong(self.song.id.clone()));
            }
        });
        ui.weak(if self.song_in_library() { t("songs.in_library") } else { t("songs.not_in_library") });
        ui.separator();
        egui::ScrollArea::vertical().id_salt("song_editor").auto_shrink([false, false]).show(ui, |ui| {
            egui::Grid::new("song_fields").num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
                ui.label(t("songs.title"));
                ui.add(egui::TextEdit::singleline(&mut self.song.title).desired_width(400.0));
                ui.end_row();
                ui.label(t("songs.author"));
                ui.add(egui::TextEdit::singleline(&mut self.song.author).desired_width(400.0));
                ui.end_row();
                ui.label(t("songs.copyright"));
                ui.add(egui::TextEdit::singleline(&mut self.song.copyright).desired_width(400.0));
                ui.end_row();
                ui.label(t("songs.order"));
                ui.add(egui::TextEdit::singleline(&mut self.song.order).hint_text("V1 C V2 C B C").desired_width(400.0))
                    .on_hover_text(t("songs.order_hint"));
                ui.end_row();
            });
            ui.separator();
            let mut remove = None;
            let mut swap = None;
            let n = self.song.parts.len();
            for (i, part) in self.song.parts.iter_mut().enumerate() {
                ui.push_id(i, |ui| {
                    ui.horizontal(|ui| {
                        egui::ComboBox::from_id_salt("tag").selected_text(t(&format!("mark.{}", part.tag))).width(130.0).show_ui(
                            ui,
                            |ui| {
                                for (code, _) in crate::app::MARKS {
                                    ui.selectable_value(&mut part.tag, code.to_string(), t(&format!("mark.{code}")));
                                }
                            },
                        );
                        ui.add(egui::DragValue::new(&mut part.number).range(0..=99)).on_hover_text(t("songs.part_number"));
                        ui.weak(part.code());
                        if ui.add_enabled(i > 0, egui::Button::new("⬆").small()).clicked() {
                            swap = Some((i, i - 1));
                        }
                        if ui.add_enabled(i + 1 < n, egui::Button::new("⬇").small()).clicked() {
                            swap = Some((i, i + 1));
                        }
                        if ui.small_button("✖").on_hover_text(t("songs.remove_part")).clicked() {
                            remove = Some(i);
                        }
                    });
                    ui.add(egui::TextEdit::multiline(&mut part.text).desired_rows(4).desired_width(f32::INFINITY))
                        .on_hover_text(t("songs.part_hint"));
                    ui.add_space(6.0);
                });
            }
            if let Some(i) = remove {
                self.song.parts.remove(i);
            }
            if let Some((a, b)) = swap {
                self.song.parts.swap(a, b);
            }
            ui.horizontal_wrapped(|ui| {
                for (code, _) in crate::app::MARKS {
                    if ui.button(format!("➕ {}", t(&format!("mark.{code}")))).clicked() {
                        let number = self.song.next_number(code);
                        let number = if *code != "V" && number == 1 { 0 } else { number };
                        self.song.parts.push(SongPart { tag: code.to_string(), number, text: String::new(), translations: Vec::new() });
                    }
                }
            });
            ui.separator();
            ui.collapsing(t("songs.paste"), |ui| {
                ui.weak(t("songs.paste_hint"));
                let id = egui::Id::unique("paste_lyrics");
                let mut text: String = ui.data_mut(|d| d.get_temp(id).unwrap_or_default());
                ui.add(egui::TextEdit::multiline(&mut text).desired_rows(8).desired_width(f32::INFINITY));
                if ui.button(t("songs.paste_apply")).clicked() && !text.trim().is_empty() {
                    let parsed = crate::song_formats::song_from_text(&text, &self.song.title);
                    self.song.parts = parsed.parts;
                    text.clear();
                }
                ui.data_mut(|d| d.insert_temp(id, text));
            });
        });
    }

    // -----------------------------------------------------------------------
    // Bible

    fn bible_tab(&mut self, ui: &mut egui::Ui) {
        let sc = self.settings.shortcuts.clone();
        ui.horizontal_wrapped(|ui| {
            ui.heading(t("bible.title"));
            let current = self
                .bible
                .current
                .as_ref()
                .map(|b| format!("{} ({})", b.meta.name, b.meta.abbrev))
                .unwrap_or_else(|| if self.bible.loading.is_some() { t("bible.loading") } else { t("bible.none") });
            let mut choose = None;
            egui::ComboBox::from_id_salt("bible_select").selected_text(current).width(260.0).show_ui(ui, |ui| {
                for m in &self.bibles {
                    if ui
                        .selectable_label(self.settings.bible.as_deref() == Some(m.id.as_str()), format!("{} ({})", m.name, m.abbrev))
                        .clicked()
                    {
                        choose = Some(m.id.clone());
                    }
                }
            });
            if let Some(id) = choose {
                self.load_bible(&id);
            }
            if ui.button(t("bible.import")).on_hover_text(t("bible.import_hint")).clicked() {
                self.choose_file(Purpose::Bible);
            }
            if let Some(id) = self.settings.bible.clone()
                && self.bibles.iter().any(|m| m.id == id)
                && ui.button(format!("✖ {}", t("bible.delete"))).clicked()
            {
                self.dialog = Some(Dialog::DeleteBible(id));
            }
        });
        if self.bibles.is_empty() {
            ui.weak(t("bible.no_bibles"));
        }
        ui.horizontal(|ui| {
            let r = ui.add(egui::TextEdit::singleline(&mut self.bible.query).hint_text(t("bible.search_hint")).desired_width(420.0));
            if self.bible.focus_search {
                r.request_focus();
                self.bible.focus_search = false;
            }
            let enter = r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            if ui.button(format!("🔍 {}", t("bible.search"))).clicked() || enter {
                self.run_bible_search();
            }
        });
        ui.weak(&self.bible.info);
        ui.horizontal_wrapped(|ui| {
            let any = !self.bible.results.is_empty();
            ui.add_enabled_ui(any, |ui| {
                if ui.button(format!("▶ {}", t("bible.show_live"))).on_hover_text(sc.describe(Action::GoLive)).clicked() {
                    self.bible_show_live();
                }
                if ui.button(format!("➕ {}", t("bible.add_to_service"))).on_hover_text(sc.describe(Action::AddToService)).clicked()
                    && let Err(e) = self.add_selected_to_service()
                {
                    self.notify(e);
                }
                if ui.button(t("bible.select_all")).clicked() {
                    self.bible.selected.iter_mut().for_each(|s| *s = true);
                }
                if ui.button(t("bible.select_none")).clicked() {
                    self.bible.selected.iter_mut().for_each(|s| *s = false);
                }
            });
        });
        ui.separator();
        let lang = crate::i18n::current_language();
        let sep = if lang == "cs" { "," } else { ":" };
        egui::Panel::right("bible_preview").resizable(true).default_size(340.0).show(ui, |ui| {
            ui.heading(t("service.live"));
            let w = ui.available_width().min(420.0);
            let shown = self.shown();
            let (rect, _) = ui.allocate_exact_size(Vec2::new(w, w * CANVAS_H / CANVAS_W), Sense::hover());
            render::draw_scene(ui.painter(), rect, &shown, &mut self.tex);
            ui.painter().rect_stroke(rect, 2.0, egui::Stroke::new(2.0, LIVE), egui::StrokeKind::Outside);
        });
        egui::ScrollArea::vertical().id_salt("bible_results").auto_shrink([false, false]).show(ui, |ui| {
            let Some(bible) = self.bible.current.as_ref() else { return };
            for (i, h) in self.bible.results.iter().enumerate() {
                let Some((_, c, v)) = bible.verse(h) else { continue };
                ui.horizontal_wrapped(|ui| {
                    if let Some(sel) = self.bible.selected.get_mut(i) {
                        ui.checkbox(sel, "");
                    }
                    ui.strong(format!("{} {}{sep}{}", bible.book_name(h.book, &lang), c.number, v.number));
                    ui.label(&v.text);
                });
            }
        });
    }

    // -----------------------------------------------------------------------
    // Settings

    fn settings_tab(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        egui::ScrollArea::vertical().id_salt("settings").auto_shrink([false, false]).show(ui, |ui| {
            ui.heading(t("settings.general"));
            let mut s = self.settings.clone();
            egui::Grid::new("general").num_columns(2).spacing([12.0, 8.0]).show(ui, |ui| {
                ui.label(t("settings.language"));
                let shown = crate::i18n::LANGUAGES
                    .iter()
                    .find(|(c, _)| *c == s.language)
                    .map(|(_, n)| n.to_string())
                    .unwrap_or_else(|| t("settings.browser_language"));
                egui::ComboBox::from_id_salt("lang").selected_text(shown).show_ui(ui, |ui| {
                    ui.selectable_value(&mut s.language, String::new(), t("settings.browser_language"));
                    for (code, name) in crate::i18n::LANGUAGES {
                        ui.selectable_value(&mut s.language, code.to_string(), *name);
                    }
                });
                ui.end_row();
                ui.label(t("settings.theme"));
                ui.horizontal(|ui| {
                    ui.selectable_value(&mut s.dark_mode, true, t("settings.dark"));
                    ui.selectable_value(&mut s.dark_mode, false, t("settings.light"));
                });
                ui.end_row();
                ui.label(t("service.live_mode"));
                ui.checkbox(&mut s.live_follows_selection, t("service.live_mode_hint"));
                ui.end_row();
                ui.label(t("settings.number_keys"));
                ui.checkbox(&mut s.number_keys_jump_to_verse, t("settings.number_keys_hint"));
                ui.end_row();
            });
            ui.separator();
            ui.heading(t("settings.display"));
            egui::Grid::new("display").num_columns(2).spacing([12.0, 8.0]).show(ui, |ui| {
                let st = &mut s.text_style;
                color_row(ui, &t("settings.text_color"), &mut st.color);
                color_row(ui, &t("settings.bg_color"), &mut st.background);
                ui.label(t("settings.max_font"));
                ui.add(egui::Slider::new(&mut st.max_font_size, 16.0..=160.0));
                ui.end_row();
                ui.label(t("settings.text_position"));
                ui.horizontal_wrapped(|ui| {
                    for (v, key) in
                        [(VAlign::Top, "settings.top"), (VAlign::Middle, "settings.middle"), (VAlign::Bottom, "settings.bottom")]
                    {
                        ui.selectable_value(&mut st.text_valign, v, t(key));
                    }
                    ui.separator();
                    for (v, key) in
                        [(HAlign::Left, "settings.left"), (HAlign::Center, "settings.center"), (HAlign::Right, "settings.right")]
                    {
                        ui.selectable_value(&mut st.text_halign, v, t(key));
                    }
                });
                ui.end_row();
                ui.label(t("settings.margin"));
                ui.add(egui::Slider::new(&mut st.margin, 0.0..=300.0));
                ui.end_row();
                ui.label(t("settings.footer"));
                ui.vertical(|ui| {
                    ui.checkbox(&mut s.show_bible_reference, t("settings.show_bible_reference"))
                        .on_hover_text(t("settings.show_bible_reference_hint"));
                    ui.checkbox(&mut s.text_style.show_footer, t("settings.show_song_footer"))
                        .on_hover_text(t("settings.show_song_footer_hint"));
                });
                ui.end_row();
                let any = s.show_bible_reference || s.text_style.show_footer;
                let st = &mut s.text_style;
                ui.label(t("settings.footer_size"));
                ui.add_enabled(any, egui::Slider::new(&mut st.footer_size, 8.0..=96.0));
                ui.end_row();
                ui.label(t("settings.footer_position"));
                ui.add_enabled_ui(any, |ui| {
                    ui.horizontal_wrapped(|ui| {
                        for (v, key) in [
                            (FooterPosition::BelowText, "settings.footer_below"),
                            (FooterPosition::Top, "settings.footer_top"),
                            (FooterPosition::Bottom, "settings.footer_bottom"),
                        ] {
                            ui.selectable_value(&mut st.footer_position, v, t(key));
                        }
                    });
                });
                ui.end_row();
                ui.label(t("settings.bible_reference"));
                ui.add_enabled_ui(s.show_bible_reference, |ui| {
                    ui.horizontal_wrapped(|ui| {
                        let r = &mut s.bible_reference;
                        ui.checkbox(&mut r.book, t("settings.ref_book"));
                        ui.checkbox(&mut r.chapter, t("settings.ref_chapter"));
                        ui.checkbox(&mut r.verse, t("settings.ref_verse"));
                        ui.checkbox(&mut r.translation, t("settings.ref_translation"));
                    });
                });
                ui.end_row();
                ui.label(t("settings.shadow"));
                ui.checkbox(&mut s.text_style.shadow, t("settings.show_shadow"));
                ui.end_row();
                ui.label(t("settings.verses_per_slide"));
                ui.add(egui::DragValue::new(&mut s.verses_per_slide).range(1..=10));
                ui.end_row();
            });
            ui.separator();
            ui.heading(t("settings.alerts"));
            egui::Grid::new("alerts").num_columns(2).spacing([12.0, 8.0]).show(ui, |ui| {
                let a = &mut s.alert_style;
                ui.label(t("settings.alert_size"));
                ui.add(egui::Slider::new(&mut a.font_size, 16.0..=120.0));
                ui.end_row();
                ui.label(t("settings.alert_speed"));
                ui.add(egui::Slider::new(&mut a.speed, 40.0..=800.0));
                ui.end_row();
                ui.label(t("settings.alert_timeout"));
                ui.add(egui::Slider::new(&mut a.timeout, 0.0..=120.0)).on_hover_text(t("settings.alert_timeout_hint"));
                ui.end_row();
                color_row(ui, &t("settings.alert_color"), &mut a.color);
                color_row(ui, &t("settings.alert_background"), &mut a.background);
            });
            self.apply_settings(s, &ctx);
            ui.separator();
            self.settings_shortcuts(ui);
            ui.separator();
            ui.heading(t("settings.data"));
            ui.label(t("settings.data_hint"));
            ui.label(t("settings.webmcp_hint"));
        });
    }

    fn settings_shortcuts(&mut self, ui: &mut egui::Ui) {
        if let Some(a) = self.capture {
            if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                self.capture = None;
            } else if let Some(b) = crate::shortcuts::capture_key(ui.ctx()) {
                if b.reserved() {
                    self.notify(tf("settings.reserved_key", &[("key", &b.to_string())]));
                } else {
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
                self.capture = None;
                ui.ctx().input_mut(|i| i.events.clear());
            }
        }
        ui.horizontal(|ui| {
            ui.heading(t("settings.shortcuts"));
            if ui.button(t("settings.reset_shortcuts")).clicked() {
                self.settings.shortcuts = Default::default();
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
                            text = text.color(Color32::from_rgb(230, 120, 60));
                        }
                        if ui
                            .small_button(text)
                            .on_hover_text(if conflict { t("settings.conflict_short") } else { t("settings.unbind") })
                            .clicked()
                        {
                            let mut l = list.clone();
                            l.remove(j);
                            self.settings.shortcuts.map.insert(*a, l);
                        }
                    }
                    let waiting = self.capture == Some(*a);
                    if ui
                        .small_button(if waiting { t("settings.press_key") } else { "➕".into() })
                        .on_hover_text(t("settings.add_binding"))
                        .clicked()
                    {
                        self.capture = Some(*a);
                    }
                    if self.settings.shortcuts.bindings(*a) != a.defaults()
                        && ui.small_button("↺").on_hover_text(t("settings.reset_one")).clicked()
                    {
                        self.settings.shortcuts.map.insert(*a, a.defaults());
                    }
                });
                ui.end_row();
            }
            // the fixed keys of the slides
            ui.label(t("settings.mark_keys"));
            let marks: Vec<String> =
                crate::app::MARKS.iter().map(|(c, k)| format!("{} {}", KeyBinding::plain(*k), t(&format!("mark.{c}")))).collect();
            ui.label(format!("{} · 1–9 {}", marks.join(" · "), t("settings.verse_numbers")));
            ui.end_row();
        });
    }

    // -----------------------------------------------------------------------
    // Dialogs

    fn dialogs(&mut self, ctx: &egui::Context) {
        let Some(mut dialog) = self.dialog.take() else { return };
        let mut close = false;
        egui::Modal::new(egui::Id::unique("dialog")).show(ctx, |ui| {
            ui.set_max_width(520.0);
            let enter = ui.input(|i| i.key_pressed(egui::Key::Enter) && !i.modifiers.shift);
            let escape = ui.input(|i| i.key_pressed(egui::Key::Escape));
            match &mut dialog {
                Dialog::Message(text) => {
                    ui.label(text.as_str());
                    if ui.button(t("dialog.ok")).clicked() || enter || escape {
                        close = true;
                    }
                }
                Dialog::Rename(name) => {
                    ui.heading(t("service.rename"));
                    let r = ui.text_edit_singleline(name);
                    r.request_focus();
                    ui.horizontal(|ui| {
                        if ui.button(t("dialog.ok")).clicked() || enter {
                            let n = name.clone();
                            self.rename_item(&n);
                            close = true;
                        }
                        if ui.button(t("dialog.cancel")).clicked() || escape {
                            close = true;
                        }
                    });
                }
                Dialog::Alert(text) => {
                    ui.heading(t("alert.title"));
                    let r = ui.add(egui::TextEdit::singleline(text).desired_width(460.0).hint_text(t("alert.hint")));
                    r.request_focus();
                    ui.horizontal(|ui| {
                        if ui.button(t("alert.start")).clicked() || (enter && !text.trim().is_empty()) {
                            let txt = text.clone();
                            self.start_alert(&txt);
                            close = true;
                        }
                        if self.alert.is_some() && ui.button(t("alert.stop")).clicked() {
                            self.alert = None;
                            close = true;
                        }
                        if ui.button(t("dialog.cancel")).clicked() || escape {
                            close = true;
                        }
                    });
                }
                Dialog::AddText { title, text } => {
                    ui.heading(t("service.add_text"));
                    ui.add(egui::TextEdit::singleline(title).hint_text(t("service.text_title")).desired_width(460.0));
                    ui.add(egui::TextEdit::multiline(text).hint_text(t("service.text_hint")).desired_rows(8).desired_width(460.0));
                    ui.horizontal(|ui| {
                        if ui.button(t("dialog.ok")).clicked() {
                            let title = if title.trim().is_empty() { t("service.new_text_title") } else { title.clone() };
                            self.add_to_service(ItemKind::Text { title, text: text.clone() });
                            close = true;
                        }
                        if ui.button(t("dialog.cancel")).clicked() || escape {
                            close = true;
                        }
                    });
                }
                Dialog::NewService => {
                    ui.label(t("service.new_confirm"));
                    ui.horizontal(|ui| {
                        if ui.button(t("service.new")).clicked() || enter {
                            self.new_service();
                            close = true;
                        }
                        if ui.button(t("dialog.cancel")).clicked() || escape {
                            close = true;
                        }
                    });
                }
                Dialog::DeleteSong(id) => {
                    ui.label(t("songs.delete_confirm"));
                    ui.horizontal(|ui| {
                        if ui.button(t("songs.delete")).clicked() {
                            let id = id.clone();
                            self.songs.retain(|s| s.id != id);
                            close = true;
                        }
                        if ui.button(t("dialog.cancel")).clicked() || escape {
                            close = true;
                        }
                    });
                }
                Dialog::DeleteBible(id) => {
                    ui.label(t("bible.delete_confirm"));
                    ui.horizontal(|ui| {
                        if ui.button(t("bible.delete")).clicked() {
                            let id = id.clone();
                            self.delete_bible(&id);
                            close = true;
                        }
                        if ui.button(t("dialog.cancel")).clicked() || escape {
                            close = true;
                        }
                    });
                }
            }
        });
        if !close && self.dialog.is_none() {
            self.dialog = Some(dialog);
        }
    }
}
