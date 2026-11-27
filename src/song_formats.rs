//! Song formats without files: OpenLyrics XML, ChordPro and plain text (to and from text).
//! Shared by the program ([`crate::formats`] re-exports it) and the web edition (`web/`).

use crate::model::*;
use crate::storage;
use std::path::Path;

pub type Res<T> = Result<T, String>;

pub(crate) fn err<E: std::fmt::Display>(e: E) -> String {
    e.to_string()
}

pub(crate) fn base_name(name: &str) -> &str {
    name.rsplit(['/', '\\']).next().unwrap_or(name)
}

pub(crate) fn ext_of(name: &str) -> String {
    Path::new(name).extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default()
}

// ---------------------------------------------------------------------------
// Part names ("v1", "c", "b2", custom) <-> (tag, number)

pub(crate) fn split_part_name(name: &str) -> (String, u32) {
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

pub(crate) fn part_name(p: &SongPart) -> String {
    let n = if p.number == 0 { 1 } else { p.number };
    format!("{}{}", p.tag.to_lowercase(), n)
}

/// Converts an order like `v1 c1 v2` into our part codes using the song's parts.
pub(crate) fn convert_order(order: &str, parts: &[SongPart]) -> String {
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

pub(crate) fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

/// Seconds since 1970 (UTC).
#[cfg(not(target_arch = "wasm32"))]
fn unix_secs() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0) as i64
}

/// In the browser (web edition) `std` has no clock: the page's.
#[cfg(target_arch = "wasm32")]
fn unix_secs() -> i64 {
    (js_sys::Date::now() / 1000.0) as i64
}

pub(crate) fn now_iso() -> String {
    // seconds precision UTC timestamp without pulling in a date crate
    let secs = unix_secs();
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

pub(crate) fn lines_text(lines: roxmltree::Node) -> String {
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

pub(crate) fn strip_chords(line: &str) -> String {
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
