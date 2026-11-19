//! Import and export of songs, designs and services.
//!
//! * own formats: `.ohsvc` service (zip: `service.json` + `media/*`), `.ohdesign`,
//!   `.ohbackup` (zip of the whole library)
//! * OpenLP: OpenLyrics XML songs, `.osz` service files, `songs.sqlite` database
//! * ChordPro and plain-text songs
//! * legacy oh-presenter files: `.ohpres`, `.ohdata`, `.ohcanvas`

use crate::model::*;
use crate::storage::{self, paths, store_media_bytes};
use serde_json::{Value, json};
use std::io::{Read, Write};
use std::path::Path;

pub type Res<T> = Result<T, String>;

fn err<E: std::fmt::Display>(e: E) -> String {
    e.to_string()
}

fn read_zip(path: &Path) -> Res<Vec<(String, Vec<u8>)>> {
    let file = std::fs::File::open(path).map_err(err)?;
    let mut zip = zip::ZipArchive::new(file).map_err(err)?;
    let mut out = Vec::new();
    for i in 0..zip.len() {
        let mut f = zip.by_index(i).map_err(err)?;
        if f.is_dir() {
            continue;
        }
        let name = f.name().map_err(err)?.to_string();
        let mut buf = Vec::new();
        f.read_to_end(&mut buf).map_err(err)?;
        out.push((name, buf));
    }
    Ok(out)
}

fn write_zip(path: &Path, entries: &[(String, Vec<u8>)]) -> Res<()> {
    let mut buf = std::io::Cursor::new(Vec::new());
    {
        let mut zip = zip::ZipWriter::new(&mut buf);
        let opts = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
        for (name, bytes) in entries {
            zip.start_file(name.as_str(), opts).map_err(err)?;
            zip.write_all(bytes).map_err(err)?;
        }
        zip.finish().map_err(err)?;
    }
    storage::write_atomic(path, &buf.into_inner(), true).map_err(err)
}

fn base_name(name: &str) -> &str {
    name.rsplit(['/', '\\']).next().unwrap_or(name)
}

fn ext_of(name: &str) -> String {
    Path::new(name).extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default()
}

// ---------------------------------------------------------------------------
// Part names ("v1", "c", "b2", custom) <-> (tag, number)

fn split_part_name(name: &str) -> (String, u32) {
    let name = name.trim();
    let letters: String = name.chars().take_while(|c| !c.is_ascii_digit()).collect();
    let number = name[letters.len()..].trim().parse().unwrap_or(0);
    let letters = letters.trim().to_uppercase();
    let tag = match letters.as_str() {
        "" => "V".to_string(),
        l if l.len() == 1 && "VCBPIEO".contains(l) => l.to_string(),
        "VERSE" => "V".into(),
        "CHORUS" | "REFRAIN" => "C".into(),
        "BRIDGE" => "B".into(),
        "PRECHORUS" | "PRE-CHORUS" => "P".into(),
        "INTRO" => "I".into(),
        "ENDING" | "OUTRO" => "E".into(),
        l => l.to_string(),
    };
    let number = if tag == "V" && number == 0 { 1 } else { number };
    (tag, number)
}

fn part_name(p: &SongPart) -> String {
    let n = if p.number == 0 { 1 } else { p.number };
    format!("{}{}", p.tag.to_lowercase(), n)
}

/// Converts an order like `v1 c1 v2` into our part codes using the song's parts.
fn convert_order(order: &str, parts: &[SongPart]) -> String {
    order
        .split_whitespace()
        .filter_map(|tok| {
            let (tag, number) = split_part_name(tok);
            parts.iter().find(|p| p.tag == tag && (p.number == number || (number <= 1 && p.number <= 1))).map(|p| p.code())
        })
        .collect::<Vec<_>>()
        .join(" ")
}

// ---------------------------------------------------------------------------
// OpenLyrics

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

fn now_iso() -> String {
    // seconds precision UTC timestamp without pulling in a date crate
    let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0) as i64;
    let days = secs.div_euclid(86400);
    let rem = secs.rem_euclid(86400);
    // civil from days (Howard Hinnant)
    let z = days + 719468;
    let era = z.div_euclid(146097);
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!("{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}", rem / 3600, rem % 3600 / 60, rem % 60)
}

pub fn song_to_openlyrics(song: &Song) -> String {
    let mut x = String::new();
    x.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
    x.push_str(&format!(
        "<song xmlns=\"http://openlyrics.info/namespace/2009/song\" version=\"0.9\" createdIn=\"oh-presenter-rs {v}\" modifiedIn=\"oh-presenter-rs {v}\" modifiedDate=\"{d}\">\n",
        v = env!("CARGO_PKG_VERSION"),
        d = now_iso()
    ));
    x.push_str("  <properties>\n    <titles>\n");
    x.push_str(&format!("      <title>{}</title>\n", xml_escape(&song.title)));
    x.push_str("    </titles>\n");
    if !song.author.is_empty() {
        x.push_str("    <authors>\n");
        for a in song.author.split([',', ';']).map(str::trim).filter(|a| !a.is_empty()) {
            x.push_str(&format!("      <author>{}</author>\n", xml_escape(a)));
        }
        x.push_str("    </authors>\n");
    }
    if !song.copyright.is_empty() {
        x.push_str(&format!("    <copyright>{}</copyright>\n", xml_escape(&song.copyright)));
    }
    if !song.ccli.is_empty() {
        x.push_str(&format!("    <ccliNo>{}</ccliNo>\n", xml_escape(&song.ccli)));
    }
    if !song.order.trim().is_empty() {
        let order: Vec<String> = song
            .order
            .split_whitespace()
            .filter_map(|c| song.parts.iter().find(|p| p.code().eq_ignore_ascii_case(c)).map(part_name))
            .collect();
        x.push_str(&format!("    <verseOrder>{}</verseOrder>\n", order.join(" ")));
    }
    x.push_str("  </properties>\n  <lyrics>\n");
    for p in &song.parts {
        // every text column (language) is its own <verse lang="..."> with the same name
        for c in 0..song.column_count() {
            let text = p.column_text(c);
            let lang = match song.columns.get(c) {
                Some(col) => format!(" lang=\"{}\"", xml_escape(&col.name)),
                None => String::new(),
            };
            if c > 0 && text.trim().is_empty() {
                continue;
            }
            x.push_str(&format!("    <verse name=\"{}\"{lang}>\n", xml_escape(&part_name(p))));
            let pages = split_pages(text);
            for page in if pages.is_empty() { vec![String::new()] } else { pages } {
                let lines: Vec<String> = page.lines().map(xml_escape).collect();
                x.push_str(&format!("      <lines>{}</lines>\n", lines.join("<br/>")));
            }
            x.push_str("    </verse>\n");
        }
    }
    x.push_str("  </lyrics>\n</song>\n");
    x
}

