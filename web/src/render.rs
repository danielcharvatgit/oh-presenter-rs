//! Drawing scenes (designs and text slides) with the egui painter: previews and the output
//! window. The drawing is the program's (`../src/render.rs`, copied: that file also plays
//! videos and reads pictures from files); pictures come from the media of the services
//! (IndexedDB, kept in memory while the page is open).

use crate::model::{
    Alert, AlertDirection, AlertFont, CANVAS_H, CANVAS_W, CanvasDoc, ColumnLayout, FontStyle, FooterPosition, HAlign, ObjectKind, Scene,
    TextScene, VAlign,
};
use eframe::egui::{self, Align, Color32, FontFamily, FontId, Painter, Pos2, Rect, Stroke, StrokeKind, TextureHandle, Vec2};
use std::collections::HashMap;

pub const OUTPUT_FONT: &str = "output";
pub const SERIF_FONT: &str = "output-serif";

/// Picture textures by media name, made from the pictures' bytes.
#[derive(Default)]
pub struct TextureCache {
    /// The bytes of the pictures (media name → file bytes).
    pub media: HashMap<String, std::rc::Rc<Vec<u8>>>,
    map: HashMap<String, Option<TextureHandle>>,
}

impl TextureCache {
    pub fn add_media(&mut self, name: &str, bytes: Vec<u8>) {
        self.map.remove(name);
        self.media.insert(name.to_string(), std::rc::Rc::new(bytes));
    }

    pub fn get(&mut self, ctx: &egui::Context, name: &str) -> Option<TextureHandle> {
        if let Some(t) = self.map.get(name) {
            return t.clone();
        }
        // not (yet) loaded: no picture, and no cached miss
        let bytes = self.media.get(name)?.clone();
        let tex = image::load_from_memory(&bytes).ok().map(|img| {
            let max = ctx.input(|i| i.max_texture_side).min(4096) as u32;
            let img = if img.width() > max || img.height() > max { img.thumbnail(max, max) } else { img };
            let rgba = img.to_rgba8();
            let size = [rgba.width() as usize, rgba.height() as usize];
            ctx.load_texture(name, egui::ColorImage::from_rgba_unmultiplied(size, rgba.as_raw()), egui::TextureOptions::LINEAR)
        });
        self.map.insert(name.to_string(), tex.clone());
        tex
    }
}

/// Draws a scene (a video, which the web edition does not play, is a black slide with a note).
pub fn draw_scene(painter: &Painter, rect: Rect, scene: &Scene, tex: &mut TextureCache) {
    let painter = painter.with_clip_rect(rect.intersect(painter.clip_rect()));
    match scene {
        Scene::Blank => {
            painter.rect_filled(rect, 0.0, Color32::BLACK);
        }
        Scene::Canvas(doc) => draw_canvas(&painter, rect, doc, tex),
        Scene::Text(t) => draw_text_scene(&painter, rect, t, tex),
        Scene::Video(v) => {
            painter.rect_filled(rect, 0.0, Color32::BLACK);
            let note = format!("🎞 {}\n{}", v.media, crate::i18n::t("render.no_video"));
            painter.text(rect.center(), egui::Align2::CENTER_CENTER, note, FontId::proportional(14.0), Color32::GRAY);
        }
    }
}

/// Largest 16:9 rect that fits into `outer`, centered.
pub fn fit_16_9(outer: Rect) -> Rect {
    let aspect = CANVAS_W / CANVAS_H;
    let size = if outer.width() / outer.height() > aspect {
        Vec2::new(outer.height() * aspect, outer.height())
    } else {
        Vec2::new(outer.width(), outer.width() / aspect)
    };
    Rect::from_center_size(outer.center(), size)
}

/// Maps a point from canvas coordinates into `rect`.
pub fn to_screen(rect: Rect, p: Pos2) -> Pos2 {
    let s = rect.width() / CANVAS_W;
    rect.min + p.to_vec2() * s
}

