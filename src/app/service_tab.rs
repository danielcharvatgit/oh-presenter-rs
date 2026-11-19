use super::{App, ItemRef, LIVE_COLOR, Then};
use crate::i18n::{t, tf};
use crate::model::*;
use crate::render;
use crate::shortcuts::Action;
use eframe::egui::{self, Color32, RichText, Sense, Stroke, StrokeKind, Vec2};

/// Height of a row in the service item list (room for a small slide preview).
const ITEM_ROW_H: f32 = 44.0;

impl App {
    pub fn slide_label(&self, s: &Slide) -> String {
        match &s.tag {
            Some(tag) => {
                let mut name = self.settings.mark_name(tag);
                if tag == "V" || s.number > 1 {
                    name.push_str(&format!(" {}", s.number.max(1)));
                }
                if let Some(i) = s.label.find(" (") {
                    name.push_str(&s.label[i..]);
                }
                name
            }
            None => s.label.clone(),
        }
    }

    pub fn service_tab(&mut self, ui: &mut egui::Ui) {
        egui::Panel::left("service_items").resizable(true).default_size(270.0).show(ui, |ui| self.service_list(ui));
        egui::Panel::right("service_live").resizable(true).default_size(380.0).show(ui, |ui| self.live_panel(ui));
        egui::CentralPanel::default_margins().show(ui, |ui| self.slides_panel(ui));
    }

