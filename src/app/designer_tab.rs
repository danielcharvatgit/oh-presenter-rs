use super::{App, Dialog, FollowFocus, Then, WriteKind};
use crate::formats;
use crate::i18n::t;
use crate::model::*;
use crate::render;
use crate::shortcuts::Action;
use eframe::egui::{self, Color32, Key, Pos2, Rect, Sense, Stroke, StrokeKind, Vec2};
use std::time::{Duration, Instant};

#[derive(Clone, Copy, Debug)]
pub enum Drag {
    Move,
    /// Resizing object `usize` by its bottom-right handle.
    Resize(usize),
}

const HANDLE: f32 = 10.0;

fn color_edit(ui: &mut egui::Ui, label: &str, c: &mut Rgba) -> bool {
    ui.label(label);
    let mut c32 = c.to_color32();
    let r = ui.color_edit_button_srgba(&mut c32);
    if r.changed() {
        *c = Rgba::from_color32(c32);
    }
    ui.end_row();
    r.changed()
}

impl App {
    pub fn designer_tab(&mut self, ui: &mut egui::Ui) {
        egui::Panel::left("designer_library").resizable(true).default_size(230.0).show(ui, |ui| self.designer_library(ui));
        egui::Panel::right("designer_props").resizable(true).default_size(280.0).show(ui, |ui| {
            egui::ScrollArea::vertical().auto_shrink([false, false]).show_following_focus(ui, |ui| self.designer_properties(ui));
        });
        egui::CentralPanel::default_margins().show(ui, |ui| self.designer_canvas(ui));
    }

    fn designer_library(&mut self, ui: &mut egui::Ui) {
        let sc = self.settings.shortcuts.clone();
        ui.heading(t("designer.title"));
        ui.horizontal_wrapped(|ui| {
            if ui.button((super::plus_icon(), t("designer.new"))).clicked() {
                self.request(Then::NewDesign);
            }
            let save = if self.designer.dirty { format!("💾 {} *", t("designer.save")) } else { format!("💾 {}", t("designer.save")) };
            if ui.button(save).on_hover_text(format!("{}\n{}", t("designer.save_hint"), sc.describe(Action::Save))).clicked() {
                self.save_design(false, false);
            }
            if ui.button(t("designer.save_as_new")).on_hover_text(sc.describe(Action::SaveAs)).clicked() {
                self.save_design(true, false);
            }
            if ui.button((super::plus_icon(), t("designer.add_to_service"))).clicked() {
                self.add_to_service(ItemKind::Design(self.designer.design.clone()));
            }
            if ui.button(format!("▶ {}", t("designer.show_live"))).on_hover_text(t("designer.show_live_hint")).clicked() {
                self.set_live(Scene::Canvas(self.designer.design.doc.clone()), None);
            }
        });
        ui.horizontal_wrapped(|ui| {
            if ui.button(t("designer.export")).clicked() {
                let name = format!("{}.{}", crate::storage::safe_file_name(&self.designer.design.name), formats::DESIGN_EXT);
                self.export_to(WriteKind::Design(self.designer.design.clone()), &name, &t("file.design_filter"), formats::DESIGN_EXT);
            }
            if ui.button(t("import.designs")).clicked() {
                self.import_designs();
            }
            if self.designer.in_library && ui.button(format!("✖ {}", t("designer.delete"))).clicked() {
                self.dialog = Some(Dialog::DeleteDesign(self.designer.design.id.clone()));
            }
        });
        ui.weak(if self.designer.in_library { t("designer.in_library") } else { t("designer.not_in_library") });
        ui.separator();
        ui.label(egui::RichText::new(t("designer.add_object")).strong());
        ui.horizontal_wrapped(|ui| {
            if ui.button(format!("T {}", t("designer.text"))).clicked() {
                self.designer_add(ObjectKind::Text {
                    text: t("designer.text"),
                    font_size: 60.0,
                    color: Rgba::BLACK,
                    align: HAlign::Center,
                    font: FontStyle::Proportional,
                });
            }
            if ui.button(format!("▭ {}", t("designer.rect"))).clicked() {
                self.designer_add(ObjectKind::Rect {
                    fill: Rgba([200, 200, 200, 255]),
                    stroke: Rgba::RED,
                    stroke_width: 3.0,
                    corner_radius: 0.0,
                });
            }
            if ui.button(format!("◯ {}", t("designer.ellipse"))).clicked() {
                self.designer_add(ObjectKind::Ellipse { fill: Rgba::TRANSPARENT, stroke: Rgba::RED, stroke_width: 3.0 });
            }
            if ui.button(format!("🖼 {}", t("designer.image"))).clicked() {
                self.pick_image(super::files::Purpose::DesignerAddImage);
            }
        });
        ui.separator();
        ui.label(egui::RichText::new(t("designer.library")).strong());
        let r = ui.add(egui::TextEdit::singleline(&mut self.designer.search).hint_text(t("search")));
        if self.focus_search {
            r.request_focus();
            self.focus_search = false;
        }
        let q = crate::storage::fold(&self.designer.search);
        let mut open = None;
        egui::ScrollArea::vertical().auto_shrink([false, false]).show_following_focus(ui, |ui| {
            for d in &self.lib.designs {
                if !q.is_empty() && !crate::storage::fold(&d.name).contains(&q) {
                    continue;
                }
                let sel = d.id == self.designer.design.id;
                if ui.add_sized([ui.available_width(), 20.0], egui::Button::selectable(sel, &d.name)).clicked() && !sel {
                    open = Some(d.clone());
                }
            }
            if self.lib.designs.is_empty() {
                ui.weak(t("designer.library_empty"));
            }
        });
        if let Some(d) = open {
            self.request(Then::EditDesign(d));
        }
    }

