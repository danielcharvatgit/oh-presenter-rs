//! Drawing scenes (canvas designs and text slides) with the egui painter.
//! Shared by previews, the output window and network canvas clients.

use crate::model::{
    Alert, AlertDirection, AlertFont, CANVAS_H, CANVAS_W, CanvasDoc, ColumnLayout, FontStyle, FooterPosition, HAlign, ObjectKind, Scene,
    TextScene, VAlign, VideoScene,
};
use eframe::egui::{self, Align, Color32, FontFamily, FontId, Painter, Pos2, Rect, Stroke, StrokeKind, TextureHandle, Vec2};
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Instant;

pub const OUTPUT_FONT: &str = "output";
/// A serif system font for alerts (falls back to the output font).
pub const SERIF_FONT: &str = "output-serif";

/// Lazily loaded image textures, keyed by media name, and the video playing in this
/// window (one view shows one live scene, so one video at a time).
pub struct TextureCache {
    dir: PathBuf,
    map: HashMap<String, Option<TextureHandle>>,
    video: Option<VideoView>,
}

/// A video being shown: which playback (token), since when, and its decoder.
struct VideoView {
    media: String,
    token: u64,
    since: Instant,
    player: Option<crate::video::Player>,
    /// The decoder could not be started (no ffmpeg); not retried for this playback.
    failed: bool,
}

impl TextureCache {
    pub fn new(dir: PathBuf) -> Self {
        Self { dir, map: HashMap::new(), video: None }
    }

    pub fn forget(&mut self, name: &str) {
        self.map.remove(name);
        // a video file that has just arrived (network outputs): try to play it again
        if let Some(v) = &mut self.video
            && v.media == name
        {
            v.player = None;
            v.failed = false;
        }
    }

    /// Stops the video playing in this window.
    pub fn stop_video(&mut self) {
        self.video = None;
    }

