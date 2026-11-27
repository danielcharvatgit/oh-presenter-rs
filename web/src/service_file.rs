//! Files the web edition opens and saves: `.ohsvc` services (a zip of `service.json` and
//! `media/*`, the program's format, so a service moves between the program and the browser),
//! songs (OpenLyrics, ChordPro, plain text) and bibles (Zefania, OSIS, OpenSong, plain text).

use crate::model::*;
use std::io::{Read, Write};

/// Pictures by media name: (name, file bytes).
pub type MediaFiles = Vec<(String, Vec<u8>)>;

/// A service and its pictures.
pub fn read_service(bytes: &[u8]) -> Result<(Service, MediaFiles), String> {
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes)).map_err(|e| e.to_string())?;
    let mut service = None;
    let mut media = Vec::new();
    for i in 0..zip.len() {
        let mut f = zip.by_index(i).map_err(|e| e.to_string())?;
        if f.is_dir() {
            continue;
        }
        let name = f.name().map_err(|e| e.to_string())?.to_string();
        let mut buf = Vec::new();
        f.read_to_end(&mut buf).map_err(|e| e.to_string())?;
        if name == "service.json" {
            service = Some(serde_json::from_slice::<Service>(&buf).map_err(|e| e.to_string())?);
        } else if let Some(m) = name.strip_prefix("media/")
            && !m.is_empty()
            && !m.contains('/')
        {
            media.push((m.to_string(), buf));
        }
    }
    Ok((service.ok_or("service.json missing")?, media))
}

/// The media names a service uses (pictures of designs and backgrounds, videos).
pub fn service_media(service: &Service) -> Vec<String> {
    let mut media: Vec<String> = service
        .items
        .iter()
        .flat_map(|i| {
            let mut m: Vec<String> = match &i.kind {
                ItemKind::Design(d) => d.doc.media().map(String::from).collect(),
                ItemKind::Video(v) => v.poster.iter().cloned().chain(std::iter::once(v.media.clone())).collect(),
                _ => vec![],
            };
            m.extend(i.background.iter().map(|b| b.media.clone()));
            m
        })
        .chain(service.background.iter().map(|b| b.media.clone()))
        .collect();
    media.sort();
    media.dedup();
    media
}

/// The `.ohsvc` file of a service with the pictures it uses (those in `media`).
pub fn write_service(service: &Service, media: &dyn Fn(&str) -> Option<std::rc::Rc<Vec<u8>>>) -> Result<Vec<u8>, String> {
    let mut buf = std::io::Cursor::new(Vec::new());
    {
        let mut zip = zip::ZipWriter::new(&mut buf);
        let opts = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
        let mut entries = vec![("service.json".to_string(), serde_json::to_vec_pretty(service).map_err(|e| e.to_string())?)];
        for m in service_media(service) {
            if let Some(bytes) = media(&m) {
                entries.push((format!("media/{m}"), bytes.to_vec()));
            }
        }
        for (name, bytes) in entries {
            zip.start_file(name.as_str(), opts).map_err(|e| e.to_string())?;
            zip.write_all(&bytes).map_err(|e| e.to_string())?;
        }
        zip.finish().map_err(|e| e.to_string())?;
    }
    Ok(buf.into_inner())
}

fn stem_and_ext(name: &str) -> (String, String) {
    match name.rsplit_once('.') {
        Some((s, e)) if !s.is_empty() => (s.to_string(), e.to_lowercase()),
        _ => (name.to_string(), String::new()),
    }
}

/// A song file: OpenLyrics XML, ChordPro or plain text (as the program imports them).
pub fn import_song(name: &str, bytes: &[u8]) -> Result<Song, String> {
    let (stem, ext) = stem_and_ext(name);
    let text = String::from_utf8_lossy(bytes).trim_start_matches('\u{feff}').to_string();
    if text.trim().is_empty() {
        return Err(crate::i18n::t("songs.empty_file"));
    }
    if text.trim_start().starts_with('<') {
        crate::song_formats::song_from_openlyrics(&text)
    } else if matches!(ext.as_str(), "cho" | "chordpro" | "chopro" | "crd" | "pro") || text.contains("{title") || text.contains("{t:") {
        Ok(crate::song_formats::song_from_chordpro(&text, &stem))
    } else {
        Ok(crate::song_formats::song_from_text(&text, &stem))
    }
}

/// A bible file (the program's import).
pub fn import_bible(name: &str, bytes: &[u8]) -> Result<crate::bible::Bible, String> {
    let (stem, ext) = stem_and_ext(name);
    if ext == "sqlite" || ext == "db" {
        return Err(crate::i18n::t("bible.no_sqlite"));
    }
    let mut b = crate::bible::import_bytes(bytes, &stem, ext == "txt")?;
    if b.books.is_empty() {
        return Err(crate::i18n::t("bible.no_verses"));
    }
    if b.meta.id.is_empty() {
        b.meta.id = make_id();
    }
    Ok(b)
}

/// A picture file as a slide covering the screen: its media name (by content) and the design.
pub fn image_design(name: &str, bytes: &[u8]) -> Result<(String, Design), String> {
    let img = image::load_from_memory(bytes).map_err(|e| e.to_string())?;
    let (stem, ext) = stem_and_ext(name);
    // the same content gets the same name
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in bytes {
        h ^= *b as u64;
        h = h.wrapping_mul(0x100_0000_01b3);
    }
    let media = format!("{h:016x}.{}", if ext.is_empty() { "png" } else { &ext });
    let (w, hgt) = (img.width().max(1) as f32, img.height().max(1) as f32);
    let s = (CANVAS_W / w).min(CANVAS_H / hgt);
    let (dw, dh) = (w * s, hgt * s);
    let transparent = img.color().has_alpha() && img.to_rgba8().pixels().any(|p| p.0[3] < 255);
    let doc = CanvasDoc {
        background: if transparent { Rgba([0, 0, 0, 0]) } else { Rgba::BLACK },
        objects: vec![CanvasObject {
            id: make_id(),
            x: (CANVAS_W - dw) / 2.0,
            y: (CANVAS_H - dh) / 2.0,
            w: dw,
            h: dh,
            kind: ObjectKind::Image { media: media.clone(), opacity: 1.0 },
        }],
    };
    Ok((media.clone(), Design { id: make_id(), name: stem, doc }))
}
