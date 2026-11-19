//! Tests against OpenLP's sample files (skipped when the OpenLP sources are not next to this crate).

use crate::{bible, formats, model::*};
use std::path::PathBuf;

fn openlp(rel: &str) -> Option<PathBuf> {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../OpenLP-3.1.7/tests/resources").join(rel);
    p.exists().then_some(p)
}

fn test_home() {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target/test-home");
    // SAFETY: tests only ever set this one variable, always to the same value
    unsafe { std::env::set_var("OH_PRESENTER_HOME", dir) };
}

#[test]
fn bibles_from_openlp_samples() {
    test_home();
    for (file, book, chapter, verse, needle) in [
        ("bibles/zefania-dk1933.xml", "1Mo 1:1", 1, 1, "Begyndelsen"),
        ("bibles/osis-kjv.xml", "Gen 1:1", 1, 1, "In the beginning"),
        ("bibles/opensong-dk1933.xml", "Gen 1:1", 1, 1, "Begyndelsen"),
        ("bibles/osis-web.xml", "Genesis 1:1", 1, 1, "beginning"),
    ] {
        let Some(path) = openlp(file) else { continue };
        let b = bible::import_file(&path).unwrap_or_else(|e| panic!("{file}: {e}"));
        assert!(b.verse_count() > 0, "{file}: no verses");
        let first = &b.books[0].chapters[0].verses[0];
        assert_eq!((b.books[0].chapters[0].number, first.number), (chapter, verse), "{file}");
        assert!(first.text.contains(needle), "{file}: {:?}", first.text);
        // Zefania files carry their own short names ("1Mo"), the others use the built-in table
        let r = b.parse_reference(book, "en");
        assert!(r.is_some(), "{file}: reference {book}");
        println!("{file}: {} '{}' {} verses", b.meta.name, b.meta.abbrev, b.verse_count());
    }
}

#[test]
fn osis_milestone_verses_do_not_leak_notes() {
    let Some(path) = openlp("bibles/osis-kjv.xml") else { return };
    let b = bible::import_file(&path).unwrap();
    let v = &b.books[0].chapters[0].verses[1];
    assert!(v.text.starts_with("And the earth was without form"), "{:?}", v.text);
}

#[test]
fn openlp_songs_database() {
    test_home();
    let Some(path) = openlp("songs/openlp/songs-2.4.6.sqlite") else { return };
    let songs = formats::import_openlp_songs_db(&path).unwrap();
    assert!(!songs.is_empty());
    let grace = songs.iter().find(|s| s.title == "Amazing Grace").expect("Amazing Grace");
    assert_eq!(grace.parts.len(), 6);
    assert_eq!(grace.order, "V1 V2 V3 V4 V5 V6");
    for s in &songs {
        println!("{} — {} — {} parts, order {:?}", s.title, s.author, s.parts.len(), s.order);
    }
}

#[test]
fn openlp_service_file_roundtrip() {
    test_home();
    let Some(path) = openlp("service/test.osz") else { return };
    let service = formats::import_osz(&path).unwrap();
    assert_eq!(service.items.len(), 3);
    let ItemKind::Song(song) = &service.items[0].kind else { panic!("first item should be a song") };
    assert_eq!(song.title, "A Safe Stronghold Our God is Still");
    assert_eq!(song.author, "Martin Luther");
    assert_eq!(song.parts.len(), 4);
    assert!(matches!(service.items[2].kind, ItemKind::Bible(_)));

    // export again and read our own export back
    let out = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target/test-home/export.osz");
    let skipped = formats::export_osz(&out, &service).unwrap();
    assert!(skipped.is_empty());
    let again = formats::import_osz(&out).unwrap();
    assert_eq!(again.items.len(), 3);
    let ItemKind::Song(s2) = &again.items[0].kind else { panic!() };
    assert_eq!(s2.parts, song.parts);
    let _ = std::fs::remove_file(out);
}

#[test]
fn own_service_format_roundtrip_with_media() {
    test_home();
    let png = {
        let img = image::RgbaImage::from_pixel(4, 3, image::Rgba([10, 20, 30, 255]));
        let mut buf = std::io::Cursor::new(Vec::new());
        img.write_to(&mut buf, image::ImageFormat::Png).unwrap();
        buf.into_inner()
    };
    let media = crate::storage::store_media_bytes(&png, "png").unwrap();
    let design = formats::full_screen_image("img", media.clone(), &png);
    assert_eq!(design.doc.objects[0].h, CANVAS_H);
    let mut song = Song::new();
    song.title = "Píseň".into();
    song.parts.push(SongPart { tag: "C".into(), number: 0, text: "Refrén".into(), translations: Vec::new() });
    let service = Service { items: vec![ServiceItem::new(ItemKind::Design(design)), ServiceItem::new(ItemKind::Song(song))] };
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target/test-home/service.ohsvc");
    formats::save_service(&path, &service).unwrap();
    // remove the media from the store; loading must bring it back
    std::fs::remove_file(crate::storage::media_path(&media)).unwrap();
    let loaded = formats::load_service(&path).unwrap();
    assert_eq!(loaded, service);
    assert!(crate::storage::media_path(&media).exists());
    let _ = std::fs::remove_file(path);
}

#[test]
fn safe_saving_never_loses_files() {
    test_home();
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target/test-home/save");
    std::fs::create_dir_all(&dir).unwrap();
    let f = dir.join("a.txt");
    std::fs::write(&f, "old").unwrap();
    assert_eq!(crate::storage::unique_path(&f), dir.join("a (2).txt"));
    crate::storage::write_atomic(&f, b"new", true).unwrap();
    assert_eq!(std::fs::read_to_string(&f).unwrap(), "new");
    let backups: Vec<_> = std::fs::read_dir(crate::storage::paths().backups())
        .unwrap()
        .flatten()
        .filter(|e| e.file_name().to_string_lossy().ends_with("-a.txt"))
        .collect();
    assert!(!backups.is_empty());
    assert_eq!(std::fs::read_to_string(backups[0].path()).unwrap(), "old");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn translations_cover_every_key() {
    let en = crate::i18n::BUILTIN_EN;
    let cs = crate::i18n::BUILTIN_CS;
    let keys = |s: &str| -> std::collections::BTreeSet<String> {
        s.lines()
            .filter(|l| !l.trim_start().starts_with('#'))
            .filter_map(|l| l.split_once('='))
            .map(|(k, _)| k.trim().to_string())
            .collect()
    };
    let (ek, ck) = (keys(en), keys(cs));
    let missing_cs: Vec<_> = ek.difference(&ck).collect();
    let missing_en: Vec<_> = ck.difference(&ek).collect();
    assert!(missing_cs.is_empty(), "missing in cs.lang: {missing_cs:?}");
    assert!(missing_en.is_empty(), "missing in en.lang: {missing_en:?}");
    // every key used in the code must exist
    let src = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut used = std::collections::BTreeSet::new();
    let mut stack = vec![src];
    while let Some(dir) = stack.pop() {
        for e in std::fs::read_dir(dir).unwrap().flatten() {
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
                continue;
            }
            let text = std::fs::read_to_string(&p).unwrap();
            for pat in ["t(\"", "tf(\""] {
                for (i, _) in text.match_indices(pat) {
                    // only the functions t / tf themselves, not e.g. `left("`
                    if i > 0 && text[..i].chars().last().map(|c| c.is_alphanumeric() || c == '_' || c == '.' || c == ':').unwrap_or(false) {
                        continue;
                    }
                    let rest = &text[i + pat.len()..];
                    if let Some(end) = rest.find('"') {
                        used.insert(rest[..end].to_string());
                    }
                }
            }
        }
    }
    for a in crate::shortcuts::Action::ALL {
        used.insert(a.label_key());
    }
    for m in crate::settings::SlideMark::defaults() {
        used.insert(format!("mark.{}", m.code));
    }
    let missing: Vec<_> = used.iter().filter(|k| !ek.contains(*k)).collect();
    assert!(missing.is_empty(), "keys used in code but missing in en.lang: {missing:?}");
}

// ---------------------------------------------------------------------------
// Headless UI: run real frames of the whole app and press keys.

