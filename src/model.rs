//! Core data model: songs, canvas designs, service items and the scene sent to outputs.

use serde::{Deserialize, Serialize};

/// Logical size of every canvas / output scene. Outputs scale it to fit.
pub const CANVAS_W: f32 = 1280.0;
pub const CANVAS_H: f32 = 720.0;

pub fn make_id() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos() as u64).unwrap_or(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let mut x = nanos ^ (n.wrapping_mul(0x9E37_79B9_7F4A_7C15)) ^ (std::process::id() as u64) << 32;
    const CHARS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";
    let mut s = String::with_capacity(16);
    for _ in 0..16 {
        // xorshift
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        s.push(CHARS[(x % CHARS.len() as u64) as usize] as char);
    }
    s
}

/// RGBA color, not premultiplied.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Rgba(pub [u8; 4]);

impl Rgba {
    pub const WHITE: Rgba = Rgba([255, 255, 255, 255]);
    pub const BLACK: Rgba = Rgba([0, 0, 0, 255]);
    pub const RED: Rgba = Rgba([255, 0, 0, 255]);
    pub const TRANSPARENT: Rgba = Rgba([0, 0, 0, 0]);

    pub fn to_color32(self) -> eframe::egui::Color32 {
        let [r, g, b, a] = self.0;
        eframe::egui::Color32::from_rgba_unmultiplied(r, g, b, a)
    }

    pub fn from_color32(c: eframe::egui::Color32) -> Self {
        Rgba(c.to_srgba_unmultiplied())
    }

    /// Parses `#rgb`, `#rrggbb`, `#rrggbbaa`, `rgb(..)`, `rgba(..)` and a few color names.
    pub fn parse(s: &str) -> Option<Rgba> {
        let s = s.trim().to_ascii_lowercase();
        if s.is_empty() || s == "transparent" || s == "none" {
            return Some(Rgba::TRANSPARENT);
        }
        if let Some(hex) = s.strip_prefix('#') {
            let v: Vec<u8> = hex.bytes().filter_map(|b| (b as char).to_digit(16).map(|d| d as u8)).collect();
            return match v.len() {
                3 => Some(Rgba([v[0] * 17, v[1] * 17, v[2] * 17, 255])),
                6 => Some(Rgba([v[0] * 16 + v[1], v[2] * 16 + v[3], v[4] * 16 + v[5], 255])),
                8 => Some(Rgba([v[0] * 16 + v[1], v[2] * 16 + v[3], v[4] * 16 + v[5], v[6] * 16 + v[7]])),
                _ => None,
            };
        }
        if let Some(inner) = s.strip_prefix("rgba(").or_else(|| s.strip_prefix("rgb(")) {
            let parts: Vec<f32> = inner.trim_end_matches(')').split(',').filter_map(|p| p.trim().parse().ok()).collect();
            if parts.len() >= 3 {
                let a = parts.get(3).map(|a| (a * 255.0).round() as u8).unwrap_or(255);
                return Some(Rgba([parts[0] as u8, parts[1] as u8, parts[2] as u8, a]));
            }
            return None;
        }
        Some(match s.as_str() {
            "white" => Rgba::WHITE,
            "black" => Rgba::BLACK,
            "red" => Rgba::RED,
            "green" => Rgba([0, 128, 0, 255]),
            "lime" => Rgba([0, 255, 0, 255]),
            "blue" => Rgba([0, 0, 255, 255]),
            "yellow" => Rgba([255, 255, 0, 255]),
            "orange" => Rgba([255, 165, 0, 255]),
            "gray" | "grey" => Rgba([128, 128, 128, 255]),
            "purple" => Rgba([128, 0, 128, 255]),
            _ => return None,
        })
    }
}