pub fn draw_canvas(painter: &Painter, rect: Rect, doc: &CanvasDoc, tex: &mut TextureCache) {
    painter.rect_filled(rect, 0.0, doc.background.to_color32());
    let s = rect.width() / CANVAS_W;
    for obj in &doc.objects {
        let r = Rect::from_min_max(to_screen(rect, obj.rect().min), to_screen(rect, obj.rect().max));
        match &obj.kind {
            ObjectKind::Rect { fill, stroke, stroke_width, corner_radius } => {
                painter.rect_filled(r, corner_radius * s, fill.to_color32());
                if *stroke_width > 0.0 {
                    painter.rect_stroke(r, corner_radius * s, Stroke::new(stroke_width * s, stroke.to_color32()), StrokeKind::Middle);
                }
            }
            ObjectKind::Ellipse { fill, stroke, stroke_width } => {
                let radius = r.size() / 2.0;
                painter.add(egui::Shape::ellipse_filled(r.center(), radius, fill.to_color32()));
                if *stroke_width > 0.0 {
                    painter.add(egui::Shape::ellipse_stroke(r.center(), radius, Stroke::new(stroke_width * s, stroke.to_color32())));
                }
            }
            ObjectKind::Image { media, opacity } => match tex.get(painter.ctx(), media) {
                Some(t) => {
                    // the picture's own alpha channel is kept (straight alpha, see TextureCache::get);
                    // the opacity fades the whole picture on top of it
                    let tint = Color32::WHITE.gamma_multiply(opacity.clamp(0.0, 1.0));
                    painter.image(t.id(), r, Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), tint);
                }
                None => {
                    painter.rect_stroke(r, 0.0, Stroke::new(1.0, Color32::GRAY), StrokeKind::Inside);
                    painter.line_segment([r.left_top(), r.right_bottom()], Stroke::new(1.0, Color32::GRAY));
                    painter.line_segment([r.right_top(), r.left_bottom()], Stroke::new(1.0, Color32::GRAY));
                }
            },
            ObjectKind::Text { text, font_size, color, align, font } => {
                let family = match font {
                    FontStyle::Proportional => FontFamily::Name(OUTPUT_FONT.into()),
                    FontStyle::Monospace => FontFamily::Monospace,
                };
                let galley = layout(painter, text, FontId::new((font_size * s).max(1.0), family), color.to_color32(), r.width(), *align);
                let x = match align {
                    HAlign::Left => r.left(),
                    HAlign::Center => r.center().x,
                    HAlign::Right => r.right(),
                };
                painter.galley(Pos2::new(x, r.top()), galley, color.to_color32());
            }
        }
    }
}

fn layout(painter: &Painter, text: &str, font: FontId, color: Color32, wrap: f32, align: HAlign) -> std::sync::Arc<egui::Galley> {
    let mut job = egui::text::LayoutJob::simple(text.to_string(), font, color, wrap.max(1.0));
    job.halign = match align {
        HAlign::Left => Align::LEFT,
        HAlign::Center => Align::Center,
        HAlign::Right => Align::RIGHT,
    };
    painter.layout_job(job)
}