fn lines_text(lines: roxmltree::Node) -> String {
    // OpenLyrics 0.7 uses <line> children, 0.8+ uses text with <br/>; chords and comments are dropped.
    let mut s = String::new();
    fn walk(n: roxmltree::Node, s: &mut String) {
        for c in n.children() {
            if c.is_text() {
                let t = c.text().unwrap_or("");
                // whitespace between elements is formatting only
                if !t.trim().is_empty() {
                    s.push_str(&t.replace(['\n', '\r'], " ").replace("  ", " "));
                }
            } else if c.is_element() {
                match c.tag_name().name() {
                    "br" => s.push('\n'),
                    "comment" => {}
                    "line" => {
                        if !s.is_empty() && !s.ends_with('\n') {
                            s.push('\n');
                        }
                        walk(c, s);
                        s.push('\n');
                    }
                    _ => walk(c, s),
                }
            }
        }
    }
    walk(lines, &mut s);
    s.lines().map(str::trim).collect::<Vec<_>>().join("\n").trim().to_string()
}

pub fn song_from_openlyrics(xml: &str) -> Res<Song> {
    let doc = roxmltree::Document::parse(xml).map_err(err)?;
    let root = doc.root_element();
    if root.tag_name().name() != "song" {
        return Err("not an OpenLyrics song".into());
    }
    let find = |name: &str| root.descendants().find(|n| n.is_element() && n.tag_name().name() == name);
    let text_of = |name: &str| find(name).and_then(|n| n.text()).unwrap_or("").trim().to_string();
    let mut song = Song::new();
    song.title = find("titles")
        .and_then(|t| t.children().find(|c| c.is_element() && c.tag_name().name() == "title"))
        .and_then(|t| t.text())
        .unwrap_or("")
        .trim()
        .to_string();
    song.author = find("authors")
        .map(|a| {
            a.children()
                .filter(|c| c.is_element() && c.tag_name().name() == "author")
                .filter_map(|c| c.text().map(|t| t.trim().to_string()))
                .collect::<Vec<_>>()
                .join(", ")
        })
        .unwrap_or_default();
    song.copyright = text_of("copyright");
    song.ccli = text_of("ccliNo");
    if let Some(lyrics) = find("lyrics") {
        let verses: Vec<_> = lyrics.children().filter(|c| c.is_element() && c.tag_name().name() == "verse").collect();
        // several languages: each `lang` becomes a text column, in order of appearance
        let mut langs: Vec<String> = Vec::new();
        for v in &verses {
            let l = v.attribute("lang").unwrap_or("").to_string();
            if !langs.contains(&l) {
                langs.push(l);
            }
        }
        if langs.len() > 1 {
            song.columns =
                langs.iter().map(|l| SongColumn { name: if l.is_empty() { "?".into() } else { l.clone() }, show: true }).collect();
        }
        for v in verses {
            let (tag, number) = split_part_name(v.attribute("name").unwrap_or("v"));
            let column = langs.iter().position(|l| l == v.attribute("lang").unwrap_or("")).unwrap_or(0);
            let pages: Vec<String> = v.children().filter(|c| c.is_element() && c.tag_name().name() == "lines").map(lines_text).collect();
            let text = pages.join("\n---\n");
            let idx = match song.parts.iter().position(|p| p.tag == tag && p.number == number) {
                Some(i) => i,
                None => {
                    song.parts.push(SongPart { tag, number, text: String::new(), translations: Vec::new() });
                    song.parts.len() - 1
                }
            };
            // OpenLyrics may split one verse into several <verse> elements with the same name
            let target = song.parts[idx].column_text_mut(column);
            if !target.is_empty() {
                target.push_str("\n---\n");
            }
            target.push_str(&text);
        }
    }
    song.order = convert_order(&text_of("verseOrder"), &song.parts);
    if song.title.is_empty() {
        song.title = song.parts.first().and_then(|p| p.text.lines().next()).unwrap_or("?").to_string();
    }
    Ok(song)
}

// ---------------------------------------------------------------------------
// ChordPro and plain text

fn strip_chords(line: &str) -> String {
    let mut out = String::new();
    let mut depth = 0;
    for c in line.chars() {
        match c {
            '[' => depth += 1,
            ']' if depth > 0 => depth -= 1,
            _ if depth == 0 => out.push(c),
            _ => {}
        }
    }
    out.trim_end().to_string()
}

