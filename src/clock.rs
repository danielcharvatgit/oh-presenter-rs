//! The clock on the output: three coloured dots that encode the time (and optionally
//! the time as plain digits), so the speaker can see the time without the audience
//! reading a clock on the screen.
//!
//! # Reading the encoded clock
//!
//! Every dot has one of four colours, in this order: red = 0, green = 1, blue = 2,
//! brown = 3. The dots split the 12-hour clock face into quarters, again and again:
//!
//! * 1st dot: the quarter of the clock face (3 hours each):
//!   red 12–3, green 3–6, blue 6–9, brown 9–12 o'clock;
//! * 2nd dot: the quarter of those 3 hours (45 minutes each): +0, +45, +90, +135 min;
//! * 3rd dot: the quarter of those 45 minutes (11¼ minutes each): +0, +11, +22, +34 min.
//!
//! time = 12:00 + 3 h × 1st + 45 min × 2nd + 11¼ min × 3rd
//!
//! At 12 o'clock (noon and midnight) all three dots are red; then the last dot
//! changes colour every 11¼ minutes, the middle one every 45 minutes and the first
//! one every 3 hours, until all are brown just before 12 again. Example: green, blue,
//! red = 3:00 + 1:30 = 4:30 (to 4:41).

use crate::model::{CANVAS_W, ClockPosition, ClockSize, ClockStyle};
use crate::render::OUTPUT_FONT;
use eframe::egui::{Color32, FontFamily, FontId, Painter, Pos2, Rect, Stroke, Vec2};
use std::time::Duration;

/// Seconds in the 12 hours the encoded clock goes round in.
const ROUND: u32 = 12 * 3600;
/// Seconds one step of each dot lasts: 3 h, 45 min, 11¼ min.
const STEP: [u32; 3] = [ROUND / 4, ROUND / 16, ROUND / 64];

/// The four colours, in the order of their values 0–3: red, green, blue, brown.
pub const COLORS: [Color32; 4] =
    [Color32::from_rgb(225, 40, 40), Color32::from_rgb(30, 175, 70), Color32::from_rgb(40, 115, 235), Color32::from_rgb(140, 85, 40)];

/// Translation keys of the colour names, in the order of [`COLORS`].
pub const COLOR_KEYS: [&str; 4] = ["clock.red", "clock.green", "clock.blue", "clock.brown"];

/// Local time: seconds since midnight.
pub fn now_secs() -> u32 {
    use chrono::Timelike;
    chrono::Local::now().num_seconds_from_midnight()
}

/// The colours (0–3) of the three dots at `secs` seconds after midnight.
pub fn encode(secs: u32) -> [usize; 3] {
    let s = secs % ROUND;
    [(s / STEP[0]) as usize, (s % STEP[0] / STEP[1]) as usize, (s % STEP[1] / STEP[2]) as usize]
}

/// The time the dots stand for: seconds after 12 o'clock at which they appear
/// (they stay for the next 11¼ minutes).
pub fn decode(dots: [usize; 3]) -> u32 {
    dots.iter().zip(STEP).map(|(d, step)| (*d as u32 % 4) * step).sum()
}

/// `h:mm` on a 12-hour clock face (12:00 … 11:59), for explaining the dots.
pub fn face_time(secs: u32) -> String {
    let m = secs % ROUND / 60;
    let h = if m / 60 == 0 { 12 } else { m / 60 };
    format!("{h}:{:02}", m % 60)
}

/// `HH:MM` (24 hours), the plain clock.
pub fn plain_time(secs: u32) -> String {
    let m = secs / 60 % (24 * 60);
    format!("{:02}:{:02}", m / 60, m % 60)
}

/// How long until the clock looks different (the next minute or the next dot change).
pub fn repaint_in(style: &ClockStyle, secs: u32) -> Duration {
    let mut left = STEP[2] - secs % STEP[2];
    if style.plain {
        left = left.min(60 - secs % 60);
    }
    // a little after the change, so the new state is certainly drawn
    Duration::from_millis(left as u64 * 1000 + 200)
}