pub fn draw_text_scene(painter: &Painter, rect: Rect, t: &TextScene, tex: &mut TextureCache) {
    match &t.background {
        Some(doc) => draw_canvas(painter, rect, doc, tex),
        None => {
            painter.rect_filled(rect, 0.0, t.style.background.to_color32());
        }
    }
    let s = rect.width() / CANVAS_W;
    let st = &t.style;
    let margin = st.margin.clamp(0.0, 400.0) * s;
    let mut area = rect.shrink(margin);
    let color = st.color.to_color32();
    let family = FontFamily::Name(OUTPUT_FONT.into());
    let shadow = |pos: Pos2, g: &std::sync::Arc<egui::Galley>, off: f32| {
        if st.shadow {
            painter.galley_with_override_text_color(pos + Vec2::splat(off * s), g.clone(), Color32::from_black_alpha(200));
        }
    };
    let x_of = |a: Rect, align: HAlign| match align {
        HAlign::Left => a.left(),
        HAlign::Center => a.center().x,
        HAlign::Right => a.right(),
    };

    // the additional info (reference, song title): at an edge, or right below the text
    let footer = (st.show_footer && !t.footer.trim().is_empty()).then(|| {
        layout(painter, &t.footer, FontId::new((st.footer_size * s).max(1.0), family.clone()), color, area.width(), st.footer_align)
    });
    let footer_gap = 10.0 * s;
    if let Some(fg) = &footer {
        match st.footer_position {
            FooterPosition::Top => {
                let pos = Pos2::new(x_of(area, st.footer_align), area.top());
                shadow(pos, fg, 2.0);
                painter.galley(pos, fg.clone(), color);
                area.min.y += fg.size().y + footer_gap;
            }
            FooterPosition::Bottom => {
                let pos = Pos2::new(x_of(area, st.footer_align), area.bottom() - fg.size().y);
                shadow(pos, fg, 2.0);
                painter.galley(pos, fg.clone(), color);
                area.max.y -= fg.size().y + footer_gap;
            }
            FooterPosition::BelowText => {}
        }
    }
    let below = footer.as_ref().filter(|_| st.footer_position == FooterPosition::BelowText);

    // the columns (other languages) next to each other, or one below the other with an
    // empty line between them
    let texts: Vec<&str> =
        std::iter::once(t.text.as_str()).chain(t.extra.iter().map(String::as_str)).filter(|x| !x.trim().is_empty()).collect();
    let texts = if texts.is_empty() { vec![t.text.as_str()] } else { texts };
    let n = texts.len() as f32;
    let col_gap = 30.0 * s;
    let side = t.layout == ColumnLayout::SideBySide && texts.len() > 1;
    let col_w = if side { (area.width() - col_gap * (n - 1.0)) / n } else { area.width() };

    // shrink the font until everything fits; all columns share one size
    let mut size = st.max_font_size * s;
    let min = 8.0 * s;
    let (galleys, line, height) = loop {
        let font = FontId::new(size.max(1.0), family.clone());
        let gs: Vec<_> = texts.iter().map(|text| layout(painter, text, font.clone(), color, col_w, st.text_halign)).collect();
        // one empty line of the text's size
        let line = painter.layout_no_wrap("Ag".into(), font, color).size().y;
        let text_h = if side {
            gs.iter().map(|g| g.size().y).fold(0.0, f32::max)
        } else {
            gs.iter().map(|g| g.size().y).sum::<f32>() + line * (n - 1.0)
        };
        let height = text_h + below.map(|f| line + f.size().y).unwrap_or(0.0);
        let fits = height <= area.height() && gs.iter().all(|g| g.size().x <= col_w + 1.0);
        if fits || size <= min {
            break (gs, line, height);
        }
        size *= 0.92;
    };
    let top = match st.text_valign {
        VAlign::Top => area.top(),
        VAlign::Middle => area.center().y - height / 2.0,
        VAlign::Bottom => area.bottom() - height,
    };
    let mut y = top;
    if side {
        let mut text_h: f32 = 0.0;
        for (i, g) in galleys.iter().enumerate() {
            let a = Rect::from_min_size(Pos2::new(area.left() + i as f32 * (col_w + col_gap), top), Vec2::new(col_w, area.height()));
            let pos = Pos2::new(x_of(a, st.text_halign), top);
            shadow(pos, g, 3.0);
            painter.galley(pos, g.clone(), color);
            text_h = text_h.max(g.size().y);
            if i > 0 {
                // a thin divider between the columns
                let x = a.left() - col_gap / 2.0;
                let stroke = Stroke::new(1.5 * s, color.gamma_multiply(0.35));
                painter.line_segment([Pos2::new(x, top), Pos2::new(x, top + text_h.max(g.size().y))], stroke);
            }
        }
        y += text_h;
    } else {
        for (i, g) in galleys.iter().enumerate() {
            if i > 0 {
                y += line;
            }
            let pos = Pos2::new(x_of(area, st.text_halign), y);
            shadow(pos, g, 3.0);
            painter.galley(pos, g.clone(), color);
            y += g.size().y;
        }
    }
    if let Some(fg) = below {
        let pos = Pos2::new(x_of(area, st.footer_align), y + line);
        shadow(pos, fg, 2.0);
        painter.galley(pos, fg.clone(), color);
    }
}