fn key(k: eframe::egui::Key, modifiers: eframe::egui::Modifiers) -> Vec<eframe::egui::Event> {
    vec![
        eframe::egui::Event::Key { key: k, physical_key: None, pressed: true, repeat: false, modifiers },
        eframe::egui::Event::Key { key: k, physical_key: None, pressed: false, repeat: false, modifiers },
    ]
}

struct Harness {
    ctx: eframe::egui::Context,
    app: crate::app::App,
    /// Every text drawn in the last frame.
    texts: Vec<String>,
    // UI tests change the global language, so they run one at a time
    _lock: std::sync::MutexGuard<'static, ()>,
}

impl Harness {
    fn new() -> Self {
        static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
        let lock = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        test_home();
        let ctx = eframe::egui::Context::default();
        let settings = crate::settings::Settings { network_enabled: false, language: "en".into(), ..Default::default() };
        crate::i18n::set_language("en");
        let app = crate::app::App::with_settings(&ctx, settings);
        let mut h = Self { ctx, app, texts: vec![], _lock: lock };
        h.frame(vec![]);
        h
    }

    fn frame(&mut self, events: Vec<eframe::egui::Event>) {
        self.frame_with(eframe::egui::RawInput { events, ..Default::default() });
    }

    fn frame_with(&mut self, mut input: eframe::egui::RawInput) {
        input.screen_rect.get_or_insert(eframe::egui::Rect::from_min_size(eframe::egui::Pos2::ZERO, eframe::egui::vec2(1400.0, 900.0)));
        input.viewports.entry(eframe::egui::ViewportId::ROOT).or_default().focused = Some(true);
        input.focused = true;
        let app = &mut self.app;
        // as eframe does before every frame
        app.menu_arrows_as_tab(&self.ctx, &mut input);
        let mut out = self.ctx.run_ui(input, |ui| app.frame(ui));
        out.textures_delta.clear();
        self.texts.clear();
        fn collect(shape: &eframe::egui::Shape, out: &mut Vec<String>) {
            match shape {
                eframe::egui::Shape::Text(t) => out.push(t.galley.text().to_string()),
                eframe::egui::Shape::Vec(v) => v.iter().for_each(|s| collect(s, out)),
                _ => {}
            }
        }
        for s in &out.shapes {
            collect(&s.shape, &mut self.texts);
        }
    }

    fn press(&mut self, k: eframe::egui::Key) {
        self.frame(key(k, eframe::egui::Modifiers::NONE));
    }
}

fn sample_song() -> Song {
    let mut s = Song::new();
    s.title = "Test song".into();
    s.parts = vec![
        SongPart { tag: "V".into(), number: 1, text: "verse one".into(), translations: Vec::new() },
        SongPart { tag: "C".into(), number: 0, text: "chorus".into(), translations: Vec::new() },
        SongPart { tag: "V".into(), number: 2, text: "verse two\n---\nverse two b".into(), translations: Vec::new() },
        SongPart { tag: "B".into(), number: 0, text: "bridge".into(), translations: Vec::new() },
    ];
    s.order = "V1 C V2 C B C".into();
    s
}

#[test]
fn ui_presenting_with_keyboard() {
    use crate::app::{ItemRef, Tab};
    use eframe::egui::{Key, Modifiers};
    let mut h = Harness::new();
    h.app.service.items.push(ServiceItem::new(ItemKind::Song(sample_song())));
    h.app.service.items.push(ServiceItem::new(ItemKind::Text { title: "Announcements".into(), text: "one\n---\ntwo".into() }));
    h.app.tab = Tab::Service;
    h.frame(vec![]);

    // Down selects the first slide and (live mode) shows it
    h.press(Key::ArrowDown);
    assert_eq!(h.app.sel_item, Some(ItemRef::Service(0)));
    assert_eq!(h.app.sel_slide, 0);
    assert_eq!(h.app.live_pos, Some((ItemRef::Service(0), 0)));
    h.press(Key::ArrowDown);
    assert_eq!(h.app.sel_slide, 1); // C
    // V1 C V2a V2b C B C -> slide 2 is verse two (first page)
    h.press(Key::Num2);
    assert_eq!(h.app.sel_slide, 2);
    // slides: V1 C V2a V2b C B C — B jumps to the bridge, C to the next chorus after it
    h.press(Key::B);
    assert_eq!(h.app.sel_slide, 5);
    h.press(Key::C);
    assert_eq!(h.app.sel_slide, 6);
    h.press(Key::C); // wraps around to the first chorus
    assert_eq!(h.app.sel_slide, 1);
    // End, then Down moves on to the next item
    h.press(Key::End);
    assert_eq!(h.app.sel_slide, 6);
    h.press(Key::ArrowDown);
    assert_eq!(h.app.sel_item, Some(ItemRef::Service(1)));
    // blank screen toggles
    h.press(Key::Period);
    assert!(h.app.blank);
    h.press(Key::Period);
    assert!(!h.app.blank);
    // live mode off: navigation only previews
    h.press(Key::L);
    assert!(!h.app.settings.live_follows_selection);
    h.press(Key::ArrowDown);
    assert_eq!(h.app.sel_slide, 1);
    assert_eq!(h.app.live_pos, Some((ItemRef::Service(1), 0)));
    h.press(Key::Enter);
    assert_eq!(h.app.live_pos, Some((ItemRef::Service(1), 1)));
    // Ctrl+Up goes to the previous item
    h.frame(key(Key::ArrowUp, Modifiers::COMMAND));
    assert_eq!(h.app.sel_item, Some(ItemRef::Service(0)));
    // tab switching shortcuts; every tab renders
    for (k, tab) in [
        (Key::Num2, Tab::Songs),
        (Key::Num3, Tab::Designer),
        (Key::Num4, Tab::Bible),
        (Key::Num5, Tab::Settings),
        (Key::Num1, Tab::Service),
    ] {
        h.frame(key(k, Modifiers::COMMAND));
        assert_eq!(h.app.tab, tab);
        h.frame(vec![]);
    }
    // output window (embedded when there is no native backend)
    h.press(Key::F5);
    assert!(h.app.output_open);
    h.frame(vec![]);
}

#[test]
fn ui_editors() {
    use crate::app::Tab;
    use eframe::egui::{Key, Modifiers};
    let mut h = Harness::new();
    // song editor with a song loaded
    h.app.songs.song = sample_song();
    h.app.tab = Tab::Songs;
    h.frame(vec![]);
    h.frame(vec![]);
    // designer: add objects, select all, duplicate, delete, undo
    h.app.tab = Tab::Designer;
    h.frame(vec![]);
    let doc = &mut h.app.designer.design.doc;
    doc.objects.push(CanvasObject {
        id: make_id(),
        x: 10.0,
        y: 10.0,
        w: 100.0,
        h: 50.0,
        kind: ObjectKind::Rect { fill: Rgba::RED, stroke: Rgba::BLACK, stroke_width: 2.0, corner_radius: 4.0 },
    });
    doc.objects.push(CanvasObject {
        id: make_id(),
        x: 100.0,
        y: 100.0,
        w: 400.0,
        h: 100.0,
        kind: ObjectKind::Text {
            text: "Ahoj světe".into(),
            font_size: 50.0,
            color: Rgba::BLACK,
            align: HAlign::Center,
            font: FontStyle::Proportional,
        },
    });
    h.frame(vec![]);
    h.frame(key(Key::A, Modifiers::COMMAND));
    assert_eq!(h.app.designer.selected.len(), 2);
    h.frame(key(Key::D, Modifiers::COMMAND));
    assert_eq!(h.app.designer.design.doc.objects.len(), 4);
    h.press(Key::Delete);
    assert_eq!(h.app.designer.design.doc.objects.len(), 2);
    h.frame(vec![]);
    h.frame(key(Key::Z, Modifiers::COMMAND));
    h.frame(vec![]);
    assert_eq!(h.app.designer.design.doc.objects.len(), 4, "undo restores the deleted copies");
    assert!(h.app.designer.dirty);
    // single selection shows the property panel
    h.app.designer.selected = vec![1];
    h.frame(vec![]);
}