/// Diameter of a dot in canvas points (the canvas is 1280 wide).
fn dot_size(size: ClockSize) -> f32 {
    match size {
        ClockSize::Tiny => 12.0 / 3.0,
        ClockSize::Small => 12.0,
        ClockSize::Medium => 18.0,
    }
}

/// The size of the whole clock (the dark plate with the dots and the digits), as drawn
/// for a slide `slide_width` wide.
pub fn plate_size(painter: &Painter, slide_width: f32, style: &ClockStyle, secs: u32) -> Vec2 {
    layout(painter, slide_width, style, secs).0
}

/// The plate size and the digits (if shown).
fn layout(painter: &Painter, slide_width: f32, style: &ClockStyle, secs: u32) -> (Vec2, Option<std::sync::Arc<eframe::egui::Galley>>) {
    let d = dot_size(style.size) * slide_width / CANVAS_W;
    let gap = d * 0.45;
    let pad = d * 0.35;
    let text = style
        .plain
        .then(|| painter.layout_no_wrap(plain_time(secs), FontId::new(d * 1.25, FontFamily::Name(OUTPUT_FONT.into())), Color32::WHITE));
    let dots_w = 3.0 * d + 2.0 * gap;
    let text_w = text.as_ref().map(|g| gap * 1.5 + g.size().x).unwrap_or(0.0);
    let h = text.as_ref().map(|g| g.size().y).unwrap_or(0.0).max(d);
    (Vec2::new(dots_w + text_w + 2.0 * pad, h + 2.0 * pad), text)
}

/// Draws the clock at the edge of `screen` (the whole output window). `slide_width` is
/// the width of the slide in it, which sets the size (like everything on the slide).
pub fn draw(painter: &Painter, screen: Rect, slide_width: f32, style: &ClockStyle, secs: u32) {
    if !style.show {
        return;
    }
    let s = slide_width / CANVAS_W;
    let d = dot_size(style.size) * s;
    let gap = d * 0.45;
    let pad = d * 0.35;
    let alpha = style.opacity.clamp(10, 100) as f32 / 100.0;
    let (size, text) = layout(painter, slide_width, style, secs);
    let dots_w = 3.0 * d + 2.0 * gap;
    let margin = d * 0.6;
    let x = match style.position {
        ClockPosition::Left => screen.left() + margin,
        ClockPosition::BottomMiddle | ClockPosition::TopMiddle => screen.center().x - size.x / 2.0,
        ClockPosition::Right => screen.right() - margin - size.x,
    };
    let y = match style.position {
        ClockPosition::TopMiddle => screen.top() + margin,
        _ => screen.bottom() - margin - size.y,
    };
    let bg = Rect::from_min_size(Pos2::new(x, y), size);
    // a dark rounded plate, so the dots can be told apart on any slide
    painter.rect_filled(bg, size.y / 2.0, Color32::from_black_alpha(120).gamma_multiply(alpha));
    let cy = bg.center().y;
    // the smallest dots have no outline (it would cover most of their colour)
    let outline = if style.size == ClockSize::Tiny {
        Stroke::NONE
    } else {
        Stroke::new((d * 0.08).max(1.0), Color32::from_white_alpha(140).gamma_multiply(alpha))
    };
    for (i, c) in encode(secs).into_iter().enumerate() {
        let cx = bg.left() + pad + d / 2.0 + i as f32 * (d + gap);
        painter.circle(Pos2::new(cx, cy), d / 2.0, COLORS[c].gamma_multiply(alpha), outline);
    }
    if let Some(g) = text {
        let pos = Pos2::new(bg.left() + pad + dots_w + gap * 1.5, cy - g.size().y / 2.0);
        painter.galley_with_override_text_color(pos, g, Color32::WHITE.gamma_multiply(alpha));
    }
}