fn alert_family(font: AlertFont) -> FontFamily {
    match font {
        AlertFont::Sans => FontFamily::Name(OUTPUT_FONT.into()),
        AlertFont::Serif => FontFamily::Name(SERIF_FONT.into()),
        AlertFont::Monospace => FontFamily::Monospace,
    }
}

/// Seconds the alert's text takes to run once across the screen.
pub fn alert_pass_secs(painter: &Painter, alert: &Alert) -> f32 {
    let g = painter.layout_no_wrap(alert.text.clone(), FontId::new(alert.style.font_size, alert_family(alert.style.font)), Color32::WHITE);
    (CANVAS_W + g.size().x) / alert.style.speed.max(10.0)
}

/// How long an alert really runs: its duration, but at least once across (0 = until stopped).
pub fn alert_run_secs(painter: &Painter, alert: &Alert) -> Option<f32> {
    (alert.duration > 0.0).then(|| alert.duration.max(alert_pass_secs(painter, alert)))
}

/// Draws an alert `elapsed` seconds after it started over the slide in `rect`.
/// Returns false once it has run its time (nothing is drawn then).
pub fn draw_alert(painter: &Painter, rect: Rect, alert: &Alert, elapsed: f32) -> bool {
    if alert_run_secs(painter, alert).is_some_and(|end| elapsed >= end) || alert.text.trim().is_empty() {
        return false;
    }
    let st = &alert.style;
    let s = rect.width() / CANVAS_W;
    let painter = painter.with_clip_rect(rect.intersect(painter.clip_rect()));
    let text = alert.text.replace(['\n', '\r'], "  ");
    let galley = painter.layout_no_wrap(text, FontId::new(st.font_size * s, alert_family(st.font)), st.color.to_color32());
    let pad = st.font_size * 0.3 * s;
    let h = galley.size().y + 2.0 * pad;
    let top = match st.valign {
        VAlign::Top => rect.top(),
        VAlign::Middle => rect.center().y - h / 2.0,
        VAlign::Bottom => rect.bottom() - h,
    };
    let strip = Rect::from_min_size(Pos2::new(rect.left(), top), Vec2::new(rect.width(), h));
    painter.rect_filled(strip, 0.0, st.background.to_color32());
    // one pass: from fully outside one edge to fully outside the other, then again
    let w = galley.size().x;
    let travel = rect.width() + w;
    let moved = (elapsed * st.speed.max(10.0) * s) % travel;
    let x = match st.direction {
        AlertDirection::RightToLeft => rect.right() - moved,
        AlertDirection::LeftToRight => rect.left() - w + moved,
    };
    painter.galley(Pos2::new(x, top + pad), galley, st.color.to_color32());
    true
}

/// The `output` font families: egui's fonts (they have the Czech letters).
pub fn install_fonts(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    let sans = fonts.families.get(&FontFamily::Proportional).cloned().unwrap_or_default();
    fonts.families.insert(FontFamily::Name(OUTPUT_FONT.into()), sans.clone());
    fonts.families.insert(FontFamily::Name(SERIF_FONT.into()), sans);
    ctx.set_fonts(fonts);
}