#[test]
fn ui_bible_search_and_quick_live() {
    use crate::app::{ItemRef, Tab};
    let Some(path) = openlp("bibles/osis-kjv.xml") else { return };
    let mut h = Harness::new();
    h.app.bible.current = Some(bible::import_file(&path).unwrap());
    h.app.tab = Tab::Bible;
    h.app.bible.query = "Gen 1:1-3".into();
    h.app.run_bible_search();
    assert_eq!(h.app.bible.results.len(), 3);
    h.frame(vec![]);
    let p = h.app.bible_selected_passage().unwrap();
    assert_eq!(p.reference, "Genesis 1:1-3");
    // pin + history
    h.app.pin_current_passage();
    assert!(h.app.bible.bookmarks.is_pinned("Genesis 1:1-3", &h.app.bible.current.as_ref().unwrap().meta.id));
    assert_eq!(h.app.bible.bookmarks.history[0].reference, "Genesis 1:1-3");
    // text search
    h.app.bible.query = "light day".into();
    h.app.run_bible_search();
    assert!(!h.app.bible.results.is_empty());
    h.frame(vec![]);
    // quick live through the service tab
    h.app.bible.query = "Gen 1:1-2".into();
    h.app.run_bible_search();
    h.app.quick = Some(ServiceItem::new(ItemKind::Bible(h.app.bible_selected_passage().unwrap())));
    h.app.select(ItemRef::Quick, 0);
    h.app.tab = Tab::Service;
    h.frame(vec![]);
    h.press(eframe::egui::Key::ArrowDown);
    assert_eq!((h.app.sel_item, h.app.sel_slide), (Some(ItemRef::Quick), 1));
    let Scene::Text(t) = &h.app.live else { panic!("bible slide should be a text scene") };
    assert!(t.footer.starts_with("Genesis 1:2"), "{}", t.footer);
}

#[test]
fn ui_czech_and_shortcut_capture() {
    use crate::app::Tab;
    use eframe::egui::Key;
    let mut h = Harness::new();
    crate::i18n::set_language("cs");
    assert_eq!(crate::i18n::t("mark.C"), "Refrén");
    h.app.tab = Tab::Settings;
    h.frame(vec![]);
    // rebind "blank" to B... capture mode adds the key
    h.app.capture = Some(crate::app::settings_tab_capture(crate::shortcuts::Action::Blank));
    h.press(Key::W);
    assert!(h.app.settings.shortcuts.bindings(crate::shortcuts::Action::Blank).contains(&crate::shortcuts::KeyBinding::plain(Key::W)));
    assert!(h.app.capture.is_none());
    crate::i18n::set_language("en");
}

#[test]
fn network_output_receives_scenes_and_images() {
    test_home();
    use crate::net::{ClientEvent, Server, run_client};
    use std::time::Duration;
    // a free port
    let port = std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
    let server = Server::start(port);
    assert!(server.error.is_none(), "{:?}", server.error);
    let rx = run_client(Some(format!("127.0.0.1:{port}")), "test-canvas".into(), eframe::egui::Context::default());
    let wait = |pred: &dyn Fn(&ClientEvent) -> bool| -> bool {
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        while std::time::Instant::now() < deadline {
            if let Ok(ev) = rx.recv_timeout(Duration::from_millis(200))
                && pred(&ev)
            {
                return true;
            }
        }
        false
    };
    assert!(wait(&|e| matches!(e, ClientEvent::Connected(_))));
    // the current scene (blank) arrives on connect
    assert!(wait(&|e| matches!(e, ClientEvent::Scene(Scene::Blank))));
    std::thread::sleep(Duration::from_millis(200));
    let clients = server.clients();
    assert_eq!(clients.len(), 1);
    assert_eq!(clients[0].display_name, "test-canvas");

    // a design with an image: the image is sent first, then the scene
    let png = {
        let img = image::RgbaImage::from_pixel(2, 2, image::Rgba([1, 2, 3, 255]));
        let mut buf = std::io::Cursor::new(Vec::new());
        img.write_to(&mut buf, image::ImageFormat::Png).unwrap();
        buf.into_inner()
    };
    let media = crate::storage::store_media_bytes(&png, "png").unwrap();
    let design = formats::full_screen_image("x", media.clone(), &png);
    server.broadcast(&Scene::Canvas(design.doc.clone()));
    let m = media.clone();
    assert!(wait(&move |e| matches!(e, ClientEvent::Asset(n) if *n == m)));
    assert_eq!(std::fs::read(crate::storage::paths().client_media().join(&media)).unwrap(), png);
    assert!(wait(&|e| matches!(e, ClientEvent::Scene(Scene::Canvas(_)))));

    // alerts: start and stop
    let alert = Alert { text: "Car 1AB 2345".into(), style: AlertStyle::default(), duration: 20.0 };
    server.alert(Some(&alert));
    assert!(wait(&|e| matches!(e, ClientEvent::Alert(Some(a)) if a.text == "Car 1AB 2345")));
    // a client connecting while it runs gets the rest of it
    let rx2 = run_client(Some(format!("127.0.0.1:{port}")), "late".into(), eframe::egui::Context::default());
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    let mut late = None;
    while late.is_none() && std::time::Instant::now() < deadline {
        if let Ok(ClientEvent::Alert(a)) = rx2.recv_timeout(Duration::from_millis(200)) {
            late = a;
        }
    }
    let late = late.expect("the late client gets the running alert");
    assert!(late.duration > 15.0 && late.duration <= 20.0, "{}", late.duration);
    server.alert(None);
    assert!(wait(&|e| matches!(e, ClientEvent::Alert(None))));
}

#[test]
fn network_discovery_answers_broadcast() {
    use std::net::UdpSocket;
    let port = UdpSocket::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
    let _server = crate::net::Server::start(port);
    std::thread::sleep(std::time::Duration::from_millis(100));
    let sock = UdpSocket::bind("127.0.0.1:0").unwrap();
    sock.set_read_timeout(Some(std::time::Duration::from_secs(2))).unwrap();
    sock.send_to(b"OH_PRESENTER_DISCOVER", ("127.0.0.1", port)).unwrap();
    let mut buf = [0u8; 64];
    let (n, _) = sock.recv_from(&mut buf).unwrap();
    assert_eq!(String::from_utf8_lossy(&buf[..n]), format!("OH_PRESENTER_SERVER {port}"));
}

/// No raw translation key (like `tab.songs`) may ever reach the screen, in any
/// language, on any tab or dialog — and switching the language must switch
/// the texts right away.
#[test]
fn ui_shows_no_translation_keys() {
    use crate::app::{Dialog, Tab};
    let mut h = Harness::new();
    h.app.service.items.push(ServiceItem::new(ItemKind::Song(sample_song())));
    let looks_like_key = |s: &str| {
        let s = s.trim().trim_end_matches(" •");
        s.contains('.')
            && !s.contains(' ')
            && s.split('.').all(|p| {
                !p.is_empty() && p.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c.is_ascii_uppercase())
            })
            && s.chars().next().map(|c| c.is_ascii_lowercase()).unwrap_or(false)
    };
    let mut seen = std::collections::HashMap::new();
    for lang in ["en", "cs"] {
        crate::i18n::set_language(lang);
        let mut all = Vec::new();
        for tab in [Tab::Service, Tab::Songs, Tab::Designer, Tab::Bible, Tab::Settings] {
            h.app.tab = tab;
            h.frame(vec![]);
            h.frame(vec![]);
            all.extend(h.texts.clone());
        }
        for d in [
            Dialog::UnsavedOnQuit,
            Dialog::ConfirmQuit,
            Dialog::ChangedOnDisk { path: "x.ohsvc".into() },
            Dialog::FileExists { path: "x.ohsvc".into(), kind: crate::app::WriteKind::Service },
        ] {
            h.app.dialog = Some(d);
            h.frame(vec![]);
            h.frame(vec![]);
            all.extend(h.texts.clone());
        }
        h.app.dialog = None;
        let keys: Vec<_> = all.iter().filter(|s| looks_like_key(s)).collect();
        assert!(keys.is_empty(), "{lang}: untranslated keys on screen: {keys:?}");
        assert!(all.iter().any(|s| s.contains(&crate::i18n::t("tab.songs"))), "{lang}: tab label missing: {all:?}");
        seen.insert(lang, all);
    }
    assert!(seen["cs"].iter().any(|s| s.contains("Písně")));
    assert!(!seen["en"].iter().any(|s| s.contains("Písně")));
    crate::i18n::set_language("en");
}