    fn service_list(&mut self, ui: &mut egui::Ui) {
        let sc = self.settings.shortcuts.clone();
        ui.heading(t("service.title"));
        ui.horizontal_wrapped(|ui| {
            if ui.button(t("service.new")).on_hover_text(sc.describe(Action::NewService)).clicked() {
                self.request(Then::NewService);
            }
            if ui.button(t("service.open")).on_hover_text(sc.describe(Action::OpenService)).clicked() {
                self.request(Then::OpenService(None));
            }
            let save = if self.service_dirty { format!("💾 {} *", t("service.save")) } else { format!("💾 {}", t("service.save")) };
            if ui.button(save).on_hover_text(format!("{}\n{}", t("service.save_hint"), sc.describe(Action::Save))).clicked() {
                self.save_service();
            }
            if ui.button(t("service.save_as")).on_hover_text(sc.describe(Action::SaveAs)).clicked() {
                self.save_service_as();
            }
        });
        ui.separator();
        self.add_menu(ui);
        ui.separator();

        if let Some(q) = self.quick.clone() {
            ui.horizontal(|ui| {
                let sel = self.sel_item == Some(ItemRef::Quick);
                if ui.selectable_label(sel, format!("⚡ {}", q.title())).on_hover_text(t("service.quick_hint")).clicked() {
                    self.select(ItemRef::Quick, 0);
                }
                if ui.small_button(super::plus_icon()).on_hover_text(t("service.add_quick")).clicked() {
                    self.add_to_service(q.kind.clone());
                    self.quick = None;
                    if self.sel_item == Some(ItemRef::Quick) {
                        self.sel_item = None;
                    }
                }
            });
            ui.separator();
        }

        let mut action: Option<(usize, &str)> = None;
        // the selected item shows its selected slide, the others their first one
        let thumbs: Vec<Option<Scene>> = (0..self.service.items.len())
            .map(|i| {
                let slide = if self.sel_item == Some(ItemRef::Service(i)) { self.sel_slide } else { 0 };
                self.slides_of(ItemRef::Service(i)).into_iter().nth(slide).map(|s| s.scene)
            })
            .collect();
        egui::ScrollArea::vertical().auto_shrink([false, true]).max_height(ui.available_height() - 40.0).show(ui, |ui| {
            if self.service.items.is_empty() {
                ui.weak(t("service.empty"));
            }
            for (i, item) in self.service.items.iter().enumerate() {
                let sel = self.sel_item == Some(ItemRef::Service(i));
                let live = matches!(self.live_pos, Some((ItemRef::Service(l), _)) if l == i);
                let (rect, r) = ui.allocate_exact_size(Vec2::new(ui.available_width(), ITEM_ROW_H), Sense::click());
                let visuals = ui.style().interact_selectable(&r, sel);
                let painter = ui.painter_at(rect);
                if sel || r.hovered() {
                    painter.rect_filled(rect, visuals.corner_radius, visuals.weak_bg_fill);
                }
                let thumb_h = ITEM_ROW_H - 6.0;
                let thumb = egui::Rect::from_min_size(rect.min + Vec2::new(3.0, 3.0), Vec2::new(thumb_h * CANVAS_W / CANVAS_H, thumb_h));
                match &thumbs[i] {
                    Some(scene) => render::draw_scene(&painter, thumb, scene, &mut self.textures),
                    None => {
                        painter.rect_filled(thumb, 0.0, Color32::from_gray(30));
                    }
                }
                let border = if live { Stroke::new(2.0, LIVE_COLOR) } else { Stroke::new(1.0, ui.visuals().weak_text_color()) };
                painter.rect_stroke(thumb, 0.0, border, StrokeKind::Outside);
                let text =
                    RichText::new(format!("{} {}", item.icon(), item.title())).color(if live { LIVE_COLOR } else { visuals.text_color() });
                let text_rect = egui::Rect::from_min_max(egui::pos2(thumb.right() + 8.0, rect.top()), rect.right_bottom());
                let galley = egui::WidgetText::from(text).into_galley(
                    ui,
                    Some(egui::TextWrapMode::Truncate),
                    text_rect.width().max(1.0),
                    egui::TextStyle::Button,
                );
                painter.galley(egui::pos2(text_rect.left(), text_rect.center().y - galley.size().y / 2.0), galley, visuals.text_color());
                let r = r.on_hover_text(item.title());
                if r.clicked() {
                    self.sel_item = Some(ItemRef::Service(i));
                    self.sel_slide = 0;
                }
                if r.double_clicked() {
                    action = Some((i, "live"));
                }
                r.context_menu(|ui| {
                    for (key, act) in [
                        ("service.move_up", "up"),
                        ("service.move_down", "down"),
                        ("service.edit", "edit"),
                        ("service.duplicate", "dup"),
                        ("service.remove", "remove"),
                    ] {
                        if ui.button(t(key)).clicked() {
                            action = Some((i, act));
                            ui.close();
                        }
                    }
                });
            }
        });
        ui.separator();
        ui.horizontal(|ui| {
            let sel = match self.sel_item {
                Some(ItemRef::Service(i)) => Some(i),
                _ => None,
            };
            ui.add_enabled_ui(sel.is_some(), |ui| {
                let i = sel.unwrap_or(0);
                if ui.button("⬆").on_hover_text(t("service.move_up")).clicked() {
                    action = Some((i, "up"));
                }
                if ui.button("⬇").on_hover_text(t("service.move_down")).clicked() {
                    action = Some((i, "down"));
                }
                if ui.button("✏").on_hover_text(t("service.edit")).clicked() {
                    action = Some((i, "edit"));
                }
                if ui.button("✖").on_hover_text(t("service.remove")).clicked() {
                    action = Some((i, "remove"));
                }
            });
        });
        if let Some((i, act)) = action {
            self.item_action(i, act);
        }
    }

