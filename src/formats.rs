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

pub use crate::song_formats::*;

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
                Some(media) => ObjectKind::Image { media, opacity: 1.0 },
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
                let verses = slides.iter().map(|(text, _)| PassageVerse { text: strip_tags(text), ..Default::default() }).collect();
                let footer = header["footer"].as_array().and_then(|f| f.get(1)).and_then(|v| v.as_str()).unwrap_or("");
                let version_name = footer.split(',').next().unwrap_or("").trim().to_string();
                let reference = title.split(" (").next().unwrap_or(&title).to_string();
                service.items.push(ServiceItem::new(ItemKind::Bible(Passage {
                    reference,
                    version: version_name,
                    verses,
                    ..Default::default()
                })));
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
    let img = image::load_from_memory(bytes).ok();
    let (w, h) = img.as_ref().map(|i| (i.width() as f32, i.height() as f32)).unwrap_or((CANVAS_W, CANVAS_H));
    // a picture with transparent parts gets a transparent background: shown as an overlay
    // (over what is behind a transparent output window), else on black
    let see_through = img.as_ref().is_some_and(|i| i.color().has_alpha() && i.to_rgba8().pixels().any(|p| p.0[3] < 255));
    let s = (CANVAS_W / w).min(CANVAS_H / h);
    let (w, h) = (w * s, h * s);
    Design {
        id: make_id(),
        name: name.to_string(),
        doc: CanvasDoc {
            background: if see_through { Rgba::TRANSPARENT } else { Rgba::BLACK },
            objects: vec![CanvasObject {
                id: make_id(),
                x: (CANVAS_W - w) / 2.0,
                y: (CANVAS_H - h) / 2.0,
                w,
                h,
                kind: ObjectKind::Image { media, opacity: 1.0 },
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