pub fn song_from_chordpro(text: &str, fallback_title: &str) -> Song {
    let mut song = Song::new();
    let mut section: Option<String> = None;
    let mut buf: Vec<String> = Vec::new();
    let flush = |song: &mut Song, tag: &str, buf: &mut Vec<String>| {
        let t = buf.join("\n").trim().to_string();
        buf.clear();
        if !t.is_empty() {
            let number = song.next_number(tag);
            let number = if tag != "V" && number == 1 { 0 } else { number };
            song.parts.push(SongPart { tag: tag.into(), number, text: t, translations: Vec::new() });
        }
    };
    for raw in text.lines() {
        let line = raw.trim();
        if line.starts_with('#') {
            continue;
        }
        if line.starts_with('{') && line.ends_with('}') {
            let inner = &line[1..line.len() - 1];
            let (dir, val) = inner.split_once(':').map(|(a, b)| (a.trim(), b.trim())).unwrap_or((inner.trim(), ""));
            match dir.to_lowercase().as_str() {
                "title" | "t" => song.title = val.into(),
                "artist" | "composer" | "lyricist" | "subtitle" | "st" if song.author.is_empty() => song.author = val.into(),
                "copyright" => song.copyright = val.into(),
                "ccli" => song.ccli = val.into(),
                d @ ("start_of_chorus" | "soc" | "start_of_verse" | "sov" | "start_of_bridge" | "sob") => {
                    flush(&mut song, section.as_deref().unwrap_or("V"), &mut buf);
                    section = Some(
                        match d {
                            "start_of_chorus" | "soc" => "C",
                            "start_of_bridge" | "sob" => "B",
                            _ => "V",
                        }
                        .into(),
                    );
                }
                "end_of_chorus" | "eoc" | "end_of_verse" | "eov" | "end_of_bridge" | "eob" => {
                    flush(&mut song, section.as_deref().unwrap_or("V"), &mut buf);
                    section = None;
                }
                _ => {}
            }
            continue;
        }
        if line.is_empty() {
            if section.is_none() {
                flush(&mut song, "V", &mut buf);
            }
            continue;
        }
        buf.push(strip_chords(raw));
    }
    flush(&mut song, section.as_deref().unwrap_or("V"), &mut buf);
    if song.title.is_empty() {
        song.title = fallback_title.into();
    }
    song
}

/// Plain text: blocks separated by empty lines. A block starting with a label such as
/// `R:`, `Ref.:`, `Refrén`, `Chorus`, `Bridge:`, `1.`, `2)` or `V3` sets the part type.
pub fn song_from_text(text: &str, fallback_title: &str) -> Song {
    let mut song = Song::new();
    song.title = fallback_title.to_string();
    let mut blocks: Vec<Vec<&str>> = vec![vec![]];
    for line in text.lines() {
        if line.trim().is_empty() {
            if !blocks.last().unwrap().is_empty() {
                blocks.push(vec![]);
            }
        } else {
            blocks.last_mut().unwrap().push(line.trim_end());
        }
    }
    for block in blocks.into_iter().filter(|b| !b.is_empty()) {
        let first = block[0].trim();
        let folded = storage::fold(first);
        let (tag, rest): (&str, String) = {
            let label_end = first.find([':', '.', ')']).map(|i| i + 1);
            let label = label_end.map(|i| storage::fold(&first[..i])).unwrap_or_default();
            let lbl = label.trim_end_matches([':', '.', ')']).trim();
            let rest_of_line = label_end.map(|i| first[i..].trim().to_string()).unwrap_or_default();
            if ["r", "ref", "refren", "chorus", "c"].contains(&lbl) {
                ("C", rest_of_line)
            } else if ["b", "bridge", "most"].contains(&lbl) {
                ("B", rest_of_line)
            } else if ["intro"].contains(&lbl) {
                ("I", rest_of_line)
            } else if ["outro", "ending", "zaver", "coda"].contains(&lbl) {
                ("E", rest_of_line)
            } else if !lbl.is_empty() && lbl.trim_start_matches('v').chars().all(|c| c.is_ascii_digit()) && lbl.len() <= 3 {
                ("V", rest_of_line)
            } else if ["refren", "chorus"].contains(&folded.as_str()) {
                ("C", String::new())
            } else {
                ("", first.to_string())
            }
        };
        let tag_used = if tag.is_empty() { "V" } else { tag };
        let mut lines: Vec<String> = Vec::new();
        if !rest.is_empty() {
            lines.push(rest);
        }
        lines.extend(block[1..].iter().map(|l| l.to_string()));
        let text = lines.join("\n");
        if text.trim().is_empty() {
            continue;
        }
        let number = song.next_number(tag_used);
        let number = if tag_used != "V" && number == 1 { 0 } else { number };
        song.parts.push(SongPart { tag: tag_used.into(), number, text, translations: Vec::new() });
    }
    song
}

/// Imports a song file, choosing the format by extension / content.
pub fn import_song_file(path: &Path) -> Res<Vec<Song>> {
    let ext = ext_of(&path.to_string_lossy());
    let stem = path.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
    match ext.as_str() {
        "sqlite" | "db" => import_openlp_songs_db(path),
        "ohdata" => import_ohdata(path),
        _ => {
            let bytes = std::fs::read(path).map_err(err)?;
            let text = String::from_utf8_lossy(&bytes).trim_start_matches('\u{feff}').to_string();
            if text.trim_start().starts_with('<') {
                Ok(vec![song_from_openlyrics(&text)?])
            } else if matches!(ext.as_str(), "cho" | "chordpro" | "chopro" | "crd" | "pro")
                || text.contains("{title")
                || text.contains("{t:")
            {
                Ok(vec![song_from_chordpro(&text, &stem)])
            } else {
                Ok(vec![song_from_text(&text, &stem)])
            }
        }
    }
}