/// A system file dialog must not block the UI: frames keep running while it
/// is open, keys are ignored meanwhile, and the answer finishes the job.
#[test]
fn ui_file_dialog_does_not_block() {
    use crate::app::files::{Pending, Purpose};
    use eframe::egui::Key;
    let mut h = Harness::new();
    h.app.service.items.push(ServiceItem::new(ItemKind::Song(sample_song())));
    h.app.tab = crate::app::Tab::Service;
    h.frame(vec![]);

    // still open: the UI runs, shows the waiting note and ignores presenting keys
    let (pending, tx) = Pending::answered(Purpose::ImportSongs, None);
    h.app.file_dialog = Some(pending);
    let before = h.app.sel_slide;
    for _ in 0..3 {
        h.press(Key::ArrowDown);
    }
    assert!(h.app.file_dialog.is_some());
    assert_eq!(h.app.sel_slide, before);
    assert!(h.texts.iter().any(|s| s == &crate::i18n::t("file.dialog_open")), "{:?}", h.texts);

    // the user picks a song file: it gets imported
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target/test-home/dialog");
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("dialog-song.cho");
    std::fs::write(&file, "{title: Song from the dialog}\n{start_of_verse}\nline one\n{end_of_verse}\n").unwrap();
    tx.send(vec![file]).unwrap();
    h.frame(vec![]);
    assert!(h.app.file_dialog.is_none());
    assert!(h.app.lib.songs.iter().any(|s| s.title == "Song from the dialog"));

    // cancelling does nothing; keys work again
    let (pending, _tx) = Pending::answered(Purpose::OpenService, Some(vec![]));
    h.app.file_dialog = Some(pending);
    h.frame(vec![]);
    assert!(h.app.file_dialog.is_none());
    assert!(h.app.dialog.is_none());
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn missing_translation_is_readable() {
    assert_eq!(crate::i18n::t("tab.no_such_key"), "No such key");
}

/// A key as a Czech keyboard reports it: the digit row types `+ ě š č ř ž ý á í é`,
/// so the `1` key arrives as logical `+` with the physical key `1`.
fn czech_key(logical: eframe::egui::Key, physical: eframe::egui::Key, modifiers: eframe::egui::Modifiers) -> Vec<eframe::egui::Event> {
    [true, false]
        .into_iter()
        .map(|pressed| eframe::egui::Event::Key { key: logical, physical_key: Some(physical), pressed, repeat: false, modifiers })
        .collect()
}

#[test]
fn ui_czech_keyboard_digits_and_zoom() {
    use crate::app::{ItemRef, Tab};
    use eframe::egui::{Key, Modifiers};
    let mut h = Harness::new();
    h.app.service.items.push(ServiceItem::new(ItemKind::Song(sample_song())));
    h.app.tab = Tab::Songs;
    h.frame(vec![]);
    // Ctrl + the "1" key (logical "+") switches to the service tab and does not zoom
    h.frame(czech_key(Key::Plus, Key::Num1, Modifiers::COMMAND));
    assert_eq!(h.app.tab, Tab::Service);
    h.frame(vec![]);
    assert_eq!(h.ctx.zoom_factor(), 1.0);
    // the "2" key types "ě": egui already falls back to the digit
    h.frame(czech_key(Key::Num2, Key::Num2, Modifiers::COMMAND));
    assert_eq!(h.app.tab, Tab::Songs);
    h.frame(czech_key(Key::Plus, Key::Num1, Modifiers::COMMAND));
    // plain "1" (logical "+") jumps to verse 1
    h.app.select(ItemRef::Service(0), 3);
    h.frame(czech_key(Key::Plus, Key::Num1, Modifiers::NONE));
    assert_eq!(h.app.sel_slide, 0);

    // a real Ctrl+Plus (numpad, or "+" on an English keyboard) still zooms,
    // and the status bar shows the zoom in percent
    assert!(!h.texts.iter().any(|t| t.contains('%')), "{:?}", h.texts);
    h.frame(czech_key(Key::Plus, Key::Plus, Modifiers::COMMAND));
    h.frame(vec![]);
    assert!((h.ctx.zoom_factor() - 1.1).abs() < 0.001);
    assert!(h.texts.iter().any(|t| t.contains("110 %")), "{:?}", h.texts);
    h.frame(key(Key::Minus, Modifiers::COMMAND));
    h.frame(vec![]);
    assert_eq!(h.ctx.zoom_factor(), 1.0);
    assert!(!h.texts.iter().any(|t| t.contains(" %")), "{:?}", h.texts);
}

#[test]
fn ui_quit_always_asks() {
    use crate::app::Dialog;
    use eframe::egui::{Key, Modifiers};
    let mut h = Harness::new();
    // nothing unsaved: Ctrl+Q still asks
    h.frame(key(Key::Q, Modifiers::COMMAND));
    assert!(matches!(h.app.dialog, Some(Dialog::ConfirmQuit)), "{:?}", h.app.dialog);
    assert!(!h.app.quit_confirmed);
    h.frame(vec![]);
    assert!(h.texts.iter().any(|t| t == &crate::i18n::t("dialog.confirm_quit")), "{:?}", h.texts);
    // Esc = Cancel: the dialog closes, the program stays
    h.press(Key::Escape);
    h.frame(vec![]);
    assert!(h.app.dialog.is_none(), "{:?}", h.app.dialog);
    assert!(!h.app.quit_confirmed);

    // closing the window asks too, and Esc cancels that as well
    let mut close = eframe::egui::RawInput::default();
    close.viewports.entry(eframe::egui::ViewportId::ROOT).or_default().events.push(eframe::egui::ViewportEvent::Close);
    h.frame_with(close);
    assert!(matches!(h.app.dialog, Some(Dialog::ConfirmQuit)));
    h.press(Key::Escape);
    h.frame(vec![]);
    assert!(h.app.dialog.is_none());
    assert!(!h.app.quit_confirmed);

    // unsaved work: the "unsaved changes" question instead; Esc keeps everything
    h.app.service_dirty = true;
    h.frame(key(Key::Q, Modifiers::COMMAND));
    assert!(matches!(h.app.dialog, Some(Dialog::UnsavedOnQuit)));
    assert!(!h.app.quit_confirmed);
    h.frame(vec![]);
    h.press(Key::Escape);
    h.frame(vec![]);
    assert!(h.app.dialog.is_none());
    assert!(!h.app.quit_confirmed);
    assert!(h.app.service_dirty, "nothing was saved or discarded");
    // "Quit without saving" quits
    h.app.dialog = None;
    h.app.run(crate::app::Then::Quit);
    assert!(h.app.quit_confirmed);
}

#[test]
fn ui_service_list_and_about() {
    use crate::app::{ItemRef, Tab};
    let mut h = Harness::new();
    h.app.service.items.push(ServiceItem::new(ItemKind::Song(sample_song())));
    h.app.service.items.push(ServiceItem::new(ItemKind::Text { title: "Announcements".into(), text: "one".into() }));
    h.app.tab = Tab::Service;
    h.app.select(ItemRef::Service(1), 0);
    h.frame(vec![]);
    h.frame(vec![]);
    // items are listed (with their slide previews), the separate preview is gone
    assert!(h.texts.iter().any(|t| t.contains("Test song")), "{:?}", h.texts);
    assert!(h.texts.iter().any(|t| t.contains("Announcements")));
    assert!(h.texts.iter().any(|t| t == "verse one"), "the song's thumbnail shows its first slide: {:?}", h.texts);
    assert!(!h.texts.iter().any(|t| t == "Preview"));
    // Settings → About
    // (at the bottom of the page: use a tall window so it is not scrolled away)
    h.app.tab = Tab::Settings;
    let tall = || eframe::egui::RawInput {
        screen_rect: Some(eframe::egui::Rect::from_min_size(eframe::egui::Pos2::ZERO, eframe::egui::vec2(1400.0, 5000.0))),
        ..Default::default()
    };
    h.frame_with(tall());
    h.frame_with(tall());
    assert!(h.texts.iter().any(|t| t == &crate::i18n::t("settings.about")), "{:?}", h.texts);
    assert!(h.texts.iter().any(|t| t.contains(env!("CARGO_PKG_VERSION"))));
}

#[test]
fn song_columns_slides_and_openlyrics() {
    test_home();
    let mut song = sample_song();
    song.add_column("English".into(), "Čeština".into());
    song.parts[0].column_text_mut(1).push_str("verse one EN\n---\npage two EN");
    *song.parts[1].column_text_mut(1) = "chorus EN".into();
    assert_eq!(song.column_count(), 2);
    // V1 has one Czech page and two English pages: two slides, the second with an empty main text
    let pages = song.parts[0].column_pages(&song.shown_columns());
    assert_eq!(pages, vec![vec!["verse one".to_string(), "verse one EN".into()], vec![String::new(), "page two EN".into()]]);
    // hiding a column shows only the other one
    song.columns[0].show = false;
    assert_eq!(song.parts[1].column_pages(&song.shown_columns()), vec![vec!["chorus EN".to_string()]]);
    song.columns[0].show = true;
    assert!(song.text_matches("chorus en"));

    // OpenLyrics keeps the columns as <verse lang="...">
    let xml = formats::song_to_openlyrics(&song);
    assert!(xml.contains("lang=\"English\""), "{xml}");
    let back = formats::song_from_openlyrics(&xml).unwrap();
    assert_eq!(back.columns.iter().map(|c| c.name.as_str()).collect::<Vec<_>>(), ["Čeština", "English"]);
    assert_eq!(back.parts[0].column_text(1), "verse one EN\n---\npage two EN");
    assert_eq!(back.parts[1].column_text(1), "chorus EN");
    assert_eq!(back.parts[0].text, song.parts[0].text);

    // removing the main column makes the translation the main text
    let mut s = back.clone();
    s.remove_column(0);
    assert!(s.columns.is_empty());
    assert_eq!(s.parts[1].text, "chorus EN");

    // older songs without columns still load
    let old: Song = serde_json::from_str(r#"{"id":"x","title":"T","parts":[{"tag":"V","number":1,"text":"a"}]}"#).unwrap();
    assert_eq!(old.column_count(), 1);
    assert_eq!(old.shown_columns(), vec![0]);
}

/// A new empty folder for one test run, removed when dropped. It is outside the project
/// (which may be on a shared or network drive) and unique per process, so files left by
/// another run — possibly of another user — never get in the way.
struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("oh-presenter-rs-test-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn legacy_files_are_found_and_imported() {
    test_home();
    let old = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../oh-presenter");
    if !old.join("test_old_oh_presenter.ohdata").exists() {
        return;
    }
    let songs = formats::import_ohdata(&old.join("test_old_oh_presenter.ohdata")).unwrap();
    assert_eq!(songs.len(), 2);
    assert_eq!(songs[0].title, "song");
    assert_eq!(songs[0].parts.iter().map(|p| p.text.as_str()).collect::<Vec<_>>(), ["song1", "song2"]);
    for f in ["test_old_canvas.ohcanvas", "test_old_canvas2.ohcanvas"] {
        let d = formats::import_design_file(&old.join(f)).unwrap();
        assert!(!d.doc.objects.is_empty(), "{f}");
    }

    // the scan finds them (and skips hidden / node_modules folders)
    let scratch = Scratch::new("legacy-scan");
    let dir = scratch.0.clone();
    std::fs::create_dir_all(dir.join("Documents/songs")).unwrap();
    std::fs::create_dir_all(dir.join("node_modules")).unwrap();
    std::fs::create_dir_all(dir.join(".hidden")).unwrap();
    std::fs::write(dir.join("Documents/songs/a.ohdata"), std::fs::read(old.join("test_old_oh_presenter.ohdata")).unwrap()).unwrap();
    std::fs::write(dir.join("b.ohcanvas"), std::fs::read(old.join("test_old_canvas.ohcanvas")).unwrap()).unwrap();
    std::fs::write(dir.join("node_modules/c.ohdata"), "[]").unwrap();
    std::fs::write(dir.join(".hidden/d.ohdata"), "[]").unwrap();
    std::fs::write(dir.join("e.txt"), "").unwrap();
    let scan = formats::scan_legacy(&dir, 7, 10_000);
    let mut names: Vec<String> = scan.files.iter().map(|f| f.file_name().unwrap().to_string_lossy().to_string()).collect();
    names.sort();
    assert_eq!(names, ["a.ohdata", "b.ohcanvas"]);
    assert!(scan.old_app_dir.is_none());
}

#[test]
fn ui_song_columns_and_legacy_import() {
    use crate::app::Tab;
    let mut h = Harness::new();
    // a song with two columns renders the editor and its slides
    let mut song = sample_song();
    song.add_column("English".into(), "Čeština".into());
    *song.parts[0].column_text_mut(1) = "verse one EN".into();
    h.app.songs.song = song.clone();
    h.app.tab = Tab::Songs;
    h.frame(vec![]);
    h.frame(vec![]);
    assert!(h.texts.iter().any(|t| t == "Columns"), "{:?}", h.texts);
    assert!(h.texts.iter().any(|t| t.contains("verse one EN")));
    let slides = h.app.item_slides(&ServiceItem::new(ItemKind::Song(song.clone())));
    let Scene::Text(first) = &slides[0].scene else { panic!() };
    assert_eq!((first.text.as_str(), first.extra.clone()), ("verse one", vec!["verse one EN".to_string()]));
    h.app.songs.song.column_layout = ColumnLayout::Stacked;
    h.frame(vec![]);

    // old files: the offer shows a dialog listing them
    let old = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../oh-presenter");
    if !old.join("test_old_canvas.ohcanvas").exists() {
        return;
    }
    let scratch = Scratch::new("legacy-import");
    let dir = scratch.0.clone();
    // an .ohpres the way the old app writes it: a plain tar with slides.json and the images
    let canvas: serde_json::Value = serde_json::from_slice(&std::fs::read(old.join("test_old_canvas.ohcanvas")).unwrap()).unwrap();
    let mut canvas_with_image = canvas.clone();
    canvas_with_image["objects"].as_array_mut().unwrap().push(serde_json::json!({
        "type": "image", "left": 10, "top": 10, "width": 100, "height": 50, "scaleX": 2, "scaleY": 2, "src": "presentation://img1"
    }));
    let datasets: serde_json::Value = serde_json::from_slice(&std::fs::read(old.join("test_old_oh_presenter.ohdata")).unwrap()).unwrap();
    let slides = serde_json::json!([
        {"id": "a", "name": "Uvítání", "type": "canvas", "jsonCanvasData": canvas_with_image, "data": null},
        {"id": "b", "name": "Lyrics slide", "type": "lyrics", "jsonCanvasData": null, "data": datasets},
    ]);
    let mut png = Vec::new();
    image::RgbaImage::from_pixel(2, 2, image::Rgba([0, 0, 255, 255]))
        .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
        .unwrap();
    {
        let mut tar = tar::Builder::new(std::fs::File::create(dir.join("sunday.ohpres")).unwrap());
        for (name, bytes) in [("slides.json", serde_json::to_vec(&slides).unwrap()), ("img1", png)] {
            let mut head = tar::Header::new_gnu();
            head.set_size(bytes.len() as u64);
            head.set_mode(0o644);
            head.set_cksum();
            tar.append_data(&mut head, name, bytes.as_slice()).unwrap();
        }
        tar.finish().unwrap();
    }
    std::fs::write(dir.join("songs.ohdata"), std::fs::read(old.join("test_old_oh_presenter.ohdata")).unwrap()).unwrap();
    std::fs::write(dir.join("title.ohcanvas"), std::fs::read(old.join("test_old_canvas2.ohcanvas")).unwrap()).unwrap();

    let scan = formats::scan_legacy(&dir, 3, 1000);
    assert_eq!(scan.files.len(), 3);
    h.app.offer_legacy(scan, false);
    h.frame(vec![]);
    h.frame(vec![]);
    assert!(matches!(h.app.dialog, Some(crate::app::Dialog::LegacyImport)));
    assert!(h.texts.iter().any(|t| t == "Import from the old oh! Presenter"), "{:?}", h.texts);
    assert!(h.texts.iter().any(|t| t.contains("Import selected (3)")));

    let files = h.app.legacy.as_ref().unwrap().scan.files.clone();
    clean_legacy_imports(&mut h.app);
    h.app.dialog = None;
    h.app.legacy = None;
    let (songs0, designs0) = (h.app.lib.songs.len(), h.app.lib.designs.len());
    h.app.import_legacy_files(files.clone());
    // songs from the .ohdata (the same sets are in the .ohpres, so they count once) and 2 designs
    assert_eq!(h.app.lib.songs.len(), songs0 + 2, "{:?}", h.app.dialog);
    assert_eq!(h.app.lib.designs.len(), designs0 + 2);
    let svc_path = dir.join("sunday.ohsvc");
    assert!(svc_path.exists(), "the presentation is converted to a service next to it");
    let svc = formats::load_service(&svc_path).unwrap();
    assert_eq!(svc.items.len(), 3);
    let ItemKind::Design(d) = &svc.items[0].kind else { panic!() };
    assert!(d.doc.objects.iter().any(|o| matches!(o.kind, ObjectKind::Image { .. })), "the image came along");
    // importing again adds nothing and does not overwrite the converted service
    h.app.import_legacy_files(files);
    assert_eq!(h.app.lib.songs.len(), songs0 + 2);
    assert_eq!(h.app.lib.designs.len(), designs0 + 2);
    assert!(dir.join("sunday (2).ohsvc").exists(), "a second conversion gets a new name");
    clean_legacy_imports(&mut h.app);
}

/// Removes what the legacy import test put into the shared test library.
fn clean_legacy_imports(app: &mut crate::app::App) {
    for s in app.lib.songs.clone().iter().filter(|s| s.title == "song" || s.title == "bible") {
        app.lib.delete_song(&s.id).unwrap();
    }
    for d in app.lib.designs.clone().iter().filter(|d| d.name == "Uvítání" || d.name == "title") {
        app.lib.delete_design(&d.id).unwrap();
    }
}

/// A small bible with verses in the given language, saved into the test library.
fn test_bible(name: &str, verses: &[&str]) -> crate::bible::Bible {
    use crate::bible::*;
    let chapter = |n: u32| Chapter {
        number: n,
        verses: verses.iter().enumerate().map(|(i, t)| Verse { number: i as u32 + 1, text: t.to_string() }).collect(),
    };
    let b = Bible::new(
        new_meta(name, name, ""),
        vec![
            Book { number: 1, name: String::new(), abbrev: String::new(), chapters: vec![chapter(1), chapter(2)] },
            Book { number: 59, name: String::new(), abbrev: String::new(), chapters: vec![chapter(1)] },
        ],
    );
    b.save().unwrap();
    b
}

#[test]
fn ui_bible_word_search_index_per_language() {
    use crate::app::{Dialog, Tab};
    use crate::bible::SearchIndex;
    let mut h = Harness::new();
    // bibles left over from an earlier failed run
    for b in crate::bible::list_bibles().iter().filter(|b| b.name == "Test CS" || b.name == "Test EN") {
        crate::bible::Bible::delete(&b.id);
    }
    let cs = test_bible(
        "Test CS",
        &[
            "Na počátku stvořil Bůh nebe a zemi.",
            "Země pak byla nesličná a pustá, a tma byla nad propastí, a Duch Boží vznášel se nad vodami.",
            "I řekl Bůh: Buď světlo! I bylo světlo. Jak je to dobré.",
        ],
    );
    let en = test_bible(
        "Test EN",
        &[
            "In the beginning God created the heaven and the earth.",
            "And the earth was without form, and void; and darkness was upon the face of the deep.",
            "And God said, Let there be light: and there was light.",
        ],
    );
    assert_eq!((cs.meta.language.as_str(), en.meta.language.as_str()), ("cs", "en"));
    h.app.bible.bibles = crate::bible::list_bibles();
    h.app.bible.current = Some(cs.clone());
    h.app.tab = Tab::Bible;
    h.frame(vec![]);
    let wait_for_index = |h: &mut Harness| {
        for _ in 0..500 {
            h.frame(vec![]);
            if h.app.bible.indexing.is_none() {
                return;
            }
        }
        panic!("indexing did not finish");
    };

    // a reference works without any index
    h.app.bible.query = "Gn 1,2".into();
    h.app.run_bible_search();
    assert_eq!(h.app.bible.results.len(), 1);
    assert!(h.app.dialog.is_none());

    // the first word search asks for the language, suggesting the bible's own
    h.app.bible.query = "světlo".into();
    h.app.run_bible_search();
    assert!(matches!(&h.app.dialog, Some(Dialog::BibleIndexLanguage(l)) if l == "cs"));
    h.frame(vec![]);
    h.frame(vec![]);
    assert!(h.texts.iter().any(|t| t == "Word search in bibles"), "{:?}", h.texts);
    assert!(h.texts.iter().any(|t| t.contains("Čeština — ")), "{:?}", h.texts);
    // choosing Czech (what the "Create index" button does)
    h.app.dialog = None;
    h.app.settings.bible_index_languages.push("cs".into());
    h.app.build_missing_indexes(true);
    wait_for_index(&mut h);
    assert!(SearchIndex::exists(&cs.meta.id));
    assert!(!SearchIndex::exists(&en.meta.id), "only the chosen language gets an index");
    // "světlo" is in verse 3 of each of the three chapters
    assert_eq!(h.app.bible.results.len(), 3, "{}", h.app.bible.info);

    // "jak" is a word here, not James / Jakub chapter 1; opening the book is offered
    h.app.bible.query = "Jak".into();
    h.app.run_bible_search();
    assert_eq!(h.app.bible.results.len(), 3);
    assert_eq!(h.app.bible.book_hint, Some(1));
    // several words in any order, without diacritics and with punctuation
    h.app.bible.query = "buh, SVETLO".into();
    h.app.run_bible_search();
    assert_eq!(h.app.bible.results.len(), 3);

    // English is not enabled: no dialog, an offer to enable it instead
    h.app.bible.current = Some(en.clone());
    h.app.load_current_index();
    h.app.bible.query = "light".into();
    h.app.run_bible_search();
    assert!(h.app.dialog.is_none());
    assert_eq!(h.app.bible.offer_index.as_deref(), Some("en"));
    assert!(h.app.bible.results.is_empty());
    h.app.set_index_language("en", true);
    wait_for_index(&mut h);
    h.app.run_bible_search();
    assert_eq!(h.app.bible.results.len(), 3);

    // Settings → Bible lists the languages; turning one off deletes its indexes
    h.app.tab = Tab::Settings;
    h.frame(vec![]);
    assert!(h.texts.iter().any(|t| t == "keep above all other windows"));
    // the bible section is further down the page
    for _ in 0..10 {
        if h.texts.iter().any(|t| t.starts_with("index: 1 of 1")) {
            break;
        }
        h.press(eframe::egui::Key::PageDown);
        h.frame(vec![]);
    }
    assert!(h.texts.iter().any(|t| t.starts_with("index: 1 of 1")), "{:?}", h.texts);
    h.app.set_index_language("cs", false);
    assert!(!SearchIndex::exists(&cs.meta.id));
    assert!(SearchIndex::exists(&en.meta.id));

    // a stale index (the bible changed) is not used
    let mut changed = en.clone();
    changed.books[0].chapters[0].verses.pop();
    assert!(SearchIndex::load(&changed).is_none());

    crate::bible::Bible::delete(&cs.meta.id);
    crate::bible::Bible::delete(&en.meta.id);
    assert!(!SearchIndex::exists(&en.meta.id), "deleting a bible deletes its index");
}

#[test]
fn ui_alt_letters_choose_tabs_and_menus_and_keys_scroll() {
    use crate::app::Tab;
    use eframe::egui::{Event, Key, Modifiers};
    let mut h = Harness::new();
    let mods = |m: Modifiers| Event::ModifiersChanged(m);
    let alt_typing = |h: &mut Harness, text: &str| {
        // hold Alt, type, release Alt
        h.frame(vec![mods(Modifiers::ALT)]);
        for c in text.chars() {
            let key = Key::from_name(&c.to_ascii_uppercase().to_string()).unwrap();
            h.frame(vec![
                Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::ALT },
                Event::Text(c.to_string()),
                Event::Key { key, physical_key: None, pressed: false, repeat: false, modifiers: Modifiers::ALT },
            ]);
        }
        h.frame(vec![mods(Modifiers::NONE)]);
        h.frame(vec![]);
    };
    h.app.tab = Tab::Bible;
    alt_typing(&mut h, "s");
    assert_eq!(h.app.tab, Tab::Service);
    alt_typing(&mut h, "3s");
    assert_eq!(h.app.tab, Tab::Designer);
    alt_typing(&mut h, "set");
    assert_eq!(h.app.tab, Tab::Settings);
    alt_typing(&mut h, "so");
    assert_eq!(h.app.tab, Tab::Songs);
    alt_typing(&mut h, "sss");
    assert_eq!(h.app.tab, Tab::Designer, "Service › Songs › Slide designer");
    alt_typing(&mut h, "so");
    alt_typing(&mut h, "x");
    assert_eq!(h.app.tab, Tab::Songs, "nothing matches: nothing happens");

    // the list shows while Alt is held after typing
    let alt = |key| Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::ALT };
    h.frame(vec![mods(Modifiers::ALT), alt(Key::S)]);
    h.frame(vec![]);
    assert!(h.texts.iter().any(|t| t == "Alt + s"), "{:?}", h.texts);
    assert!(h.texts.iter().any(|t| t.contains("▶ 1  Service")));
    // typing the letter again moves the marker in the list
    h.frame(vec![alt(Key::S)]);
    h.frame(vec![]);
    assert!(h.texts.iter().any(|t| t == "Alt + ss"), "{:?}", h.texts);
    assert!(h.texts.iter().any(|t| t.contains("▶ 2  Songs")), "{:?}", h.texts);
    // Esc cancels
    h.frame(vec![alt(Key::Escape)]);
    h.frame(vec![mods(Modifiers::NONE)]);
    h.frame(vec![]);
    assert_eq!(h.app.tab, Tab::Songs);

    // Alt+f opens the File menu
    alt_typing(&mut h, "f");
    h.frame(vec![]);
    assert!(h.texts.iter().any(|t| t == "Recent services"), "{:?}", h.texts);
    h.press(Key::Escape);
    h.frame(vec![]);

    // letters typed with Alt never land in a focused text field
    h.app.tab = Tab::Songs;
    h.app.songs.song = sample_song();
    let before = h.app.songs.song.title.clone();
    h.app.focus_search = true;
    h.frame(vec![]);
    alt_typing(&mut h, "b");
    assert_eq!(h.app.tab, Tab::Bible);
    assert_eq!(h.app.songs.search, "");
    assert_eq!(h.app.songs.song.title, before);

    // End / Home scroll the settings page
    h.app.tab = Tab::Settings;
    h.frame(vec![]);
    h.frame(vec![]);
    assert!(h.texts.iter().any(|t| t == "General"));
    h.press(Key::End);
    h.frame(vec![]);
    assert!(!h.texts.iter().any(|t| t == "General"), "scrolled to the end");
    assert!(h.texts.iter().any(|t| t.contains("About")), "{:?}", h.texts);
    h.press(Key::Home);
    h.frame(vec![]);
    assert!(h.texts.iter().any(|t| t == "General"), "back at the top");
    h.press(Key::PageDown);
    h.frame(vec![]);
    assert!(!h.texts.iter().any(|t| t == "General"), "a page down");
    h.press(Key::PageUp);
    h.frame(vec![]);
    assert!(h.texts.iter().any(|t| t == "General"), "and back up");
    for _ in 0..8 {
        h.press(Key::ArrowDown);
    }
    h.frame(vec![]);
    assert!(!h.texts.iter().any(|t| t == "General"), "eight lines down");
    // in the service tab the arrows still present (not scroll)
    h.app.tab = Tab::Service;
    h.frame(vec![]);
    h.press(Key::ArrowDown);
    assert!(h.app.key_scroll_is_none());
}