    fn item_action(&mut self, i: usize, act: &str) {
        let n = self.service.items.len();
        if i >= n {
            return;
        }
        let fix_live = |app: &mut App, from: usize, to: usize| {
            if let Some((ItemRef::Service(l), s)) = app.live_pos {
                if l == from {
                    app.live_pos = Some((ItemRef::Service(to), s));
                } else if l == to {
                    app.live_pos = Some((ItemRef::Service(from), s));
                }
            }
        };
        match act {
            "up" if i > 0 => {
                self.service.items.swap(i, i - 1);
                fix_live(self, i, i - 1);
                self.sel_item = Some(ItemRef::Service(i - 1));
                self.service_dirty = true;
            }
            "down" if i + 1 < n => {
                self.service.items.swap(i, i + 1);
                fix_live(self, i, i + 1);
                self.sel_item = Some(ItemRef::Service(i + 1));
                self.service_dirty = true;
            }
            "dup" => {
                let mut copy = self.service.items[i].clone();
                copy.id = make_id();
                self.service.items.insert(i + 1, copy);
                self.service_dirty = true;
            }
            "remove" => {
                self.service.items.remove(i);
                self.service_dirty = true;
                if let Some((ItemRef::Service(l), s)) = self.live_pos {
                    self.live_pos = match l.cmp(&i) {
                        std::cmp::Ordering::Equal => None,
                        std::cmp::Ordering::Greater => Some((ItemRef::Service(l - 1), s)),
                        std::cmp::Ordering::Less => Some((ItemRef::Service(l), s)),
                    };
                }
                self.sel_item =
                    if self.service.items.is_empty() { None } else { Some(ItemRef::Service(i.min(self.service.items.len() - 1))) };
                self.sel_slide = 0;
            }
            "edit" => match self.service.items[i].kind.clone() {
                ItemKind::Song(s) => {
                    // edit the library version when there is one, so saving updates both
                    let song = self.lib.song(&s.id).cloned().unwrap_or(s);
                    self.request(Then::EditSong(song));
                }
                ItemKind::Design(d) => self.request(Then::EditDesign(d)),
                _ => {
                    self.sel_item = Some(ItemRef::Service(i));
                }
            },
            "live" => {
                self.sel_item = Some(ItemRef::Service(i));
                self.sel_slide = 0;
                self.go_live();
            }
            _ => {}
        }
    }

    fn add_menu(&mut self, ui: &mut egui::Ui) {
        ui.horizontal_wrapped(|ui| {
            ui.menu_button((super::plus_icon(), t("service.add_song")), |ui| {
                ui.set_min_width(280.0);
                let r = ui.add(egui::TextEdit::singleline(&mut self.add_search).hint_text(t("search")));
                r.request_focus();
                let q = crate::storage::fold(&self.add_search);
                let mut add = None;
                egui::ScrollArea::vertical().max_height(320.0).show(ui, |ui| {
                    for s in &self.lib.songs {
                        if !q.is_empty() && !crate::storage::fold(&s.title).contains(&q) {
                            continue;
                        }
                        if ui.button(&s.title).clicked() {
                            add = Some(s.clone());
                        }
                    }
                    if self.lib.songs.is_empty() {
                        ui.weak(t("songs.library_empty"));
                    }
                });
                if let Some(s) = add {
                    self.add_to_service(ItemKind::Song(s));
                    ui.close();
                }
            });
            ui.menu_button((super::plus_icon(), t("service.add_design")), |ui| {
                ui.set_min_width(240.0);
                let mut add = None;
                egui::ScrollArea::vertical().max_height(320.0).show(ui, |ui| {
                    for d in &self.lib.designs {
                        if ui.button(&d.name).clicked() {
                            add = Some(d.clone());
                        }
                    }
                    if self.lib.designs.is_empty() {
                        ui.weak(t("designer.library_empty"));
                    }
                });
                if let Some(d) = add {
                    self.add_to_service(ItemKind::Design(d));
                    ui.close();
                }
            });
            if ui.button((super::plus_icon(), t("service.add_text"))).clicked() {
                self.add_to_service(ItemKind::Text { title: t("service.new_text_title"), text: String::new() });
                if let Some(ItemRef::Service(i)) = self.sel_item {
                    self.sel_item = Some(ItemRef::Service(i + 1));
                } else {
                    self.sel_item = Some(ItemRef::Service(self.service.items.len() - 1));
                }
                self.sel_slide = 0;
            }
            if ui.button((super::plus_icon(), t("service.add_image"))).clicked() {
                self.import_image_slides();
            }
            if ui.button((super::plus_icon(), t("service.add_video"))).on_hover_text(t("video.add_hint")).clicked() {
                self.import_videos();
            }
        });
    }