/// Imports all songs from an OpenLP `songs.sqlite` database.
pub fn import_openlp_songs_db(path: &Path) -> Res<Vec<Song>> {
    let conn = rusqlite::Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY).map_err(err)?;
    let mut stmt = conn
        .prepare(
            "SELECT id, title, lyrics, verse_order, copyright, ccli_number FROM songs WHERE \"temporary\" IS NULL OR \"temporary\" = 0",
        )
        .or_else(|_| conn.prepare("SELECT id, title, lyrics, verse_order, copyright, ccli_number FROM songs"))
        .map_err(err)?;
    let rows: Vec<(i64, String, String, String, String, String)> = stmt
        .query_map([], |r| {
            Ok((
                r.get(0)?,
                r.get::<_, Option<String>>(1)?.unwrap_or_default(),
                r.get::<_, Option<String>>(2)?.unwrap_or_default(),
                r.get::<_, Option<String>>(3)?.unwrap_or_default(),
                r.get::<_, Option<String>>(4)?.unwrap_or_default(),
                r.get::<_, Option<String>>(5)?.unwrap_or_default(),
            ))
        })
        .map_err(err)?
        .flatten()
        .collect();
    let mut authors_stmt = conn
        .prepare("SELECT a.display_name FROM authors a JOIN authors_songs s ON s.author_id = a.id WHERE s.song_id = ?1")
        .map_err(err)?;
    let mut out = Vec::new();
    for (id, title, lyrics, order, copyright, ccli) in rows {
        let mut song = Song::new();
        song.title = title;
        song.copyright = copyright;
        song.ccli = ccli;
        song.author = authors_stmt
            .query_map([id], |r| r.get::<_, String>(0))
            .map(|it| it.flatten().collect::<Vec<_>>().join(", "))
            .unwrap_or_default();
        // <song version="1.0"><lyrics><verse type="v" label="1"><![CDATA[...]]></verse>...
        if let Ok(doc) = roxmltree::Document::parse(&lyrics) {
            for v in doc.descendants().filter(|n| n.tag_name().name() == "verse") {
                let (tag, _) = split_part_name(v.attribute("type").unwrap_or("v"));
                let number = v.attribute("label").and_then(|l| l.parse().ok()).unwrap_or(1);
                let number = if tag != "V" && number == 1 { 0 } else { number };
                let text: String = v.children().filter_map(|c| c.text()).collect::<String>().replace("[---]", "\n---\n");
                song.parts.push(SongPart { tag, number, text: text.trim().to_string(), translations: Vec::new() });
            }
        } else if lyrics.contains("<song") {
            // OpenLyrics stored directly
            if let Ok(s) = song_from_openlyrics(&lyrics) {
                song.parts = s.parts;
            }
        }
        song.order = convert_order(&order, &song.parts);
        out.push(song);
    }
    Ok(out)
}

// ---------------------------------------------------------------------------
// Legacy oh-presenter files

/// `.ohdata`: `[{id, name, data: [{id, name, value}]}]` — each data set becomes a song.
pub fn import_ohdata(path: &Path) -> Res<Vec<Song>> {
    let v: Value = serde_json::from_slice(&std::fs::read(path).map_err(err)?).map_err(err)?;
    Ok(songs_from_datasets(&v))
}

fn songs_from_datasets(v: &Value) -> Vec<Song> {
    let mut out = Vec::new();
    for set in v.as_array().into_iter().flatten() {
        let mut song = Song::new();
        song.title = set["name"].as_str().unwrap_or("?").to_string();
        for item in set["data"].as_array().into_iter().flatten() {
            let Some(text) = item["value"].as_str() else { continue };
            let name = item["name"].as_str().unwrap_or("");
            let (mut tag, mut number) = split_part_name(name);
            if tag.len() > 1 {
                // free form names like "sloka" or "refrén"
                let f = storage::fold(name);
                tag = if f.starts_with("ref") || f.starts_with("chor") { "C".into() } else { "V".into() };
                number = 0;
            }
            if number == 0 {
                number = song.next_number(&tag);
                if tag != "V" && number == 1 {
                    number = 0;
                }
            }
            song.parts.push(SongPart { tag, number, text: text.to_string(), translations: Vec::new() });
        }
        out.push(song);
    }
    out
}

/// Converts a fabric.js canvas (oh-presenter's `.ohcanvas` / slides) into a design.
/// `image_src` maps an image `src` to a media name.
fn canvas_from_fabric(v: &Value, image_src: &mut dyn FnMut(&str) -> Option<String>) -> CanvasDoc {
    let mut doc = CanvasDoc::default();
    if let Some(bg) = v["background"].as_str().and_then(Rgba::parse) {
        doc.background = bg;
    }
    for o in v["objects"].as_array().into_iter().flatten() {
        let f = |k: &str, d: f64| o[k].as_f64().unwrap_or(d) as f32;
        let (sx, sy) = (f("scaleX", 1.0), f("scaleY", 1.0));
        let mut w = f("width", 100.0) * sx;
        let mut h = f("height", 100.0) * sy;
        let ty = o["type"].as_str().unwrap_or("");
        if ty == "circle" {
            let r = f("radius", 50.0);
            w = r * 2.0 * sx;
            h = r * 2.0 * sy;
        }
        let mut x = f("left", 0.0);
        let mut y = f("top", 0.0);
        if o["originX"].as_str() == Some("center") {
            x -= w / 2.0;
        }
        if o["originY"].as_str() == Some("center") {
            y -= h / 2.0;
        }
        let color = |k: &str| o[k].as_str().and_then(Rgba::parse).unwrap_or(Rgba::TRANSPARENT);
        let stroke_width = if o["stroke"].is_null() { 0.0 } else { f("strokeWidth", 0.0) };
        let kind = match ty {
            "rect" => ObjectKind::Rect { fill: color("fill"), stroke: color("stroke"), stroke_width, corner_radius: f("rx", 0.0) },
            "circle" | "ellipse" => ObjectKind::Ellipse { fill: color("fill"), stroke: color("stroke"), stroke_width },
            "i-text" | "text" | "textbox" => {
                let fill = o["fill"].as_str().and_then(Rgba::parse).unwrap_or(Rgba::BLACK);
                let align = match o["textAlign"].as_str() {
                    Some("center") => HAlign::Center,
                    Some("right") => HAlign::Right,
                    _ => HAlign::Left,
                };
                ObjectKind::Text {
                    text: o["text"].as_str().unwrap_or("").to_string(),
                    font_size: f("fontSize", 40.0) * sy,
                    color: fill,
                    align,
                    font: FontStyle::Proportional,
                }
            }
            "image" | "video" => match o["src"].as_str().and_then(&mut *image_src) {
                Some(media) => ObjectKind::Image { media },
                None => continue,
            },
            _ => continue,
        };
        doc.objects.push(CanvasObject { id: o["id"].as_str().map(String::from).unwrap_or_else(make_id), x, y, w, h, kind });
    }
    doc
}