#[test]
fn alert_is_drawn_running_across_and_ends() {
    use crate::render::{alert_pass_secs, alert_run_secs, draw_alert};
    use eframe::egui::{Pos2, Rect, Shape, vec2};
    let ctx = eframe::egui::Context::default();
    crate::render::install_fonts(&ctx, None);
    let rect = Rect::from_min_size(Pos2::ZERO, vec2(1280.0, 720.0));
    // where the text and strip are drawn at a moment
    let draw = |alert: &Alert, t: f32| -> (bool, Option<Rect>, Option<Pos2>) {
        let mut res = (false, None, None);
        let mut out = ctx.run_ui(Default::default(), |ui| {
            res.0 = draw_alert(ui.painter(), rect, alert, t);
        });
        out.textures_delta.clear();
        for cs in &out.shapes {
            match &cs.shape {
                Shape::Rect(r) if r.rect.width() >= 1279.0 => res.1 = Some(r.rect),
                Shape::Text(tx) => res.2 = Some(tx.pos),
                _ => {}
            }
        }
        res
    };
    let mut alert = Alert { text: "Hello".into(), style: AlertStyle::default(), duration: 1.0 };
    let (mut pass, mut run) = (0.0, None);
    ctx.run_ui(Default::default(), |ui| {
        pass = alert_pass_secs(ui.painter(), &alert);
        run = alert_run_secs(ui.painter(), &alert);
    })
    .textures_delta
    .clear();
    assert!(pass > 1280.0 / alert.style.speed, "{pass}");
    // a short timeout still runs once across
    assert_eq!(run, Some(pass));

    // right to left: starts at the right edge, moves left; strip at the bottom
    let (running, strip, start) = draw(&alert, 0.0);
    assert!(running);
    let strip = strip.unwrap();
    assert!((strip.bottom() - 720.0).abs() < 0.5, "{strip:?}");
    assert!((start.unwrap().x - 1280.0).abs() < 1.0, "{start:?}");
    let (_, _, later) = draw(&alert, 1.0);
    assert!(later.unwrap().x < start.unwrap().x - 100.0);
    // left to right, at the top
    alert.style.direction = AlertDirection::LeftToRight;
    alert.style.valign = VAlign::Top;
    let (_, strip, start) = draw(&alert, 0.0);
    assert!(strip.unwrap().top().abs() < 0.5);
    assert!(start.unwrap().x < 0.0, "starts outside the left edge");
    let (_, _, later) = draw(&alert, 1.0);
    assert!(later.unwrap().x > start.unwrap().x + 100.0);
    // middle
    alert.style.valign = VAlign::Middle;
    let (_, strip, _) = draw(&alert, 0.0);
    assert!((strip.unwrap().center().y - 360.0).abs() < 0.5);
    // after its time it is gone; duration 0 runs until stopped
    assert!(!draw(&alert, pass + 0.1).0);
    alert.duration = 0.0;
    assert!(draw(&alert, 10_000.0).0);
}