    pub fn get(&mut self, ctx: &egui::Context, name: &str) -> Option<TextureHandle> {
        if let Some(t) = self.map.get(name) {
            return t.clone();
        }
        let path = self.dir.join(name);
        let tex = image::open(&path).ok().map(|img| {
            let max = ctx.input(|i| i.max_texture_side).min(4096) as u32;
            let img = if img.width() > max || img.height() > max { img.thumbnail(max, max) } else { img };
            let rgba = img.to_rgba8();
            let size = [rgba.width() as usize, rgba.height() as usize];
            ctx.load_texture(name, egui::ColorImage::from_rgba_unmultiplied(size, rgba.as_raw()), egui::TextureOptions::LINEAR)
        });
        if tex.is_none() && !path.exists() {
            // may arrive later (network clients) — don't cache the miss
            return None;
        }
        self.map.insert(name.to_string(), tex.clone());
        tex
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

/// Whether the output window paints black behind `scene`. With a transparent output
/// (`transparent`), a designed slide with a transparent background (a picture with an alpha
/// channel) is not: what is behind the window (another program, a video player) shows
/// through it, so the picture is an overlay.
pub fn paints_black_behind(transparent: bool, scene: &Scene) -> bool {
    !(transparent && matches!(scene, Scene::Canvas(doc) if doc.background.0[3] == 0))
}

/// Draws a scene as a still picture (previews): a video shows its poster.
pub fn draw_scene(painter: &Painter, rect: Rect, scene: &Scene, tex: &mut TextureCache) {
    let painter = painter.with_clip_rect(rect.intersect(painter.clip_rect()));
    match scene {
        Scene::Blank => {
            painter.rect_filled(rect, 0.0, Color32::BLACK);
        }
        Scene::Canvas(doc) => draw_canvas(&painter, rect, doc, tex),
        Scene::Text(t) => draw_text_scene(&painter, rect, t, tex),
        Scene::Video(v) => {
            draw_poster(&painter, rect, v, tex);
            draw_play_badge(&painter, rect, v.duration);
        }
    }
}

/// Draws the scene shown live (output window, live preview, network output): a video plays.
/// Only one live scene per texture cache, so a video that is no longer shown stops.
pub fn draw_scene_live(painter: &Painter, rect: Rect, scene: &Scene, tex: &mut TextureCache) {
    match scene {
        Scene::Video(v) => {
            let painter = painter.with_clip_rect(rect.intersect(painter.clip_rect()));
            draw_video(&painter, rect, v, tex);
        }
        _ => {
            tex.stop_video();
            draw_scene(painter, rect, scene, tex);
        }
    }
}

/// The video's still picture, letterboxed on black.
fn draw_poster(painter: &Painter, rect: Rect, v: &VideoScene, tex: &mut TextureCache) {
    painter.rect_filled(rect, 0.0, Color32::BLACK);
    if let Some(t) = v.poster.as_deref().and_then(|p| tex.get(painter.ctx(), p)) {
        let [w, h] = t.size();
        let scale = (rect.width() / w as f32).min(rect.height() / h as f32);
        let r = Rect::from_center_size(rect.center(), Vec2::new(w as f32, h as f32) * scale);
        painter.image(t.id(), r, Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), Color32::WHITE);
    }
}

/// A ▶ sign and the length over a video preview.
fn draw_play_badge(painter: &Painter, rect: Rect, duration: f64) {
    let r = (rect.height() * 0.16).clamp(5.0, 40.0);
    let c = rect.center();
    painter.circle_filled(c, r, Color32::from_black_alpha(150));
    let a = r * 0.5;
    painter.add(egui::Shape::convex_polygon(
        vec![Pos2::new(c.x - a * 0.6, c.y - a), Pos2::new(c.x + a, c.y), Pos2::new(c.x - a * 0.6, c.y + a)],
        Color32::WHITE,
        Stroke::NONE,
    ));
    if duration > 0.0 && rect.height() > 60.0 {
        let g = painter.layout_no_wrap(crate::video::format_time(duration), FontId::proportional(12.0), Color32::WHITE);
        let pos = rect.right_bottom() - g.size() - Vec2::splat(6.0);
        painter.rect_filled(Rect::from_min_size(pos, g.size()).expand(3.0), 3.0, Color32::from_black_alpha(170));
        painter.galley(pos, g, Color32::WHITE);
    }
}

/// A small message in the corner of the output (e.g. ffmpeg is missing).
fn draw_note(painter: &Painter, rect: Rect, text: String) {
    let g = painter.layout(text, FontId::proportional(14.0), Color32::WHITE, (rect.width() - 24.0).max(20.0));
    let pos = rect.left_bottom() + Vec2::new(10.0, -10.0 - g.size().y);
    painter.rect_filled(Rect::from_min_size(pos, g.size()).expand(4.0), 4.0, Color32::from_black_alpha(180));
    painter.galley(pos, g, Color32::WHITE);
}

fn draw_video(painter: &Painter, rect: Rect, v: &VideoScene, tex: &mut TextureCache) {
    let ctx = painter.ctx().clone();
    if tex.video.as_ref().is_none_or(|p| p.media != v.media || p.token != v.token) {
        tex.video = Some(VideoView { media: v.media.clone(), token: v.token, since: Instant::now(), player: None, failed: false });
    }
    let path = tex.dir.join(&v.media);
    let size = crate::video::frame_size(rect.width() * ctx.pixels_per_point());
    let view = tex.video.as_mut().expect("set above");
    let elapsed = view.since.elapsed().as_secs_f64();
    let wrong_size = view.player.as_ref().is_some_and(|p| p.size() != size && !p.finished());
    if (view.player.is_none() || wrong_size) && !view.failed && path.exists() && !v.ended_after(elapsed) {
        // (re)start the decoder where playback stands now
        view.player = None;
        match crate::video::Player::start(&path, v.position_after(elapsed), v.looped, v.paused, size) {
            Ok(p) => view.player = Some(p),
            Err(_) => view.failed = true,
        }
    }
    let waiting = !path.exists();
    let failed = view.failed;
    let has_frame = match view.player.as_mut().and_then(|p| p.texture(&ctx)) {
        Some(t) => {
            painter.image(t.id(), rect, Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), Color32::WHITE);
            true
        }
        None => false,
    };
    // ffmpeg could not decode the file (no picture at all): once more, now with VLC
    if !has_frame
        && view.player.as_ref().is_some_and(|p| p.is_ffmpeg() && p.finished() && p.no_frame_yet())
        && crate::video::uses_vlc(&path)
    {
        view.player = None;
        ctx.request_repaint();
    }
    let running = view.player.as_ref().is_some_and(|p| !p.finished());
    if !has_frame {
        draw_poster(painter, rect, v, tex);
    }
    if failed {
        draw_note(painter, rect, crate::i18n::t("video.no_ffmpeg"));
    } else if waiting {
        draw_note(painter, rect, crate::i18n::t("video.receiving"));
    }
    if running || (waiting && !failed) {
        // next frame (~60 per second; the decoder delivers the video's own rate)
        ctx.request_repaint_after(std::time::Duration::from_millis(if running { 15 } else { 500 }));
    }
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

/// Height of the alert's strip over a slide `slide_width` wide.
pub fn alert_strip_height(painter: &Painter, slide_width: f32, alert: &Alert) -> f32 {
    let s = slide_width / CANVAS_W;
    let st = &alert.style;
    let g = painter.layout_no_wrap(
        alert.text.replace(['\n', '\r'], "  "),
        FontId::new(st.font_size * s, alert_family(st.font)),
        Color32::WHITE,
    );
    g.size().y + 2.0 * st.font_size * 0.3 * s
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

/// Registers the `output` font family: a system sans font if one is found,
/// with egui's default fonts as fallback (so Czech diacritics always render).
/// A font in the Windows fonts folder (Windows need not be on C:).
fn windows_font(name: &str) -> String {
    let windir = std::env::var("WINDIR").unwrap_or_else(|_| "C:\\Windows".into());
    format!("{windir}\\Fonts\\{name}")
}

pub fn install_fonts(ctx: &egui::Context, custom_font: Option<&std::path::Path>) {
    let mut fonts = egui::FontDefinitions::default();
    let mut candidates: Vec<PathBuf> = custom_font.map(|p| vec![p.to_path_buf()]).unwrap_or_default();
    let sans_candidates: [&str; 11] = [
        "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
        "/usr/share/fonts/TTF/DejaVuSans.ttf",
        "/usr/share/fonts/dejavu/DejaVuSans.ttf",
        "/usr/share/fonts/truetype/noto/NotoSans-Regular.ttf",
        "/usr/share/fonts/noto/NotoSans-Regular.ttf",
        "/usr/share/fonts/truetype/liberation/LiberationSans-Regular.ttf",
        // the host's fonts, seen from inside a flatpak
        "/run/host/fonts/truetype/dejavu/DejaVuSans.ttf",
        "/run/host/fonts/dejavu/DejaVuSans.ttf",
        &windows_font("arial.ttf"),
        "/System/Library/Fonts/Supplemental/Arial.ttf",
        "/Library/Fonts/Arial.ttf",
    ];
    candidates.extend(sans_candidates.iter().map(PathBuf::from));
    let mut family = Vec::new();
    if let Some(bytes) = candidates.iter().find_map(|p| std::fs::read(p).ok()) {
        fonts.font_data.insert("output-sys".into(), std::sync::Arc::new(egui::FontData::from_owned(bytes)));
        family.push("output-sys".to_string());
    }
    family.extend(fonts.families.get(&FontFamily::Proportional).cloned().unwrap_or_default());
    // a serif font for alerts, falling back to the output font
    let mut serif = Vec::new();
    let serif_candidates: [&str; 11] = [
        "/usr/share/fonts/truetype/dejavu/DejaVuSerif.ttf",
        "/usr/share/fonts/TTF/DejaVuSerif.ttf",
        "/usr/share/fonts/dejavu/DejaVuSerif.ttf",
        "/usr/share/fonts/truetype/liberation/LiberationSerif-Regular.ttf",
        "/usr/share/fonts/truetype/noto/NotoSerif-Regular.ttf",
        "/usr/share/fonts/noto/NotoSerif-Regular.ttf",
        "/run/host/fonts/truetype/dejavu/DejaVuSerif.ttf",
        "/run/host/fonts/dejavu/DejaVuSerif.ttf",
        &windows_font("times.ttf"),
        "/System/Library/Fonts/Supplemental/Times New Roman.ttf",
        "/Library/Fonts/Times New Roman.ttf",
    ];
    if let Some(bytes) = serif_candidates.iter().find_map(|p| std::fs::read(p).ok()) {
        fonts.font_data.insert("output-serif".into(), std::sync::Arc::new(egui::FontData::from_owned(bytes)));
        serif.push("output-serif".to_string());
    }
    serif.extend(family.iter().cloned());
    fonts.families.insert(FontFamily::Name(SERIF_FONT.into()), serif);
    fonts.families.insert(FontFamily::Name(OUTPUT_FONT.into()), family);
    ctx.set_fonts(fonts);
}