    fn slides_panel(&mut self, ui: &mut egui::Ui) {
        let Some(item_ref) = self.sel_item else {
            ui.centered_and_justified(|ui| ui.weak(t("service.select_item")));
            return;
        };
        let Some(item) = self.item(item_ref).cloned() else {
            self.sel_item = None;
            return;
        };
        ui.horizontal(|ui| {
            ui.heading(format!("{} {}", item.icon(), item.title()));
        });

        // text items are edited right here
        if let (ItemKind::Text { .. }, ItemRef::Service(i)) = (&item.kind, item_ref) {
            let mut changed = false;
            if let ItemKind::Text { title, text } = &mut self.service.items[i].kind {
                ui.horizontal(|ui| {
                    ui.label(t("service.text_title"));
                    changed |= ui.text_edit_singleline(title).changed();
                });
                ui.label(t("service.text_hint"));
                changed |= ui.add(egui::TextEdit::multiline(text).desired_rows(5).desired_width(f32::INFINITY)).changed();
            }
            if changed {
                self.service_dirty = true;
            }
            ui.separator();
        }

        if matches!(item.kind, ItemKind::Video(_)) {
            self.video_item_options(ui, item_ref);
        }

        let slides = self.item_slides(&item);
        // jump buttons for marked parts
        let mut marks: Vec<(String, u32)> = Vec::new();
        for s in &slides {
            if let Some(tag) = &s.tag
                && !marks.contains(&(tag.clone(), s.number))
            {
                marks.push((tag.clone(), s.number));
            }
        }
        if !marks.is_empty() {
            ui.horizontal_wrapped(|ui| {
                ui.label(t("service.jump"));
                for (tag, number) in &marks {
                    let key = self.settings.mark(tag).and_then(|m| m.key).map(|k| k.to_string()).unwrap_or_default();
                    let code = SongPart { tag: tag.clone(), number: *number, text: String::new(), translations: Vec::new() }.code();
                    let hint = if tag == "V" && self.settings.number_keys_jump_to_verse && (1..=9).contains(number) {
                        format!("{} — {}", self.settings.mark_name(tag), number)
                    } else {
                        format!("{} — {}", self.settings.mark_name(tag), key)
                    };
                    if ui.button(code).on_hover_text(hint).clicked() {
                        self.jump_to_mark(tag, Some((*number).max(1)));
                    }
                }
            });
            ui.separator();
        }

        let card_w = 230.0;
        let thumb = Vec2::new(card_w, card_w * CANVAS_H / CANVAS_W);
        let live_slide = match self.live_pos {
            Some((r, s)) if r == item_ref => Some(s),
            _ => None,
        };
        let mut clicked = None;
        let mut double = None;
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = Vec2::splat(10.0);
                for (i, s) in slides.iter().enumerate() {
                    let (rect, resp) = ui.allocate_exact_size(thumb + Vec2::new(0.0, 22.0), Sense::click());
                    let painter = ui.painter_at(rect.expand(3.0));
                    let thumb_rect = egui::Rect::from_min_size(rect.min + Vec2::new(0.0, 22.0), thumb);
                    render::draw_scene(&painter, thumb_rect, &s.scene, &mut self.textures);
                    let label = format!("{}  {}", i + 1, self.slide_label(s));
                    painter.text(
                        rect.min + Vec2::new(2.0, 2.0),
                        egui::Align2::LEFT_TOP,
                        label,
                        egui::FontId::proportional(14.0),
                        ui.visuals().text_color(),
                    );
                    let selected = i == self.sel_slide && self.sel_item == Some(item_ref);
                    if Some(i) == live_slide {
                        painter.rect_stroke(thumb_rect.expand(1.5), 2.0, Stroke::new(3.0, LIVE_COLOR), StrokeKind::Outside);
                    } else if selected {
                        painter.rect_stroke(
                            thumb_rect.expand(1.5),
                            2.0,
                            Stroke::new(3.0, ui.visuals().selection.bg_fill),
                            StrokeKind::Outside,
                        );
                    } else if resp.hovered() {
                        painter.rect_stroke(thumb_rect, 2.0, Stroke::new(1.0, ui.visuals().text_color()), StrokeKind::Outside);
                    }
                    if selected && self.scroll_pending {
                        resp.scroll_to_me(None);
                        self.scroll_pending = false;
                    }
                    if resp.clicked() {
                        clicked = Some(i);
                    }
                    if resp.double_clicked() {
                        double = Some(i);
                    }
                }
            });
            if slides.is_empty() {
                ui.weak(t("service.no_slides"));
            }
        });
        if let Some(i) = clicked {
            self.select(item_ref, i);
        }
        if let Some(i) = double {
            self.sel_item = Some(item_ref);
            self.sel_slide = i;
            self.go_live();
        }

        if let ItemRef::Service(i) = item_ref {
            ui.separator();
            ui.collapsing(t("service.notes"), |ui| {
                if ui
                    .add(egui::TextEdit::multiline(&mut self.service.items[i].notes).desired_rows(2).desired_width(f32::INFINITY))
                    .changed()
                {
                    self.service_dirty = true;
                }
            });
        }
    }

    fn live_panel(&mut self, ui: &mut egui::Ui) {
        let sc = self.settings.shortcuts.clone();
        let w = ui.available_width();
        let size = Vec2::new(w, w * CANVAS_H / CANVAS_W);

        ui.horizontal(|ui| {
            ui.label(RichText::new(t("service.live")).strong().color(LIVE_COLOR));
            if let Some((item, slide)) = self.live_pos
                && let Some(it) = self.item(item)
            {
                ui.weak(format!("{} — {}", it.title(), slide + 1));
            }
        });
        let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
        let scene = if self.blank { Scene::Blank } else { self.live.clone() };
        render::draw_scene_live(ui.painter(), rect, &scene, &mut self.textures);
        self.video_controls(ui);
        ui.horizontal_wrapped(|ui| {
            if ui.button(format!("▶ {}", t("service.go_live"))).on_hover_text(sc.describe(Action::GoLive)).clicked() {
                self.go_live();
            }
            let mut live = self.settings.live_follows_selection;
            if ui
                .checkbox(&mut live, t("service.live_mode"))
                .on_hover_text(format!("{}\n{}", t("service.live_mode_hint"), sc.describe(Action::ToggleLive)))
                .changed()
            {
                self.settings.live_follows_selection = live;
                self.save_settings();
            }
        });
        ui.horizontal_wrapped(|ui| {
            if ui.button("⏮").on_hover_text(format!("{} ({})", t("action.PrevSlide"), sc.describe(Action::PrevSlide))).clicked() {
                self.step_slide(false);
            }
            if ui.button("⏭").on_hover_text(format!("{} ({})", t("action.NextSlide"), sc.describe(Action::NextSlide))).clicked() {
                self.step_slide(true);
            }
            if ui.selectable_label(self.blank, format!("⬛ {}", t("output.blank"))).on_hover_text(sc.describe(Action::Blank)).clicked() {
                self.toggle_blank();
            }
            if ui
                .selectable_label(self.output_open, format!("🖵 {}", t("output.window")))
                .on_hover_text(sc.describe(Action::ToggleOutput))
                .clicked()
            {
                self.output_open = !self.output_open;
            }
            if ui.button(format!("⛶ {}", t("output.fullscreen"))).on_hover_text(sc.describe(Action::OutputFullscreen)).clicked() {
                self.handle_actions(vec![Action::OutputFullscreen], &ui.ctx().clone());
            }
        });
        if let Some(s) = &self.server {
            ui.add_space(8.0);
            let clients = s.clients();
            ui.label(tf("status.clients", &[("n", &clients.len().to_string())]));
            for c in clients {
                ui.weak(format!("• {} ({})", c.display_name, c.addr));
            }
        }
    }
}