// ---------------------------------------------------------------------------
// Canvas designs

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum HAlign {
    Left,
    #[default]
    Center,
    Right,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum FontStyle {
    #[default]
    Proportional,
    Monospace,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum ObjectKind {
    Text {
        text: String,
        font_size: f32,
        color: Rgba,
        align: HAlign,
        font: FontStyle,
    },
    Rect {
        fill: Rgba,
        stroke: Rgba,
        stroke_width: f32,
        corner_radius: f32,
    },
    Ellipse {
        fill: Rgba,
        stroke: Rgba,
        stroke_width: f32,
    },
    /// `media` is a file name inside the media store.
    Image {
        media: String,
    },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CanvasObject {
    pub id: String,
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    pub kind: ObjectKind,
}

impl CanvasObject {
    pub fn rect(&self) -> eframe::egui::Rect {
        eframe::egui::Rect::from_min_size(eframe::egui::pos2(self.x, self.y), eframe::egui::vec2(self.w, self.h))
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CanvasDoc {
    pub background: Rgba,
    pub objects: Vec<CanvasObject>,
}

impl Default for CanvasDoc {
    fn default() -> Self {
        Self { background: Rgba::WHITE, objects: Vec::new() }
    }
}

impl CanvasDoc {
    pub fn media(&self) -> impl Iterator<Item = &str> {
        self.objects.iter().filter_map(|o| match &o.kind {
            ObjectKind::Image { media } => Some(media.as_str()),
            _ => None,
        })
    }
}

/// A saved slide design in the library.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Design {
    pub id: String,
    pub name: String,
    pub doc: CanvasDoc,
}

// ---------------------------------------------------------------------------
// Songs

/// One part of a song (verse, chorus, ...). `tag` is a slide mark code (see
/// [`crate::settings::SlideMark`]) such as `V`, `C`, `B` or a custom code.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SongPart {
    pub tag: String,
    #[serde(default)]
    pub number: u32,
    pub text: String,
    /// Texts of the song's extra columns (see [`Song::columns`]): index 0 is column 1.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub translations: Vec<String>,
}

/// Splits a text into slides on lines containing only `---` or `[---]`.
pub fn split_pages(text: &str) -> Vec<String> {
    let mut pages = vec![String::new()];
    for line in text.lines() {
        let l = line.trim();
        if l == "---" || l == "[---]" {
            pages.push(String::new());
        } else {
            let p = pages.last_mut().unwrap();
            if !p.is_empty() {
                p.push('\n');
            }
            p.push_str(line);
        }
    }
    pages.into_iter().map(|p| p.trim().to_string()).filter(|p| !p.is_empty()).collect()
}

impl SongPart {
    /// Short id used in verse orders and on slide badges, e.g. `V1`, `C`, `C2`.
    pub fn code(&self) -> String {
        if self.number == 0 || (self.number == 1 && self.tag != "V") { self.tag.clone() } else { format!("{}{}", self.tag, self.number) }
    }

    /// Text split into slides on lines containing only `---` or `[---]`.
    pub fn pages(&self) -> Vec<String> {
        split_pages(&self.text)
    }

    /// Text of a column: 0 is the main text, 1.. the translations.
    pub fn column_text(&self, column: usize) -> &str {
        if column == 0 { &self.text } else { self.translations.get(column - 1).map(String::as_str).unwrap_or("") }
    }

    pub fn column_text_mut(&mut self, column: usize) -> &mut String {
        if column == 0 {
            return &mut self.text;
        }
        if self.translations.len() < column {
            self.translations.resize(column, String::new());
        }
        &mut self.translations[column - 1]
    }

    /// Slides of this part with the texts of the given columns side by side:
    /// slide `i` holds page `i` of every column (empty where a column has fewer pages).
    pub fn column_pages(&self, columns: &[usize]) -> Vec<Vec<String>> {
        let per: Vec<Vec<String>> = columns.iter().map(|&c| split_pages(self.column_text(c))).collect();
        let n = per.iter().map(Vec::len).max().unwrap_or(0);
        (0..n).map(|i| per.iter().map(|p| p.get(i).cloned().unwrap_or_default()).collect()).collect()
    }
}

/// A text column of a song, e.g. one language.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SongColumn {
    pub name: String,
    /// Shown on the slides.
    #[serde(default = "yes")]
    pub show: bool,
}

fn yes() -> bool {
    true
}

/// How several shown columns are placed on a slide.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum ColumnLayout {
    #[default]
    SideBySide,
    Stacked,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Song {
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub author: String,
    #[serde(default)]
    pub copyright: String,
    #[serde(default)]
    pub ccli: String,
    pub parts: Vec<SongPart>,
    /// Space separated part codes, e.g. `V1 C V2 C B C`. Empty = parts in order.
    #[serde(default)]
    pub order: String,
    /// Text columns (e.g. languages). Empty = one unnamed column. Column 0 is
    /// [`SongPart::text`], column `k` is `SongPart::translations[k - 1]`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub columns: Vec<SongColumn>,
    #[serde(default)]
    pub column_layout: ColumnLayout,
}

impl Song {
    pub fn new() -> Self {
        Self { id: make_id(), ..Default::default() }
    }

    /// Parts in presentation order.
    pub fn ordered_parts(&self) -> Vec<&SongPart> {
        let codes: Vec<&str> = self.order.split_whitespace().collect();
        if codes.is_empty() {
            return self.parts.iter().collect();
        }
        let ordered: Vec<&SongPart> = codes.iter().filter_map(|c| self.parts.iter().find(|p| p.code().eq_ignore_ascii_case(c))).collect();
        if ordered.is_empty() { self.parts.iter().collect() } else { ordered }
    }

    pub fn footer(&self) -> String {
        let mut f = self.title.clone();
        if !self.author.is_empty() {
            f.push_str(" — ");
            f.push_str(&self.author);
        }
        if !self.copyright.is_empty() {
            f.push_str(" © ");
            f.push_str(&self.copyright);
        }
        f
    }

    pub fn column_count(&self) -> usize {
        self.columns.len().max(1)
    }

    /// Columns shown on slides; with no column selected the first one is shown.
    pub fn shown_columns(&self) -> Vec<usize> {
        let v: Vec<usize> = self.columns.iter().enumerate().filter(|(_, c)| c.show).map(|(i, _)| i).collect();
        if v.is_empty() { vec![0] } else { v }
    }

    /// Adds a text column; the first added column also names the existing text.
    pub fn add_column(&mut self, name: String, first_name: String) {
        if self.columns.is_empty() {
            self.columns.push(SongColumn { name: first_name, show: true });
        }
        self.columns.push(SongColumn { name, show: true });
    }

    /// Removes a column and its texts. Removing column 0 makes column 1 the main text.
    pub fn remove_column(&mut self, column: usize) {
        if column >= self.column_count() {
            return;
        }
        for p in &mut self.parts {
            if column == 0 {
                p.text = if p.translations.is_empty() { String::new() } else { p.translations.remove(0) };
            } else if column - 1 < p.translations.len() {
                p.translations.remove(column - 1);
            }
        }
        if column < self.columns.len() {
            self.columns.remove(column);
        }
        if self.columns.len() == 1 {
            self.columns.clear();
        }
    }

    /// True if any part's text in any column contains `folded` (see [`crate::storage::fold`]).
    pub fn text_matches(&self, folded: &str) -> bool {
        self.parts.iter().any(|p| {
            crate::storage::fold(&p.text).contains(folded) || p.translations.iter().any(|t| crate::storage::fold(t).contains(folded))
        })
    }

    /// Next free number for a part with the given tag.
    pub fn next_number(&self, tag: &str) -> u32 {
        self.parts.iter().filter(|p| p.tag == tag).map(|p| p.number.max(1)).max().map(|n| n + 1).unwrap_or(1)
    }
}

// ---------------------------------------------------------------------------
// Bible passage attached to a service

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PassageVerse {
    pub book: String,
    pub chapter: u32,
    pub verse: u32,
    pub text: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Passage {
    pub reference: String,
    pub version: String,
    pub verses: Vec<PassageVerse>,
}

// ---------------------------------------------------------------------------
// Service (the whole presentation for an event)

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum ItemKind {
    Song(Song),
    Design(Design),
    Bible(Passage),
    /// Free text slides, split like song parts.
    Text {
        title: String,
        text: String,
    },
    /// A video file played on the outputs.
    Video(Video),
}

/// A video in the service. `media` and `poster` are names in the media store.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Video {
    pub title: String,
    pub media: String,
    /// A still picture of the video (PNG), shown in previews and before playing.
    #[serde(default)]
    pub poster: Option<String>,
    /// Length in seconds (0 = unknown).
    #[serde(default)]
    pub duration: f64,
    /// Start again at the end.
    #[serde(default)]
    pub looped: bool,
    /// Play without sound.
    #[serde(default)]
    pub muted: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ServiceItem {
    pub id: String,
    pub kind: ItemKind,
    #[serde(default)]
    pub notes: String,
}

impl ServiceItem {
    pub fn new(kind: ItemKind) -> Self {
        Self { id: make_id(), kind, notes: String::new() }
    }

    pub fn title(&self) -> String {
        match &self.kind {
            ItemKind::Song(s) => s.title.clone(),
            ItemKind::Design(d) => d.name.clone(),
            ItemKind::Bible(p) => format!("{} ({})", p.reference, p.version),
            ItemKind::Text { title, .. } => title.clone(),
            ItemKind::Video(v) => v.title.clone(),
        }
    }

    pub fn icon(&self) -> &'static str {
        match &self.kind {
            ItemKind::Song(_) => "♪",
            ItemKind::Design(_) => "▣",
            ItemKind::Bible(_) => "✝",
            ItemKind::Text { .. } => "¶",
            ItemKind::Video(_) => "🎞",
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Service {
    pub items: Vec<ServiceItem>,
}

/// Everything needed to show one slide; shown in previews, the output window and
/// sent over the network to canvas clients.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Scene {
    Blank,
    Canvas(CanvasDoc),
    Text(TextScene),
    Video(VideoScene),
}

/// A video on the outputs, with its playback state at the moment it was sent.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct VideoScene {
    pub media: String,
    #[serde(default)]
    pub poster: Option<String>,
    #[serde(default)]
    pub duration: f64,
    #[serde(default)]
    pub looped: bool,
    #[serde(default)]
    pub muted: bool,
    /// Where playback starts (seconds), or where it stands while paused.
    #[serde(default)]
    pub position: f64,
    #[serde(default)]
    pub paused: bool,
    /// Changes whenever playback is (re)started, paused or moved, so every output
    /// starts its player again from `position`.
    #[serde(default)]
    pub token: u64,
}

impl VideoScene {
    pub fn new(v: &Video) -> Self {
        Self {
            media: v.media.clone(),
            poster: v.poster.clone(),
            duration: v.duration,
            looped: v.looped,
            muted: v.muted,
            position: 0.0,
            paused: false,
            token: 0,
        }
    }

    /// The position `elapsed` seconds after this state was sent.
    pub fn position_after(&self, elapsed: f64) -> f64 {
        let pos = if self.paused { self.position } else { self.position + elapsed };
        if self.duration > 0.0 {
            if self.looped { pos % self.duration } else { pos.min(self.duration) }
        } else {
            pos
        }
    }

    /// Playing and past the end (not looped).
    pub fn ended_after(&self, elapsed: f64) -> bool {
        !self.paused && !self.looped && self.duration > 0.0 && self.position + elapsed >= self.duration
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TextScene {
    pub text: String,
    /// Texts of further columns (other languages), drawn next to / below `text`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub extra: Vec<String>,
    #[serde(default)]
    pub layout: ColumnLayout,
    pub footer: String,
    pub style: TextStyle,
    /// Optional design drawn behind the text.
    pub background: Option<CanvasDoc>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TextStyle {
    pub background: Rgba,
    pub color: Rgba,
    pub max_font_size: f32,
    pub footer_size: f32,
    pub show_footer: bool,
    pub shadow: bool,
}

impl Default for TextStyle {
    fn default() -> Self {
        Self { background: Rgba::BLACK, color: Rgba::WHITE, max_font_size: 64.0, footer_size: 20.0, show_footer: true, shadow: true }
    }
}

impl Scene {
    pub fn media(&self) -> Vec<String> {
        match self {
            Scene::Blank => vec![],
            Scene::Canvas(doc) => doc.media().map(String::from).collect(),
            Scene::Text(t) => t.background.iter().flat_map(|d| d.media().map(String::from)).collect(),
            // the poster first: it is shown while the (large) video is still on its way
            Scene::Video(v) => v.poster.iter().cloned().chain(std::iter::once(v.media.clone())).collect(),
        }
    }
}

/// A slide of a service item, ready to display.
#[derive(Clone, Debug)]
pub struct Slide {
    /// Part code for songs (`V1`, `C`...), verse reference for bibles.
    pub label: String,
    /// Slide mark tag (songs only), used for jump shortcuts.
    pub tag: Option<String>,
    pub number: u32,
    pub scene: Scene,
}

// ---------------------------------------------------------------------------
// Alerts (a message running across the output over whatever is shown)

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum AlertDirection {
    /// Enters on the right, runs to the left.
    #[default]
    RightToLeft,
    LeftToRight,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum VAlign {
    Top,
    Middle,
    #[default]
    Bottom,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum AlertFont {
    #[default]
    Sans,
    Serif,
    Monospace,
}

/// Look of alerts (Settings → Alerts).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AlertStyle {
    /// Font size in canvas points (the canvas is 1280×720).
    pub font_size: f32,
    pub color: Rgba,
    pub font: AlertFont,
    /// The strip under the text (may be transparent).
    pub background: Rgba,
    pub valign: VAlign,
    pub direction: AlertDirection,
    /// Canvas points per second.
    pub speed: f32,
    /// Seconds the alert runs; 0 = until stopped. It always runs at least once across.
    pub timeout: f32,
}

impl Default for AlertStyle {
    fn default() -> Self {
        Self {
            font_size: 44.0,
            color: Rgba::WHITE,
            font: AlertFont::Sans,
            background: Rgba([160, 0, 0, 220]),
            valign: VAlign::Bottom,
            direction: AlertDirection::RightToLeft,
            speed: 220.0,
            timeout: 15.0,
        }
    }
}

/// An alert as sent to the outputs.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Alert {
    pub text: String,
    pub style: AlertStyle,
    /// How long it runs, in seconds from when an output receives it; 0 = until stopped.
    pub duration: f32,
}