#[test]
fn ui_alert_window_send_and_stop() {
    use crate::app::{Dialog, Tab};
    use eframe::egui::{Key, Modifiers};
    let mut h = Harness::new();
    h.app.tab = Tab::Service;
    h.app.settings.alert_default_message = "Parking: car 1AB 2345".into();
    h.app.output_open = true;
    // F7 opens the window with the default message (also while typing in a field)
    h.press(Key::F7);
    h.frame(vec![]);
    assert!(matches!(h.app.dialog, Some(Dialog::Alert)));
    assert_eq!(h.app.alert_text, "Parking: car 1AB 2345");
    assert!(h.texts.iter().any(|t| t == "📢 Send message"), "{:?}", h.texts);
    // the text field has the focus; Tab moves on to the buttons and stays there
    let focused = |h: &Harness| h.ctx.memory(|m| m.focused());
    let field = focused(&h);
    assert!(field.is_some(), "the text field is focused when the window opens");
    h.frame(key(Key::Tab, Modifiers::NONE));
    h.frame(vec![]);
    let button = focused(&h);
    assert!(button.is_some() && button != field, "Tab moved the focus away from the text field");
    h.frame(vec![]);
    h.frame(vec![]);
    assert_eq!(focused(&h), button, "and the focus is not pulled back");
    // Shift+Tab goes back to the field, Enter there sends
    h.frame(key(Key::Tab, Modifiers::SHIFT));
    h.frame(vec![]);
    assert_eq!(focused(&h), field);
    // Enter sends
    h.app.alert_text = "Car 9Z9 9999".into();
    h.frame(key(Key::Enter, Modifiers::NONE));
    h.frame(vec![]);
    assert!(h.app.dialog.is_none());
    assert_eq!(h.app.alert.as_ref().map(|(a, _)| a.text.as_str()), Some("Car 9Z9 9999"));
    // shown in the status bar and drawn in the output window
    h.frame(vec![]);
    assert!(h.texts.iter().any(|t| t.starts_with("📢 Alert: Car 9Z9 9999")), "{:?}", h.texts);
    assert!(h.texts.iter().filter(|t| *t == "Car 9Z9 9999").count() >= 1, "{:?}", h.texts);
    // F7 again: the running alert can be edited or stopped
    h.press(Key::F7);
    h.frame(vec![]);
    assert_eq!(h.app.alert_text, "Car 9Z9 9999");
    assert!(h.texts.iter().any(|t| t == "■ Stop alert"));
    h.app.dialog = None;
    h.app.stop_alert();
    h.frame(vec![]);
    assert!(h.app.alert.is_none());
    assert!(!h.texts.iter().any(|t| t.starts_with("📢 Alert:")));
    // it ends by itself after its time
    h.app.send_alert("Short one");
    h.app.alert.as_mut().unwrap().1 = std::time::Instant::now() - std::time::Duration::from_secs(3600);
    h.frame(vec![]);
    assert!(h.app.alert.is_none(), "timed out");
    // timeout 0: until stopped
    h.app.settings.alert_style.timeout = 0.0;
    h.app.send_alert("Forever");
    h.app.alert.as_mut().unwrap().1 = std::time::Instant::now() - std::time::Duration::from_secs(3600);
    h.frame(vec![]);
    assert!(h.app.alert.is_some());
    h.app.stop_alert();
    // empty messages are not sent
    h.app.send_alert("   ");
    assert!(h.app.alert.is_none());
}