/// Images in old canvases point to oh-presenter's local cache web server; map them back to the cache folder.
fn legacy_cache_image(src: &str) -> Option<String> {
    let home = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")).map(std::path::PathBuf::from)?;
    let name = base_name(src);
    let path = home.join(".oh-presenter/cache/files").join(name);
    let bytes = std::fs::read(path).ok()?;
    // cached files lost their extension ("photo_jpg"), guess it from the name
    let ext = name.rsplit('_').next().unwrap_or("png");
    store_media_bytes(&bytes, ext).ok()
}

pub fn import_ohcanvas(path: &Path) -> Res<Design> {
    let v: Value = serde_json::from_slice(&std::fs::read(path).map_err(err)?).map_err(err)?;
    let doc = canvas_from_fabric(&v, &mut |src| legacy_cache_image(src));
    Ok(Design { id: make_id(), name: path.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default(), doc })
}

/// `.ohpres`: tar with `slides.json` and the images it references as `presentation://<file>`.
pub fn import_ohpres(path: &Path) -> Res<Service> {
    let mut files: std::collections::HashMap<String, Vec<u8>> = Default::default();
    let mut archive = tar::Archive::new(std::fs::File::open(path).map_err(err)?);
    for entry in archive.entries().map_err(err)? {
        let mut e = entry.map_err(err)?;
        let name = e.path().map_err(err)?.to_string_lossy().to_string();
        let mut buf = Vec::new();
        e.read_to_end(&mut buf).map_err(err)?;
        files.insert(base_name(&name).to_string(), buf);
    }
    let slides: Value = serde_json::from_slice(files.get("slides.json").ok_or("slides.json missing")?).map_err(err)?;
    let mut service = Service::default();
    for s in slides.as_array().into_iter().flatten() {
        let name = s["name"].as_str().unwrap_or("Slide").to_string();
        match s["type"].as_str() {
            Some("lyrics") => {
                for song in songs_from_datasets(&s["data"]) {
                    service.items.push(ServiceItem::new(ItemKind::Song(song)));
                }
            }
            _ => {
                let doc = canvas_from_fabric(&s["jsonCanvasData"], &mut |src| {
                    let key = src.trim_start_matches("presentation://");
                    let bytes = files.get(key)?;
                    let ext = image::guess_format(bytes).ok().and_then(|f| f.extensions_str().first().copied()).unwrap_or("png");
                    store_media_bytes(bytes, ext).ok()
                });
                service.items.push(ServiceItem::new(ItemKind::Design(Design { id: make_id(), name, doc })));
            }
        }
    }
    Ok(service)
}

/// File extensions written by the old oh-presenter.
pub const LEGACY_EXTS: &[&str] = &["ohdata", "ohcanvas", "ohpres"];

/// What was found of an old oh-presenter installation.
#[derive(Debug, Default)]
pub struct LegacyScan {
    /// `~/.oh-presenter` (the old app's cache), if it exists.
    pub old_app_dir: Option<std::path::PathBuf>,
    /// Old files, newest first.
    pub files: Vec<std::path::PathBuf>,
}

/// Looks for files of the old oh-presenter below `home`. The old app kept no library of its
/// own, only the files the user saved, so they have to be searched for. Skips hidden and
/// build folders, does not follow symbolic links and stops after `budget` folder entries.
pub fn scan_legacy(home: &Path, max_depth: usize, budget: usize) -> LegacyScan {
    const SKIP: &[&str] = &["node_modules", "target", "AppData", "Library", "snap", "venv", "__pycache__"];
    let old = home.join(".oh-presenter");
    let mut scan = LegacyScan { old_app_dir: old.is_dir().then_some(old), files: Vec::new() };
    let mut seen = 0usize;
    let mut queue = std::collections::VecDeque::from([(home.to_path_buf(), 0usize)]);
    'walk: while let Some((dir, depth)) = queue.pop_front() {
        let Ok(entries) = std::fs::read_dir(&dir) else { continue };
        for e in entries.flatten() {
            seen += 1;
            if seen > budget {
                break 'walk;
            }
            let Ok(ft) = e.file_type() else { continue };
            let name = e.file_name().to_string_lossy().to_string();
            if ft.is_dir() {
                if depth < max_depth && !name.starts_with('.') && !SKIP.contains(&name.as_str()) {
                    queue.push_back((e.path(), depth + 1));
                }
            } else if ft.is_file() && LEGACY_EXTS.contains(&ext_of(&name).as_str()) {
                scan.files.push(e.path());
            }
        }
    }
    let mtime = |p: &std::path::PathBuf| std::fs::metadata(p).and_then(|m| m.modified()).ok();
    scan.files.sort_by_key(|p| std::cmp::Reverse(mtime(p)));
    scan
}

// ---------------------------------------------------------------------------
// Own formats

pub const SERVICE_EXT: &str = "ohsvc";
pub const DESIGN_EXT: &str = "ohdesign";
pub const BACKUP_EXT: &str = "ohbackup";

fn service_media(service: &Service) -> Vec<String> {
    let mut media: Vec<String> = service
        .items
        .iter()
        .flat_map(|i| match &i.kind {
            ItemKind::Design(d) => d.doc.media().map(String::from).collect(),
            ItemKind::Video(v) => v.poster.iter().cloned().chain(std::iter::once(v.media.clone())).collect(),
            _ => vec![],
        })
        .collect();
    media.sort();
    media.dedup();
    media
}

pub fn save_service(path: &Path, service: &Service) -> Res<()> {
    let mut entries = vec![("service.json".to_string(), serde_json::to_vec_pretty(service).map_err(err)?)];
    for m in service_media(service) {
        if let Ok(bytes) = std::fs::read(storage::media_path(&m)) {
            entries.push((format!("media/{m}"), bytes));
        }
    }
    write_zip(path, &entries)
}