    fn pick_image(&mut self, purpose: super::files::Purpose) {
        self.open_file_dialog(
            super::files::Spec::new(super::files::Mode::PickFile, t("designer.image")).filter(t("file.image_filter"), super::IMAGE_EXTS),
            purpose,
        );
    }

    /// Copies a picked picture into the media store.
    pub(super) fn store_image(&mut self, f: &std::path::Path) -> Option<String> {
        match crate::formats::read_image_file(f)
            .and_then(|(bytes, ext)| crate::storage::store_media_bytes(&bytes, &ext).map_err(|e| e.to_string()))
        {
            Ok(m) => Some(m),
            Err(e) => {
                self.error(format!("{}: {e}", f.display()));
                None
            }
        }
    }

    pub(super) fn designer_add(&mut self, kind: ObjectKind) {
        let (w, h) = match &kind {
            ObjectKind::Text { .. } => (600.0, 100.0),
            ObjectKind::Image { media, .. } => {
                let size =
                    image::image_dimensions(crate::storage::media_path(media)).map(|(w, h)| (w as f32, h as f32)).unwrap_or((400.0, 300.0));
                let s = (CANVAS_W * 0.8 / size.0).min(CANVAS_H * 0.8 / size.1).min(1.0);
                (size.0 * s, size.1 * s)
            }
            _ => (200.0, 200.0),
        };
        let obj = CanvasObject { id: make_id(), x: (CANVAS_W - w) / 2.0, y: (CANVAS_H - h) / 2.0, w, h, kind };
        let doc = &mut self.designer.design.doc;
        doc.objects.push(obj);
        self.designer.selected = vec![doc.objects.len() - 1];
    }