#[test]
fn network_outputs_are_off_by_default() {
    assert!(!crate::settings::Settings::default().network_enabled);
}

#[test]
fn video_duration_and_position() {
    assert_eq!(crate::video::parse_duration("  Duration: 00:03:12.48, start: 0.000000, bitrate: 1 kb/s"), Some(192.48));
    assert_eq!(crate::video::parse_duration("no such line"), None);
    assert_eq!(crate::video::format_time(192.48), "3:12");
    assert_eq!(crate::video::format_time(3725.0), "1:02:05");
    assert!(crate::video::is_video_name("abc.MP4") && !crate::video::is_video_name("abc.png"));
    let v = Video { title: "x".into(), media: "a.mp4".into(), poster: None, duration: 10.0, looped: false, muted: false };
    let mut s = VideoScene::new(&v);
    s.position = 4.0;
    assert_eq!(s.position_after(3.0), 7.0);
    assert_eq!(s.position_after(30.0), 10.0);
    assert!(s.ended_after(6.0) && !s.ended_after(5.0));
    s.looped = true;
    assert_eq!(s.position_after(8.0), 2.0);
    s.paused = true;
    assert_eq!(s.position_after(8.0), 4.0);
}

#[test]
fn network_output_receives_videos_in_pieces() {
    test_home();
    use crate::net::{ClientEvent, Server, run_client};
    use std::time::Duration;
    let port = std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
    let server = Server::start(port);
    let rx = run_client(Some(format!("127.0.0.1:{port}")), "video-canvas".into(), eframe::egui::Context::default());
    // a "video" larger than one piece (contents do not matter for sending)
    let bytes: Vec<u8> = (0..(2_500_000u32)).map(|i| (i % 251) as u8).collect();
    let media = crate::storage::store_media_bytes(&bytes, "mp4").unwrap();
    let v = Video { title: "clip".into(), media: media.clone(), poster: None, duration: 5.0, looped: false, muted: false };
    server.broadcast(&Scene::Video(VideoScene::new(&v)));
    let deadline = std::time::Instant::now() + Duration::from_secs(20);
    let (mut got_asset, mut got_scene) = (false, false);
    while !(got_asset && got_scene) && std::time::Instant::now() < deadline {
        match rx.recv_timeout(Duration::from_millis(200)) {
            Ok(ClientEvent::Asset(n)) if n == media => got_asset = true,
            Ok(ClientEvent::Scene(Scene::Video(s))) if s.media == media => got_scene = true,
            _ => {}
        }
    }
    assert!(got_asset && got_scene, "asset {got_asset}, scene {got_scene}");
    assert_eq!(std::fs::read(crate::storage::paths().client_media().join(&media)).unwrap(), bytes);
}