/// Loads a service file of any supported kind (`.ohsvc`, OpenLP `.osz`, legacy `.ohpres`).
pub fn load_service(path: &Path) -> Res<Service> {
    match ext_of(&path.to_string_lossy()).as_str() {
        "osz" | "oszl" => import_osz(path),
        "ohpres" => import_ohpres(path),
        _ => {
            let entries = read_zip(path)?;
            let mut service = None;
            for (name, bytes) in &entries {
                if name == "service.json" {
                    service = Some(serde_json::from_slice::<Service>(bytes).map_err(err)?);
                } else if let Some(m) = name.strip_prefix("media/") {
                    // media names are content hashes, so this never replaces a different file
                    let target = storage::media_path(m);
                    if !target.exists() {
                        storage::write_atomic(&target, bytes, false).map_err(err)?;
                    }
                }
            }
            service.ok_or_else(|| "service.json missing".into())
        }
    }
}

pub fn export_design(path: &Path, design: &Design) -> Res<()> {
    let mut entries = vec![("design.json".to_string(), serde_json::to_vec_pretty(design).map_err(err)?)];
    for m in design.doc.media() {
        if let Ok(bytes) = std::fs::read(storage::media_path(m)) {
            entries.push((format!("media/{m}"), bytes));
        }
    }
    write_zip(path, &entries)
}

pub fn import_design_file(path: &Path) -> Res<Design> {
    if ext_of(&path.to_string_lossy()) == "ohcanvas" {
        return import_ohcanvas(path);
    }
    let mut design = None;
    for (name, bytes) in read_zip(path)? {
        if name == "design.json" {
            design = Some(serde_json::from_slice::<Design>(&bytes).map_err(err)?);
        } else if let Some(m) = name.strip_prefix("media/") {
            let target = storage::media_path(m);
            if !target.exists() {
                storage::write_atomic(&target, &bytes, false).map_err(err)?;
            }
        }
    }
    design.ok_or_else(|| "design.json missing".into())
}

/// Zip of the whole library: songs, designs, media, bibles and bookmarks.
pub fn export_backup(path: &Path) -> Res<usize> {
    let p = paths();
    let mut entries = Vec::new();
    for (dir, prefix) in [(p.songs(), "songs"), (p.designs(), "designs"), (p.media(), "media"), (p.bibles(), "bibles")] {
        for e in std::fs::read_dir(&dir).map_err(err)?.flatten() {
            if e.path().is_file() {
                entries.push((format!("{prefix}/{}", e.file_name().to_string_lossy()), std::fs::read(e.path()).map_err(err)?));
            }
        }
    }
    if let Ok(b) = std::fs::read(p.bookmarks_file()) {
        entries.push(("bible_bookmarks.json".into(), b));
    }
    let n = entries.len();
    write_zip(path, &entries)?;
    Ok(n)
}

#[derive(Default, Debug)]
pub struct BackupReport {
    pub songs: usize,
    pub designs: usize,
    pub bibles: usize,
    pub skipped: usize,
}

/// Merges a backup into the library. Nothing existing is replaced: entries that
/// differ from a local one with the same id are added as copies.
pub fn import_backup(path: &Path, lib: &mut storage::Library) -> Res<BackupReport> {
    let mut report = BackupReport::default();
    let bibles = crate::bible::list_bibles();
    for (name, bytes) in read_zip(path)? {
        if let Some(_f) = name.strip_prefix("songs/") {
            match serde_json::from_slice::<Song>(&bytes).map(|s| lib.import_song(s)) {
                Ok(Ok(true)) => report.songs += 1,
                Ok(Err(e)) => return Err(e.to_string()),
                _ => report.skipped += 1,
            }
        } else if name.starts_with("designs/") {
            match serde_json::from_slice::<Design>(&bytes).map(|d| lib.import_design(d)) {
                Ok(Ok(true)) => report.designs += 1,
                Ok(Err(e)) => return Err(e.to_string()),
                _ => report.skipped += 1,
            }
        } else if let Some(m) = name.strip_prefix("media/") {
            let target = storage::media_path(m);
            if !target.exists() {
                storage::write_atomic(&target, &bytes, false).map_err(err)?;
            }
        } else if let Some(f) = name.strip_prefix("bibles/") {
            let id = f.trim_end_matches(".json").trim_end_matches(".meta");
            if bibles.iter().any(|b| b.id == id) {
                if f.ends_with(".json") {
                    report.skipped += 1;
                }
                continue;
            }
            storage::write_atomic(&paths().bibles().join(f), &bytes, false).map_err(err)?;
            if f.ends_with(".json") {
                report.bibles += 1;
            }
        } else if name == "bible_bookmarks.json"
            && let Ok(imported) = serde_json::from_slice::<crate::bible::Bookmarks>(&bytes)
        {
            let mut local = crate::bible::Bookmarks::load();
            for p in imported.pins {
                if !local.is_pinned(&p.reference, &p.bible) {
                    local.pins.push(p);
                }
            }
            local.save();
        }
    }
    Ok(report)
}

// ---------------------------------------------------------------------------
// OpenLP service files (.osz)