    fn designer_canvas(&mut self, ui: &mut egui::Ui) {
        let avail = ui.available_rect_before_wrap();
        let rect = render::fit_16_9(avail.shrink(8.0));
        let resp = ui.allocate_rect(rect, Sense::click_and_drag());
        let s = rect.width() / CANVAS_W;
        let to_canvas = |p: Pos2| Pos2::new((p.x - rect.min.x) / s, (p.y - rect.min.y) / s);

        // interaction
        let ds = &mut self.designer;
        let objects = &mut ds.design.doc.objects;
        let shift = ui.input(|i| i.modifiers.shift);
        let hit = |objects: &Vec<CanvasObject>, p: Pos2| objects.iter().rposition(|o| o.rect().expand(2.0 / s).contains(p));
        let handle_hit = |objects: &Vec<CanvasObject>, sel: &Vec<usize>, p: Pos2| {
            sel.iter().copied().find(|i| {
                objects.get(*i).map(|o| Rect::from_center_size(o.rect().max, Vec2::splat(HANDLE * 1.5 / s)).contains(p)).unwrap_or(false)
            })
        };
        if resp.drag_started()
            && let Some(p) = resp.interact_pointer_pos().map(to_canvas)
        {
            if let Some(i) = handle_hit(objects, &ds.selected, p) {
                ds.drag = Some(Drag::Resize(i));
            } else if let Some(i) = hit(objects, p) {
                if !ds.selected.contains(&i) {
                    if shift {
                        ds.selected.push(i);
                    } else {
                        ds.selected = vec![i];
                    }
                }
                ds.drag = Some(Drag::Move);
            } else {
                ds.drag = None;
            }
        }
        if resp.dragged() {
            let d = resp.drag_delta() / s;
            match ds.drag {
                Some(Drag::Move) => {
                    for i in &ds.selected {
                        if let Some(o) = objects.get_mut(*i) {
                            o.x += d.x;
                            o.y += d.y;
                        }
                    }
                }
                Some(Drag::Resize(i)) => {
                    if let Some(o) = objects.get_mut(i) {
                        let keep_aspect = matches!(o.kind, ObjectKind::Image { .. }) != shift;
                        if keep_aspect && o.w > 0.0 {
                            let aspect = o.h / o.w;
                            o.w = (o.w + d.x).max(5.0);
                            o.h = o.w * aspect;
                        } else {
                            o.w = (o.w + d.x).max(5.0);
                            o.h = (o.h + d.y).max(5.0);
                        }
                    }
                }
                None => {}
            }
        }
        if resp.drag_stopped() {
            ds.drag = None;
        }
        if resp.clicked()
            && let Some(p) = resp.interact_pointer_pos().map(to_canvas)
        {
            match hit(objects, p) {
                Some(i) if shift => {
                    if let Some(pos) = ds.selected.iter().position(|x| *x == i) {
                        ds.selected.remove(pos);
                    } else {
                        ds.selected.push(i);
                    }
                }
                Some(i) => ds.selected = vec![i],
                None => ds.selected.clear(),
            }
        }
        ds.selected.retain(|i| *i < objects.len());

        // keyboard: delete, select all, duplicate, nudge (when not typing)
        if !ui.ctx().egui_wants_keyboard_input() && self.dialog.is_none() {
            let (del, all, dup, nudge) = ui.input_mut(|i| {
                let del = i.consume_key(egui::Modifiers::NONE, Key::Delete) || i.consume_key(egui::Modifiers::NONE, Key::Backspace);
                let all = i.consume_key(egui::Modifiers::COMMAND, Key::A);
                let dup = i.consume_key(egui::Modifiers::COMMAND, Key::D);
                let step = if i.modifiers.shift { 10.0 } else { 1.0 };
                let mut n = Vec2::ZERO;
                for (k, v) in [
                    (Key::ArrowLeft, Vec2::new(-step, 0.0)),
                    (Key::ArrowRight, Vec2::new(step, 0.0)),
                    (Key::ArrowUp, Vec2::new(0.0, -step)),
                    (Key::ArrowDown, Vec2::new(0.0, step)),
                ] {
                    if i.key_pressed(k) {
                        n += v;
                    }
                }
                (del, all, dup, n)
            });
            let ds = &mut self.designer;
            let objects = &mut ds.design.doc.objects;
            if del && !ds.selected.is_empty() {
                let mut sel = ds.selected.clone();
                sel.sort_unstable();
                for i in sel.into_iter().rev() {
                    objects.remove(i);
                }
                ds.selected.clear();
            }
            if all {
                ds.selected = (0..objects.len()).collect();
            }
            if dup && !ds.selected.is_empty() {
                let copies: Vec<CanvasObject> = ds
                    .selected
                    .iter()
                    .filter_map(|i| objects.get(*i).cloned())
                    .map(|mut o| {
                        o.id = make_id();
                        o.x += 20.0;
                        o.y += 20.0;
                        o
                    })
                    .collect();
                let start = objects.len();
                objects.extend(copies);
                ds.selected = (start..objects.len()).collect();
            }
            if nudge != Vec2::ZERO {
                for i in &ds.selected {
                    if let Some(o) = objects.get_mut(*i) {
                        o.x += nudge.x;
                        o.y += nudge.y;
                    }
                }
            }
        }

        // drawing
        let painter = ui.painter_at(rect.expand(HANDLE));
        render::draw_canvas(&painter, rect, &self.designer.design.doc, &mut self.textures);
        painter.rect_stroke(rect, 0.0, Stroke::new(1.0, Color32::GRAY), StrokeKind::Outside);
        let sel_color = Color32::from_rgb(40, 140, 255);
        for i in &self.designer.selected {
            if let Some(o) = self.designer.design.doc.objects.get(*i) {
                let r = Rect::from_min_max(render::to_screen(rect, o.rect().min), render::to_screen(rect, o.rect().max));
                painter.rect_stroke(r, 0.0, Stroke::new(1.5, sel_color), StrokeKind::Outside);
                painter.rect_filled(Rect::from_center_size(r.max, Vec2::splat(HANDLE)), 2.0, sel_color);
            }
        }
        if let Some(p) = resp.hover_pos()
            && self.designer.selected.iter().any(|i| {
                self.designer
                    .design
                    .doc
                    .objects
                    .get(*i)
                    .map(|o| Rect::from_center_size(render::to_screen(rect, o.rect().max), Vec2::splat(HANDLE * 1.5)).contains(p))
                    .unwrap_or(false)
            })
        {
            ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeNwSe);
        }
        ui.scope_builder(
            egui::UiBuilder::new()
                .max_rect(Rect::from_min_size(Pos2::new(rect.left(), rect.bottom() + 4.0), Vec2::new(rect.width(), 20.0))),
            |ui| {
                ui.weak(t("designer.keys_hint"));
            },
        );
    }

    fn designer_properties(&mut self, ui: &mut egui::Ui) {
        let ds = &mut self.designer;
        if ui
            .horizontal(|ui| {
                ui.label(t("designer.name"));
                ui.text_edit_singleline(&mut ds.design.name).changed()
            })
            .inner
        {
            ds.dirty = true;
        }
        ui.separator();
        if ds.selected.len() != 1 {
            ui.label(egui::RichText::new(t("designer.canvas_props")).strong());
            egui::Grid::new("canvas_props").num_columns(2).show(ui, |ui| {
                color_edit(ui, &t("designer.background"), &mut ds.design.doc.background);
            });
            if ds.selected.len() > 1 {
                ui.weak(t("designer.multi_selected").replace("{n}", &ds.selected.len().to_string()));
            }
            ui.separator();
            ui.label(egui::RichText::new(t("designer.objects")).strong());
            let mut select = None;
            for (i, o) in ds.design.doc.objects.iter().enumerate().rev() {
                let name = match &o.kind {
                    ObjectKind::Text { text, .. } => format!("T  {}", text.lines().next().unwrap_or("")),
                    ObjectKind::Rect { .. } => format!("▭  {}", t("designer.rect")),
                    ObjectKind::Ellipse { .. } => format!("◯  {}", t("designer.ellipse")),
                    ObjectKind::Image { .. } => format!("🖼  {}", t("designer.image")),
                };
                if ui.selectable_label(ds.selected.contains(&i), name).clicked() {
                    select = Some(i);
                }
            }
            if let Some(i) = select {
                ds.selected = vec![i];
            }
            return;
        }
        let idx = ds.selected[0];
        let n = ds.design.doc.objects.len();
        let mut replace_image = false;
        let mut order: Option<isize> = None;
        let mut delete = false;
        {
            let o = &mut ds.design.doc.objects[idx];
            ui.label(egui::RichText::new(t("designer.object")).strong());
            let (wl, hl) = (t("designer.width"), t("designer.height"));
            egui::Grid::new("obj_props").num_columns(2).spacing([8.0, 6.0]).show(ui, |ui| {
                for (label, v) in [("X", &mut o.x), ("Y", &mut o.y), (wl.as_str(), &mut o.w), (hl.as_str(), &mut o.h)] {
                    ui.label(label);
                    ui.add(egui::DragValue::new(v).speed(1.0));
                    ui.end_row();
                }
                match &mut o.kind {
                    ObjectKind::Text { text, font_size, color, align, font } => {
                        ui.label(t("designer.font_size"));
                        ui.add(egui::DragValue::new(font_size).range(4.0..=400.0));
                        ui.end_row();
                        color_edit(ui, &t("designer.color"), color);
                        ui.label(t("designer.align"));
                        ui.horizontal(|ui| {
                            ui.selectable_value(align, HAlign::Left, "⬅");
                            ui.selectable_value(align, HAlign::Center, "↔");
                            ui.selectable_value(align, HAlign::Right, "➡");
                        });
                        ui.end_row();
                        ui.label(t("designer.font"));
                        ui.horizontal(|ui| {
                            ui.selectable_value(font, FontStyle::Proportional, t("designer.font_sans"));
                            ui.selectable_value(font, FontStyle::Monospace, t("designer.font_mono"));
                        });
                        ui.end_row();
                        ui.label(t("designer.text"));
                        ui.add(egui::TextEdit::multiline(text).desired_rows(4));
                        ui.end_row();
                    }
                    ObjectKind::Rect { fill, stroke, stroke_width, corner_radius } => {
                        color_edit(ui, &t("designer.fill"), fill);
                        color_edit(ui, &t("designer.stroke"), stroke);
                        ui.label(t("designer.stroke_width"));
                        ui.add(egui::DragValue::new(stroke_width).range(0.0..=100.0));
                        ui.end_row();
                        ui.label(t("designer.radius"));
                        ui.add(egui::DragValue::new(corner_radius).range(0.0..=500.0));
                        ui.end_row();
                    }
                    ObjectKind::Ellipse { fill, stroke, stroke_width } => {
                        color_edit(ui, &t("designer.fill"), fill);
                        color_edit(ui, &t("designer.stroke"), stroke);
                        ui.label(t("designer.stroke_width"));
                        ui.add(egui::DragValue::new(stroke_width).range(0.0..=100.0));
                        ui.end_row();
                    }
                    ObjectKind::Image { media, opacity } => {
                        ui.label(t("designer.image"));
                        ui.weak(media.as_str());
                        ui.end_row();
                        ui.label(t("designer.opacity"));
                        let mut percent = (*opacity * 100.0).round();
                        if ui
                            .add(egui::Slider::new(&mut percent, 0.0..=100.0).suffix(" %"))
                            .on_hover_text(t("designer.opacity_hint"))
                            .changed()
                        {
                            *opacity = percent / 100.0;
                        }
                        ui.end_row();
                        ui.label("");
                        if ui.button(t("designer.replace_image")).clicked() {
                            replace_image = true;
                        }
                        ui.end_row();
                    }
                }
            });
            ui.horizontal_wrapped(|ui| {
                if ui.button(t("designer.fill_screen")).clicked() {
                    o.x = 0.0;
                    o.y = 0.0;
                    o.w = CANVAS_W;
                    o.h = CANVAS_H;
                }
                if ui.button(t("designer.center")).clicked() {
                    o.x = (CANVAS_W - o.w) / 2.0;
                    o.y = (CANVAS_H - o.h) / 2.0;
                }
            });
        }
        ui.horizontal_wrapped(|ui| {
            if ui.button(t("designer.to_front")).clicked() {
                order = Some(isize::MAX);
            }
            if ui.button(t("designer.forward")).clicked() {
                order = Some(1);
            }
            if ui.button(t("designer.backward")).clicked() {
                order = Some(-1);
            }
            if ui.button(t("designer.to_back")).clicked() {
                order = Some(isize::MIN);
            }
            if ui.button(format!("✖ {}", t("delete"))).clicked() {
                delete = true;
            }
        });
        if let Some(o) = order {
            let target = match o {
                isize::MAX => n - 1,
                isize::MIN => 0,
                d => (idx as isize + d).clamp(0, n as isize - 1) as usize,
            };
            let obj = ds.design.doc.objects.remove(idx);
            ds.design.doc.objects.insert(target, obj);
            ds.selected = vec![target];
        }
        if delete {
            ds.design.doc.objects.remove(idx);
            ds.selected.clear();
        }
        if replace_image && let Some(id) = self.designer.design.doc.objects.get(idx).map(|o| o.id.clone()) {
            self.pick_image(super::files::Purpose::DesignerReplaceImage(id));
        }
    }

    /// Called every frame: records undo steps and marks the design dirty. Rapid
    /// property edits (typing, dragging a value) merge into one step; adding,
    /// removing or duplicating objects is always a step of its own.
    pub fn designer_commit(&mut self, ctx: &egui::Context) {
        let ds = &mut self.designer;
        if ds.design.doc == ds.committed || ctx.input(|i| i.pointer.any_down()) {
            return;
        }
        let structural = ds.design.doc.objects.len() != ds.committed.objects.len();
        if structural || ds.last_push.elapsed() > Duration::from_millis(800) || ds.undo.is_empty() {
            ds.undo.push(ds.committed.clone());
            if ds.undo.len() > 200 {
                ds.undo.remove(0);
            }
        }
        ds.last_push = Instant::now();
        ds.redo.clear();
        ds.committed = ds.design.doc.clone();
        ds.dirty = true;
    }

    pub fn designer_undo(&mut self) {
        let ds = &mut self.designer;
        if let Some(prev) = ds.undo.pop() {
            ds.redo.push(ds.design.doc.clone());
            ds.design.doc = prev.clone();
            ds.committed = prev;
            ds.selected.retain(|i| *i < ds.design.doc.objects.len());
            ds.dirty = true;
        }
    }

    pub fn designer_redo(&mut self) {
        let ds = &mut self.designer;
        if let Some(next) = ds.redo.pop() {
            ds.undo.push(ds.design.doc.clone());
            ds.design.doc = next.clone();
            ds.committed = next;
            ds.selected.retain(|i| *i < ds.design.doc.objects.len());
            ds.dirty = true;
        }
    }
}