#[test]
fn service_file_carries_videos() {
    test_home();
    let bytes = b"not really a video".to_vec();
    let media = crate::storage::store_media_bytes(&bytes, "webm").unwrap();
    let mut service = Service::default();
    let v = Video { title: "clip".into(), media: media.clone(), poster: None, duration: 1.5, looped: true, muted: true };
    service.items.push(ServiceItem::new(ItemKind::Video(v)));
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target/test-home/video.ohsvc");
    formats::save_service(&path, &service).unwrap();
    std::fs::remove_file(crate::storage::media_path(&media)).unwrap();
    let back = formats::load_service(&path).unwrap();
    assert_eq!(back, service);
    assert_eq!(std::fs::read(crate::storage::media_path(&media)).unwrap(), bytes);
}

#[test]
fn ui_arrows_move_through_an_open_menu_like_tab() {
    use crate::app::{ItemRef, Tab};
    use eframe::egui::{Event, Key, Modifiers};
    let mut h = Harness::new();
    h.app.tab = Tab::Service;
    h.app.service.items.push(ServiceItem::new(ItemKind::Song(sample_song())));
    h.app.sel_item = Some(ItemRef::Service(0));
    h.app.sel_slide = 0;
    h.frame(vec![]);
    let open_menu = |h: &mut Harness, digit: Key| {
        h.frame(vec![Event::ModifiersChanged(Modifiers::ALT)]);
        h.frame(key(digit, Modifiers::ALT));
        h.frame(vec![Event::ModifiersChanged(Modifiers::NONE)]);
        h.frame(vec![]);
        h.frame(vec![]);
    };
    let focused = |h: &Harness| h.ctx.memory(|m| m.focused());
    for (digit, item) in [(Key::Num1, "Recent services"), (Key::Num2, "Alert…")] {
        // Alt+1 = File, Alt+2 = Output
        open_menu(&mut h, digit);
        let shown = |h: &Harness| h.texts.iter().any(|t| t.starts_with(item));
        assert!(shown(&h), "menu not open: {:?}", h.texts);
        h.press(Key::ArrowDown);
        h.frame(vec![]);
        assert!(shown(&h), "↓ closed the menu");
        assert_eq!(h.app.sel_slide, 0, "↓ must not step the slides while a menu is open");
        let first = focused(&h);
        assert!(first.is_some(), "↓ focuses an item like Tab");
        h.press(Key::ArrowDown);
        h.frame(vec![]);
        let second = focused(&h);
        assert_ne!(second, first, "↓ moves to the next item");
        h.press(Key::ArrowUp);
        h.frame(vec![]);
        assert_eq!(focused(&h), first, "↑ moves back like Shift+Tab");
        assert!(shown(&h));
        h.press(Key::Escape);
        h.frame(vec![]);
    }
    // with no menu open ↓ steps the slides again
    h.press(Key::ArrowDown);
    assert_eq!(h.app.sel_slide, 1);
}