pub fn import_osz(path: &Path) -> Res<Service> {
    let entries = read_zip(path)?;
    let (_, osj) = entries.iter().find(|(n, _)| n.ends_with(".osj")).ok_or("no .osj in service file")?;
    let items: Value = serde_json::from_slice(osj).map_err(err)?;
    let mut service = Service::default();
    let mut version = 2;
    for item in items.as_array().into_iter().flatten() {
        if let Some(core) = item.get("openlp_core") {
            version = core["openlp-servicefile-version"].as_i64().unwrap_or(2);
            continue;
        }
        let Some(si) = item.get("serviceitem") else { continue };
        let header = &si["header"];
        let data = &si["data"];
        let title = header["title"].as_str().unwrap_or("").to_string();
        let slides: Vec<(String, String)> = data
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|s| Some((s["raw_slide"].as_str()?.to_string(), s["verseTag"].as_str().unwrap_or("").to_string())))
            .collect();
        match header["name"].as_str().unwrap_or("") {
            "songs" => {
                let song = header["xml_version"]
                    .as_str()
                    .and_then(|x| song_from_openlyrics(x).ok().filter(|s| !s.parts.is_empty()))
                    .unwrap_or_else(|| {
                        let mut s = Song::new();
                        s.title = title.clone();
                        for (text, tag) in &slides {
                            let (tag, number) = split_part_name(tag);
                            let number = if tag != "V" && number == 1 { 0 } else { number };
                            s.parts.push(SongPart { tag, number, text: text.clone(), translations: Vec::new() });
                        }
                        s.order = s.parts.iter().map(|p| p.code()).collect::<Vec<_>>().join(" ");
                        s
                    });
                service.items.push(ServiceItem::new(ItemKind::Song(song)));
            }
            "bibles" => {
                let verses = slides
                    .iter()
                    .map(|(text, _)| PassageVerse { book: String::new(), chapter: 0, verse: 0, text: strip_tags(text) })
                    .collect();
                let footer = header["footer"].as_array().and_then(|f| f.get(1)).and_then(|v| v.as_str()).unwrap_or("");
                let version_name = footer.split(',').next().unwrap_or("").trim().to_string();
                let reference = title.split(" (").next().unwrap_or(&title).to_string();
                service.items.push(ServiceItem::new(ItemKind::Bible(Passage { reference, version: version_name, verses })));
            }
            "images" => {
                for img in data.as_array().into_iter().flatten() {
                    let (file_title, file_name) = if version >= 3 {
                        let t = img["title"].as_str().unwrap_or("");
                        (
                            t.to_string(),
                            format!(
                                "{}{}",
                                img["file_hash"].as_str().unwrap_or(""),
                                Path::new(t).extension().map(|e| format!(".{}", e.to_string_lossy())).unwrap_or_default()
                            ),
                        )
                    } else {
                        let t = img.as_str().unwrap_or("").to_string();
                        (t.clone(), t)
                    };
                    let Some((_, bytes)) = entries.iter().find(|(n, _)| base_name(n) == base_name(&file_name)) else { continue };
                    let media = store_media_bytes(bytes, &ext_of(&file_name)).map_err(err)?;
                    service.items.push(ServiceItem::new(ItemKind::Design(full_screen_image(&file_title, media, bytes))));
                }
            }
            _ => {
                // custom slides and anything else with text
                if !slides.is_empty() {
                    let text = slides.iter().map(|(t, _)| strip_tags(t)).collect::<Vec<_>>().join("\n---\n");
                    service.items.push(ServiceItem::new(ItemKind::Text { title, text }));
                }
            }
        }
        if let Some(last) = service.items.last_mut()
            && last.notes.is_empty()
        {
            last.notes = header["notes"].as_str().unwrap_or("").to_string();
        }
    }
    Ok(service)
}

/// A design showing an image scaled to fit the screen.
/// Pictures the built-in decoders cannot read; they are converted to PNG with ffmpeg.
pub const CONVERTED_IMAGE_EXTS: &[&str] = &["heic", "heif", "hif", "avif"];

/// Reads a picture to put into the media store: its bytes and file extension. Formats
/// the program cannot show itself (HEIC / HEIF from phones, AVIF) are converted to PNG.
pub fn read_image_file(path: &Path) -> Res<(Vec<u8>, String)> {
    let bytes = std::fs::read(path).map_err(err)?;
    let ext = path.extension().map(|e| e.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
    let decodable = image::ImageReader::new(std::io::Cursor::new(&bytes))
        .with_guessed_format()
        .ok()
        .is_some_and(|r| r.format().is_some() && r.into_dimensions().is_ok());
    if decodable {
        return Ok((bytes, ext));
    }
    if CONVERTED_IMAGE_EXTS.contains(&ext.as_str()) || crate::video::ffmpeg_available() {
        return crate::video::image_to_png(path).map(|png| (png, "png".to_string()));
    }
    Err(crate::i18n::t("image.unsupported"))
}

pub fn full_screen_image(name: &str, media: String, bytes: &[u8]) -> Design {
    let (w, h) = image::load_from_memory(bytes).map(|i| (i.width() as f32, i.height() as f32)).unwrap_or((CANVAS_W, CANVAS_H));
    let s = (CANVAS_W / w).min(CANVAS_H / h);
    let (w, h) = (w * s, h * s);
    Design {
        id: make_id(),
        name: name.to_string(),
        doc: CanvasDoc {
            background: Rgba::BLACK,
            objects: vec![CanvasObject {
                id: make_id(),
                x: (CANVAS_W - w) / 2.0,
                y: (CANVAS_H - h) / 2.0,
                w,
                h,
                kind: ObjectKind::Image { media },
            }],
        },
    }
}

/// Removes OpenLP formatting tags like `{su}` / `{/su}` and HTML.
fn strip_tags(s: &str) -> String {
    let mut out = String::new();
    let mut depth = 0;
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '{' if chars.peek().map(|n| n.is_ascii_alphabetic() || *n == '/').unwrap_or(false) => depth += 1,
            '}' if depth > 0 => depth -= 1,
            '<' => {
                for n in chars.by_ref() {
                    if n == '>' {
                        break;
                    }
                }
            }
            _ if depth == 0 => out.push(c),
            _ => {}
        }
    }
    out.trim().to_string()
}

/// Exports text items (songs, bible passages, text) as an OpenLP 3 service file.
/// Returns the titles of items that cannot be represented (designs).
pub fn export_osz(path: &Path, service: &Service) -> Res<Vec<String>> {
    let mut skipped = Vec::new();
    let mut items = vec![json!({"openlp_core": {"lite-service": false, "service-theme": null, "openlp-servicefile-version": 3}})];
    for item in &service.items {
        // (plugin name, footer, OpenLyrics xml, header data, slides as (text, verse tag))
        type OszItem<'a> = (&'a str, Vec<String>, Option<String>, Value, Vec<(String, String)>);
        let (name, footer, xml, data, slides): OszItem = match &item.kind {
            ItemKind::Song(song) => {
                let slides = song
                    .ordered_parts()
                    .iter()
                    .flat_map(|p| {
                        let tag = format!("{}{}", p.tag.chars().next().unwrap_or('O').to_ascii_uppercase(), p.number.max(1));
                        p.pages().into_iter().map(move |page| (page, tag.clone()))
                    })
                    .collect();
                (
                    "songs",
                    vec![song.title.clone(), song.author.clone(), song.copyright.clone()],
                    Some(song_to_openlyrics(song)),
                    json!({"title": format!("{}@", song.title.to_lowercase()), "authors": song.author}),
                    slides,
                )
            }
            ItemKind::Bible(p) => {
                let slides = p
                    .verses
                    .iter()
                    .map(|v| {
                        let text = if v.verse > 0 { format!("{{su}}{}{{/su}} {}", v.verse, v.text) } else { v.text.clone() };
                        (text, format!("{}:{}", v.chapter, v.verse))
                    })
                    .collect();
                ("bibles", vec![p.reference.clone(), p.version.clone()], None, json!(""), slides)
            }
            ItemKind::Text { title, text } => {
                let part = SongPart { tag: "O".into(), number: 1, text: text.clone(), translations: Vec::new() };
                let slides = part.pages().into_iter().enumerate().map(|(i, p)| (p, (i + 1).to_string())).collect();
                ("custom", vec![title.clone()], None, json!(""), slides)
            }
            ItemKind::Design(d) => {
                skipped.push(d.name.clone());
                continue;
            }
            ItemKind::Video(v) => {
                skipped.push(v.title.clone());
                continue;
            }
        };
        let data_slides: Vec<Value> = slides
            .iter()
            .map(|(text, tag)| json!({"raw_slide": text, "verseTag": tag, "title": text.lines().next().unwrap_or("").chars().take(30).collect::<String>()}))
            .collect();
        items.push(json!({"serviceitem": {
            "header": {
                "name": name, "plugin": name, "theme": null, "title": item.title(), "footer": footer,
                "type": 1, "audit": "", "notes": item.notes, "from_plugin": false,
                "capabilities": if name == "songs" { json!([2, 1, 5, 8, 9, 13]) } else { json!([1, 5, 14]) },
                "search": "", "data": data, "xml_version": xml,
                "auto_play_slides_once": false, "auto_play_slides_loop": false, "timed_slide_interval": 0,
                "start_time": 0, "end_time": 0, "media_length": 0, "background_audio": [],
                "theme_overwritten": false, "will_auto_start": false, "processor": null, "metadata": [],
                "sha256_file_hash": null, "stored_filename": null
            },
            "data": data_slides
        }}));
    }
    let osj = serde_json::to_vec(&items).map_err(err)?;
    write_zip(path, &[("service_data.osj".into(), osj)])?;
    Ok(skipped)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn openlyrics_roundtrip() {
        let mut s = Song::new();
        s.title = "Píseň & <test>".into();
        s.author = "A, B".into();
        s.parts = vec![
            SongPart { tag: "V".into(), number: 1, text: "line one\nline two\n---\npage two".into(), translations: Vec::new() },
            SongPart { tag: "C".into(), number: 0, text: "refrén".into(), translations: Vec::new() },
            SongPart { tag: "TAG".into(), number: 0, text: "custom".into(), translations: Vec::new() },
        ];
        s.order = "V1 C V1 C TAG".into();
        let xml = song_to_openlyrics(&s);
        let back = song_from_openlyrics(&xml).unwrap();
        assert_eq!(back.title, s.title);
        assert_eq!(back.author, "A, B");
        assert_eq!(back.parts[0].text, s.parts[0].text);
        assert_eq!(back.parts[1].code(), "C");
        assert_eq!(back.parts[2].tag, "TAG");
        assert_eq!(back.order, "V1 C V1 C TAG");
    }

    #[test]
    fn openlyrics_07_lines() {
        let xml = r#"<song xmlns="http://openlyrics.info/namespace/2009/song" version="0.7"><properties><titles><title>T</title></titles><verseOrder>v1 c1</verseOrder></properties>
        <lyrics><verse name="v1"><lines><line>a</line><line>b</line></lines></verse><verse name="c1"><lines><line>c</line></lines></verse></lyrics></song>"#;
        let s = song_from_openlyrics(xml).unwrap();
        assert_eq!(s.parts[0].text, "a\nb");
        assert_eq!(s.order, "V1 C");
    }

    #[test]
    fn chordpro() {
        let s = song_from_chordpro("{title: Song}\n{artist: Me}\n[C]Hello [G]world\nline\n\n{soc}\nChorus [Am]here\n{eoc}\n", "x");
        assert_eq!(s.title, "Song");
        assert_eq!(s.parts[0].text, "Hello world\nline");
        assert_eq!(s.parts[1].tag, "C");
    }

    #[test]
    fn plain_text() {
        let s = song_from_text("1. První sloka\nřádek\n\nR: Refrén\nřádek\n\n2. Druhá\n", "Název");
        assert_eq!(s.parts.len(), 3);
        assert_eq!(s.parts[0].code(), "V1");
        assert_eq!(s.parts[0].text, "První sloka\nřádek");
        assert_eq!(s.parts[1].code(), "C");
        assert_eq!(s.parts[2].code(), "V2");
    }

    #[test]
    fn fabric_canvas() {
        let v: Value = serde_json::from_str(r##"{"background":"#ff0000","objects":[{"type":"rect","left":10,"top":20,"width":50,"height":50,"scaleX":2,"scaleY":2,"fill":"#00ff00","stroke":"red","strokeWidth":3},{"type":"circle","left":100,"top":100,"radius":50,"fill":""},{"type":"i-text","left":5,"top":5,"text":"hi","fontSize":30,"fill":"#FF0000"}]}"##).unwrap();
        let doc = canvas_from_fabric(&v, &mut |_| None);
        assert_eq!(doc.background, Rgba([255, 0, 0, 255]));
        assert_eq!(doc.objects.len(), 3);
        assert_eq!(doc.objects[0].w, 100.0);
        assert_eq!(doc.objects[1].w, 100.0);
    }
}
