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
        eframe::App::raw_input_hook(app, &self.ctx, &mut input);
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
    // the clock section with its preview and the output window drawing the clock
    h.app.settings.clock = ClockStyle { show: true, plain: true, ..Default::default() };
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
            Dialog::LeaveOutput(crate::app::OutputChange::Windowed),
            Dialog::LeaveOutput(crate::app::OutputChange::Close),
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
    let bytes: Vec<u8> = (0..2_500_000u32).map(|i| (i % 251) as u8).collect();
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

#[test]
fn ui_tab_scrolls_the_focused_item_into_view() {
    use crate::app::Tab;
    use eframe::egui::{Key, Order};
    let mut h = Harness::new();
    h.app.tab = Tab::Settings;
    h.frame(vec![]);
    let mut hidden_at_first = 0;
    let mut checked = 0;
    for _ in 0..120 {
        h.press(Key::Tab);
        let Some(id) = h.ctx.memory(|m| m.focused()) else { continue };
        let first = h.ctx.read_response(id).unwrap();
        if first.layer_id.order != Order::Background {
            continue;
        }
        if first.interact_rect != first.rect {
            hidden_at_first += 1;
        }
        // the scroll (animated) follows within a few frames
        for _ in 0..40 {
            h.frame(vec![]);
        }
        let r = h.ctx.read_response(id).unwrap();
        let visible = r.interact_rect.expand(0.5).contains_rect(r.rect);
        assert!(visible, "the item focused with Tab stays out of view: {:?} shown {:?}", r.rect, r.interact_rect);
        checked += 1;
    }
    assert!(checked > 20, "Tab went through the Settings ({checked})");
    assert!(hidden_at_first > 0, "the Settings are longer than the window: some item was out of view first");
}

#[test]
fn ui_tab_out_of_an_open_menu_closes_it() {
    use crate::app::Tab;
    use eframe::egui::{Event, Key, Modifiers, Order, Popup};
    let mut h = Harness::new();
    h.app.tab = Tab::Service;
    h.frame(vec![]);
    // Alt+1 = the File menu
    h.frame(vec![Event::ModifiersChanged(Modifiers::ALT)]);
    h.frame(key(Key::Num1, Modifiers::ALT));
    h.frame(vec![Event::ModifiersChanged(Modifiers::NONE)]);
    h.frame(vec![]);
    h.frame(vec![]);
    assert!(Popup::is_any_open(&h.ctx), "the File menu is open");
    let mut in_menu = 0;
    for _ in 0..40 {
        h.press(Key::Tab);
        h.frame(vec![]);
        let Some(id) = h.ctx.memory(|m| m.focused()) else { continue };
        let order = h.ctx.read_response(id).unwrap().layer_id.order;
        if Popup::is_id_open(&h.ctx, id.with("popup")) {
            // the File button itself (Alt+1 focuses it): the menu stays
            assert_eq!(in_menu, 0);
            continue;
        }
        if order == Order::Foreground {
            assert!(Popup::is_any_open(&h.ctx), "Tab inside the menu keeps it open");
            in_menu += 1;
        } else {
            assert!(!Popup::is_any_open(&h.ctx), "Tab onto the page behind closes the menu");
            assert!(!h.texts.iter().any(|t| t.starts_with("Recent services")), "the menu is not drawn any more");
            assert!(in_menu > 0, "Tab first went through the menu items");
            return;
        }
    }
    panic!("Tab never left the menu");
}

// ---------------------------------------------------------------------------
// Clock on the output

#[test]
fn encoded_clock_starts_all_red_and_goes_round_in_12_hours() {
    use crate::clock::{decode, encode};
    assert_eq!(encode(0), [0, 0, 0]);
    assert_eq!(encode(12 * 3600), [0, 0, 0], "noon looks like midnight");
    assert_eq!(encode(12 * 3600 - 1), [3, 3, 3], "all brown just before 12");
    // 4:30 = 3 h (green) + 90 min (blue) + 0 (red)
    assert_eq!(encode(4 * 3600 + 30 * 60), [1, 2, 0]);
    assert_eq!(encode(16 * 3600 + 30 * 60), [1, 2, 0]);
    // the last dot changes every 11¼ minutes
    assert_eq!(encode(11 * 60 + 14), [0, 0, 0]);
    assert_eq!(encode(11 * 60 + 15), [0, 0, 1]);
    // only one dot (or a run of trailing ones) changes per step, and every state is used once
    let mut seen = std::collections::HashSet::new();
    for step in 0..64u32 {
        let secs = step * 675;
        let dots = encode(secs);
        assert!(seen.insert(dots));
        assert_eq!(decode(dots), secs, "decode is the start of the 11¼ minute window");
        assert_eq!(encode(secs + 674), dots);
    }
    assert_eq!(seen.len(), 64);
}

#[test]
fn clock_colour_order_is_red_green_blue_brown() {
    use crate::clock::{COLOR_KEYS, COLORS};
    assert_eq!(COLOR_KEYS, ["clock.red", "clock.green", "clock.blue", "clock.brown"]);
    let [r, g, b, _] = COLORS;
    assert!(r.r() > r.g() && r.r() > r.b(), "0 is red");
    assert!(g.g() > g.r() && g.g() > g.b(), "1 is green");
    assert!(b.b() > b.r() && b.b() > b.g(), "2 is blue");
}

#[test]
fn clock_texts_and_repaint() {
    use crate::clock::{face_time, plain_time, repaint_in};
    assert_eq!(plain_time(14 * 3600 + 5 * 60 + 59), "14:05");
    assert_eq!(face_time(0), "12:00");
    assert_eq!(face_time(16 * 3600 + 30 * 60), "4:30");
    let mut style = ClockStyle { show: true, ..Default::default() };
    // encoded only: wait for the next dot change (675 s steps)
    assert_eq!(repaint_in(&style, 600).as_secs(), 75);
    style.plain = true;
    assert_eq!(repaint_in(&style, 610).as_secs(), 50);
}

#[test]
fn clock_settings_default_off_and_old_settings_load() {
    let s: crate::settings::Settings = serde_json::from_str("{}").unwrap();
    assert!(!s.clock.show && !s.clock.plain);
    assert_eq!(s.clock.opacity, 100);
    let s: crate::settings::Settings = serde_json::from_str(r#"{"clock":{"show":true,"position":"TopMiddle"}}"#).unwrap();
    assert!(s.clock.show);
    assert_eq!(s.clock.position, ClockPosition::TopMiddle);
    assert_eq!(s.clock.size, ClockSize::Medium);
}

// ---------------------------------------------------------------------------
// Start: screens, loading behind the splash screen, renderer choice, packaging versions

fn monitor(x: i32, y: i32, w: u32, h: u32, primary: bool) -> crate::screens::Monitor {
    crate::screens::Monitor { name: format!("{w}x{h}"), pos: (x, y), size: (w, h), scale: 1.0, primary }
}

#[test]
fn screens_program_on_primary_output_on_second() {
    use crate::screens::{Placement, plan};
    let a = monitor(0, 0, 1920, 1080, true);
    let b = monitor(1920, 0, 1280, 720, false);
    assert_eq!(plan(&[a.clone(), b.clone()], Some(0)), Placement { main: Some(0), output: Some(1) });
    // the primary screen listed second, the window opened on the other one
    assert_eq!(plan(&[b.clone(), a.clone()], Some(0)), Placement { main: Some(1), output: Some(0) });
    // a projector left of the primary screen (negative coordinates)
    let left = monitor(-1024, 0, 1024, 768, false);
    assert_eq!(plan(&[a.clone(), left], None), Placement { main: Some(0), output: Some(1) });
    // three screens: the first other one
    let c = monitor(3200, 0, 800, 600, false);
    assert_eq!(plan(&[a.clone(), b.clone(), c], Some(0)).output, Some(1));
}

#[test]
fn screens_fallbacks() {
    use crate::screens::{Placement, plan};
    let a = monitor(0, 0, 1920, 1080, true);
    // one screen: no output screen
    assert_eq!(plan(std::slice::from_ref(&a), Some(0)), Placement { main: Some(0), output: None });
    // no screens known (detection failed)
    assert_eq!(plan(&[], None), Placement { main: None, output: None });
    // mirrored projector: the same area as the main screen
    let mirror = monitor(0, 0, 1920, 1080, false);
    assert_eq!(plan(&[a.clone(), mirror], Some(0)).output, None);
    // partly overlapping screens are not "another" screen either
    let overlap = monitor(1000, 500, 1920, 1080, false);
    assert_eq!(plan(&[a, overlap], Some(0)).output, None);
    // Wayland: no primary screen; the one the window is on counts as primary
    let w1 = monitor(0, 0, 1920, 1080, false);
    let w2 = monitor(1920, 0, 1920, 1080, false);
    assert_eq!(plan(&[w1.clone(), w2.clone()], Some(1)), Placement { main: Some(1), output: Some(0) });
    // ...and without that, the first one; an out-of-range current index is ignored
    assert_eq!(plan(&[w1.clone(), w2.clone()], None), Placement { main: Some(0), output: Some(1) });
    assert_eq!(plan(&[w1, w2], Some(7)), Placement { main: Some(0), output: Some(1) });
    // touching screens (side by side) do not overlap
    let a = monitor(0, 0, 100, 100, true);
    let b = monitor(100, 0, 100, 100, false);
    assert_eq!(plan(&[a, b], Some(0)).output, Some(1));
}

#[test]
fn screens_centering_and_containment() {
    use crate::screens::{centered_on, contains};
    let m = monitor(1920, 0, 1280, 720, false);
    assert_eq!(centered_on(&m, (640, 360)), (1920 + 320, 180));
    // bigger than the screen: its top-left corner
    assert_eq!(centered_on(&m, (2000, 1000)), (1920, 0));
    assert!(contains(&m, (1920, 0)));
    assert!(contains(&m, (3199, 719)));
    assert!(!contains(&m, (3200, 0)));
    assert!(!contains(&m, (1919, 10)));
}

#[test]
fn startup_loading_reports_progress_and_loads_the_library() {
    test_home();
    let mut song = Song::new();
    song.title = "Startup progress test".into();
    let mut lib = crate::storage::Library::load();
    lib.save_song(&song).unwrap();
    let mut seen = Vec::new();
    let loaded = crate::app::startup_load(&mut |f| seen.push(f));
    assert!(loaded.lib.songs.iter().any(|s| s.id == song.id), "the saved song is loaded");
    assert_eq!(seen.first(), Some(&0.0));
    assert_eq!(seen.last(), Some(&1.0));
    assert!(seen.windows(2).all(|w| w[0] <= w[1]), "progress only grows: {seen:?}");
    assert!(seen.iter().all(|f| (0.0..=1.0).contains(f)));
    assert!(seen.len() > 3, "progress per file, not only start and end: {seen:?}");
}

#[test]
fn startup_progress_fraction() {
    use crate::app::startup_fraction as fraction;
    assert_eq!(fraction(0, 0), 1.0);
    assert_eq!(fraction(0, 10), 0.0);
    assert_eq!(fraction(5, 10), 0.5);
    assert_eq!(fraction(15, 10), 1.0);
}

#[test]
fn renderer_order_after_failed_starts() {
    use crate::start_guard::{order, other, parse};
    use eframe::Renderer::{Glow, Wgpu};
    assert_eq!(order(None, None), vec![Wgpu, Glow]);
    // what worked last time goes first
    assert_eq!(order(Some(Glow), None), vec![Glow, Wgpu]);
    assert_eq!(order(Some(Wgpu), None), vec![Wgpu, Glow]);
    // a start that never showed a window puts its renderer last
    assert_eq!(order(None, Some(Wgpu)), vec![Glow, Wgpu]);
    assert_eq!(order(Some(Wgpu), Some(Wgpu)), vec![Glow, Wgpu]);
    assert_eq!(order(Some(Glow), Some(Glow)), vec![Wgpu, Glow]);
    assert_eq!(other(Wgpu), Glow);
    assert_eq!(other(Glow), Wgpu);
    assert_eq!(parse("wgpu\n"), Some(Wgpu));
    assert_eq!(parse(" glow "), Some(Glow));
    assert_eq!(parse("vulkan"), None);
    assert_eq!(parse(""), None);
}

#[test]
fn packaging_versions_match_cargo() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let version = env!("CARGO_PKG_VERSION");
    let iss = std::fs::read_to_string(root.join("packaging/windows/oh-presenter.iss")).unwrap();
    assert!(iss.contains(&format!("#define AppVersion \"{version}\"")), "oh-presenter.iss AppVersion must be {version}");
    let metainfo = std::fs::read_to_string(root.join("packaging/flatpak/cz.oh.Presenter.metainfo.xml")).unwrap();
    assert!(metainfo.contains(&format!("<release version=\"{version}\"")), "the newest metainfo release must be {version}");
}

#[test]
fn installer_upgrades_cleanly() {
    let iss = std::fs::read_to_string(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("packaging/windows/oh-presenter.iss")).unwrap();
    // the same AppId is what makes a new version replace the old one
    assert!(iss.contains("AppId={{6F1B6C2E-4E0B-4C8E-9B0A-0E5D3B7A9C41}"));
    // every file the installer ships is removed first, so no file of an old version stays
    let install_delete = iss.split("[InstallDelete]").nth(1).expect("[InstallDelete] section").split("\n[").next().unwrap();
    for line in iss.lines().filter(|l| l.starts_with("Source:")) {
        let dest = line
            .split("DestName: \"")
            .nth(1)
            .and_then(|s| s.split('"').next())
            .unwrap_or_else(|| line.split('"').nth(1).unwrap().rsplit('\\').next().unwrap());
        let dest = dest.replace("{#AppExe}", "oh-presenter-rs.exe");
        assert!(install_delete.contains(&format!("{{app}}\\{dest}")), "[InstallDelete] misses {dest}");
    }
    // user data is never deleted
    assert!(!iss.contains("[UninstallDelete]"));
    assert!(!iss.to_lowercase().contains("{userappdata}") && !iss.to_lowercase().contains("{localappdata}"));
    // the firewall rule is deleted before it is added, so upgrades do not pile up copies
    let delete = iss.find("advfirewall firewall delete rule").expect("delete before add");
    let add = iss.find("advfirewall firewall add rule").unwrap();
    assert!(delete < add);
}

#[test]
fn screens_wait_for_the_window_on_wayland() {
    use crate::screens::must_wait;
    let w1 = monitor(0, 0, 1920, 1080, false);
    let w2 = monitor(1920, 0, 1920, 1080, false);
    // Wayland before the window is shown: no primary, window's screen unknown -> wait
    assert!(must_wait(&[w1.clone(), w2.clone()], None));
    assert!(must_wait(&[w1.clone(), w2.clone()], Some(5)));
    // known screen, a primary screen, or nothing to choose from -> no need to wait
    assert!(!must_wait(&[w1.clone(), w2.clone()], Some(1)));
    assert!(!must_wait(&[monitor(0, 0, 1920, 1080, true), w2], None));
    assert!(!must_wait(std::slice::from_ref(&w1), None));
    assert!(!must_wait(&[], None));
}

// ---------------------------------------------------------------------------
// Video through VLC (when ffmpeg is missing)

#[test]
fn video_backend_choice() {
    use crate::video::{Backend, choose};
    assert_eq!(choose(None, true, || true), Some(Backend::Ffmpeg), "ffmpeg first");
    assert_eq!(choose(None, false, || true), Some(Backend::Vlc), "VLC without ffmpeg");
    assert_eq!(choose(None, false, || false), None);
    assert_eq!(choose(Some("vlc"), true, || true), Some(Backend::Vlc), "forced");
    assert_eq!(choose(Some("VLC"), true, || false), None, "forced but missing");
    assert_eq!(choose(Some("ffmpeg"), false, || true), None);
    assert_eq!(choose(Some("other"), false, || true), Some(Backend::Vlc));
}

#[test]
fn vlc_pictures_fit_and_letterbox() {
    use crate::vlc::{bgrx_to_rgba, fit, supported_version};
    assert!(supported_version("3.0.20 Vetinari"));
    assert!(!supported_version("4.0.0-dev Otto Chriek"));
    assert!(!supported_version("x"));
    assert_eq!(fit((1920, 1080), (1280, 720)), (1280, 720));
    assert_eq!(fit((640, 480), (1280, 720)), (960, 720), "4:3 gets bars left and right");
    assert_eq!(fit((1000, 200), (640, 4096)), (640, 128), "poster: 640 wide");
    assert_eq!(fit((0, 0), (16, 9)), (16, 9));
    // a 1×1 blue (B G R X) pixel in a 3×1 picture: black, blue, black
    let out = bgrx_to_rgba(&[255, 0, 0, 0], (1, 1), (3, 1));
    assert_eq!(out, vec![0, 0, 0, 255, 0, 0, 255, 255, 0, 0, 0, 255]);
    let same = bgrx_to_rgba(&[1, 2, 3, 0, 4, 5, 6, 0], (2, 1), (2, 1));
    assert_eq!(same, vec![3, 2, 1, 255, 6, 5, 4, 255]);
}

/// Plays a real video through libvlc (only where VLC 3 and the sample video are installed).
#[test]
fn vlc_plays_a_real_video() {
    let sample = std::path::Path::new("/usr/share/help/C/gnome-help/figures/display-dual-monitors.webm");
    if !sample.is_file() || !crate::vlc::available() {
        eprintln!("skipped: no VLC or no sample video");
        return;
    }
    let (duration, poster) = crate::vlc::probe(sample).expect("probe");
    assert!(duration > 1.0, "length {duration}");
    let poster = image::load_from_memory(&poster.expect("a still picture")).expect("PNG");
    assert_eq!(poster.width(), 640);
    // the picture, in real time, letterboxed into 640×360
    let frames = std::sync::Arc::new(crate::vlc::Frames::default());
    let video = crate::vlc::Video::start(sample, 0.0, false, false, (640, 360), frames.clone()).expect("start");
    let begin = std::time::Instant::now();
    while frames.count.load(std::sync::atomic::Ordering::SeqCst) < 10 && begin.elapsed().as_secs() < 15 {
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    let n = frames.count.load(std::sync::atomic::Ordering::SeqCst);
    assert!(n >= 10, "frames: {n}");
    let (_, size, rgba) = frames.latest.lock().unwrap().take().unwrap();
    assert_eq!(size, (640, 360));
    assert_eq!(rgba.len(), 640 * 360 * 4);
    assert!(rgba.chunks(4).any(|p| p[0] > 40 || p[1] > 40 || p[2] > 40), "not all black");
    // real time, not as fast as possible: 10 frames take a moment
    assert!(begin.elapsed().as_millis() > 150);
    assert!(!video.finished());
    drop(video);
    // paused: one frame, then finished
    let frames = std::sync::Arc::new(crate::vlc::Frames::default());
    let paused = crate::vlc::Video::start(sample, 2.0, false, true, (320, 180), frames.clone()).expect("start");
    let begin = std::time::Instant::now();
    while !paused.finished() && begin.elapsed().as_secs() < 15 {
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    assert!(paused.finished(), "the paused frame arrived");
    drop(paused);
    // what the output window uses: the chosen player (VLC here when ffmpeg is missing) gives textures
    let ctx = eframe::egui::Context::default();
    let mut player = crate::video::Player::start(sample, 0.0, true, false, (640, 360)).expect("player");
    let begin = std::time::Instant::now();
    let mut texture = None;
    while texture.is_none() && begin.elapsed().as_secs() < 15 {
        let mut out = ctx.run_ui(Default::default(), |_| {});
        out.textures_delta.clear();
        texture = player.texture(&ctx).map(|t| t.size());
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    assert_eq!(texture, Some([640, 360]));
    ctx.run_ui(Default::default(), |_| {}).textures_delta.clear();
    // the sound player starts and stops (no sound track in this sample: nothing is heard)
    drop(crate::video::Audio::start(sample, 0.0, false).expect("sound"));
}

#[test]
fn bible_txt_round_trip_and_loose_text() {
    use crate::bible::import_txt;
    let txt = "# name: Test Bible\n# language: en\nGenesis 1:1 In the beginning.\nGenesis 1:2 And the earth\nwas without form.\n1 Samuel 3:4 Samuel!\n";
    let b = import_txt(txt, "file");
    assert_eq!(b.meta.name, "Test Bible");
    assert_eq!(b.verse_count(), 3);
    assert_eq!(b.books[0].number, 1);
    assert_eq!(b.books[1].number, 9, "1 Samuel found by name");
    assert_eq!(b.books[0].chapters[0].verses[1].text, "And the earth was without form.", "a wrapped line continues the verse");
    // written out and read back: the same verses
    let again = import_txt(&b.to_txt(), "other");
    assert_eq!(again.meta.name, "Test Bible");
    assert_eq!(again.verse_count(), 3);
    assert_eq!(again.books[1].chapters[0].number, 3);
    assert_eq!(again.books[1].chapters[0].verses[0].number, 4);
    // any text: headings, verse numbers, plain lines
    let loose = import_txt("Jan\nKapitola 3\n16 Neboť tak Bůh miloval svět\nNumbers had 1,000 sheep here\nplain line", "loose");
    assert_eq!(loose.meta.name, "loose");
    assert_eq!(loose.books[0].number, 43);
    assert_eq!(loose.books[0].chapters[0].number, 3);
    assert_eq!(loose.books[0].chapters[0].verses[0].number, 16);
    assert!(loose.verse_count() >= 1);
    let prose = import_txt("first\nsecond", "prose");
    assert_eq!(prose.verse_count(), 2, "unnumbered lines become verses");
}

#[test]
fn ui_live_mode_on_shows_the_selected_slide() {
    use eframe::egui::Key;
    let mut h = Harness::new();
    h.app.service.items.push(ServiceItem::new(ItemKind::Song(sample_song())));
    h.app.tab = crate::app::Tab::Service;
    h.frame(vec![]);
    h.press(Key::L); // off
    assert!(!h.app.settings.live_follows_selection);
    h.press(Key::ArrowDown);
    h.press(Key::ArrowDown);
    assert_eq!(h.app.live_pos, None, "preview mode: nothing went live");
    h.press(Key::L); // on: the selected slide goes live
    assert!(h.app.settings.live_follows_selection);
    assert_eq!(h.app.live_pos, Some((crate::app::ItemRef::Service(0), 1)));
}

#[test]
fn ui_tab_key_starts_at_the_open_tab() {
    use crate::app::Tab;
    use eframe::egui::{Key, Modifiers};
    let mut h = Harness::new();
    h.app.tab = Tab::Bible;
    h.frame(vec![]);
    h.frame(vec![]);
    let id = |h: &Harness, t: Tab| h.app.alt_nav.tab_ids.iter().find(|(x, _)| *x == t).unwrap().1;
    h.frame(key(Key::Tab, Modifiers::NONE));
    assert_eq!(h.ctx.memory(|m| m.focused()), Some(id(&h, Tab::Settings)), "Tab from Bible goes to Settings");
    // nothing focused again: Shift+Tab goes back from Bible
    h.ctx.memory_mut(|m| m.surrender_focus(id(&h, Tab::Settings)));
    h.frame(vec![]);
    for _ in 0..3 {
        h.frame(key(Key::Tab, Modifiers::SHIFT));
        h.frame(vec![]);
    }
    assert_eq!(h.ctx.memory(|m| m.focused()), Some(id(&h, Tab::Service)), "3× Shift+Tab from Bible");
}

#[test]
fn ui_service_border_moves_with_arrows() {
    use eframe::egui::{Key, Modifiers, containers::panel::PanelState};
    let mut h = Harness::new();
    h.app.tab = crate::app::Tab::Service;
    h.frame(vec![]);
    h.frame(vec![]);
    let (left, right) = h.app.service_panel_ids().unwrap();
    let width = |h: &Harness, id| PanelState::load(&h.ctx, id).expect("the panel id is egui's").outer_rect.width();
    let (l0, r0) = (width(&h, left), width(&h, right));
    h.ctx.memory_mut(|m| m.request_focus(left.with("__resize")));
    h.frame(vec![]);
    h.frame(key(Key::ArrowRight, Modifiers::NONE));
    h.frame(vec![]);
    assert!((width(&h, left) - (l0 + 10.0)).abs() < 1.5, "{} -> {}", l0, width(&h, left));
    h.frame(key(Key::ArrowLeft, Modifiers::SHIFT));
    h.frame(vec![]);
    assert!((width(&h, left) - (l0 - 40.0)).abs() < 1.5);
    assert_eq!(h.app.sel_item, None, "the arrows did not step slides");
    h.ctx.memory_mut(|m| m.request_focus(right.with("__resize")));
    h.frame(vec![]);
    h.frame(key(Key::ArrowLeft, Modifiers::NONE));
    h.frame(vec![]);
    assert!((width(&h, right) - (r0 + 10.0)).abs() < 1.5, "moving the right border left widens the live column");
    // Ctrl+Shift+→ moves between the columns
    h.ctx.memory_mut(|m| m.surrender_focus(right.with("__resize")));
    h.frame(vec![]);
    h.frame(key(Key::ArrowRight, Modifiers::CTRL | Modifiers::SHIFT));
    h.frame(vec![]);
    let f = h.ctx.memory(|m| m.focused()).and_then(|f| h.ctx.read_response(f)).expect("focused");
    assert!(f.rect.center().x < 400.0, "the items column first");
    for _ in 0..2 {
        h.frame(key(Key::ArrowRight, Modifiers::CTRL | Modifiers::SHIFT));
        h.frame(vec![]);
    }
    let f = h.ctx.memory(|m| m.focused()).and_then(|f| h.ctx.read_response(f)).expect("focused");
    assert!(f.rect.center().x > 1000.0, "the live column (no item selected: the slides column is skipped)");
}

#[test]
fn ui_video_keys_on_the_focused_preview() {
    use eframe::egui::{Key, Modifiers};
    let mut h = Harness::new();
    h.app.tab = crate::app::Tab::Service;
    let v = Video { title: "v".into(), media: "none.mp4".into(), poster: None, duration: 60.0, looped: false, muted: true };
    h.app.set_live(Scene::Video(VideoScene::new(&v)), None);
    h.frame(vec![]);
    h.ctx.memory_mut(|m| m.request_focus(crate::app::App::video_preview_id_for_test()));
    h.frame(vec![]);
    h.frame(key(Key::Space, Modifiers::NONE));
    let Scene::Video(s) = &h.app.live else { panic!() };
    assert!(s.paused, "Space pauses");
    h.frame(key(Key::ArrowRight, Modifiers::NONE));
    h.frame(key(Key::ArrowRight, Modifiers::NONE));
    let Scene::Video(s) = &h.app.live else { panic!() };
    assert!((s.position - 10.0).abs() < 0.5, "{}", s.position);
    h.frame(key(Key::ArrowLeft, Modifiers::NONE));
    let Scene::Video(s) = &h.app.live else { panic!() };
    assert!((s.position - 5.0).abs() < 0.5);
    h.frame(key(Key::Space, Modifiers::NONE));
    let Scene::Video(s) = &h.app.live else { panic!() };
    assert!(!s.paused, "Space plays again");
}

#[test]
fn clock_sizes_and_old_settings() {
    let s: crate::settings::Settings = serde_json::from_str(r#"{"clock":{"show":true,"size":"Large"}}"#).unwrap();
    assert_eq!(s.clock.size, ClockSize::Medium, "the removed largest size becomes the new largest");
    let s: crate::settings::Settings = serde_json::from_str(r#"{"clock":{"size":"Tiny"}}"#).unwrap();
    assert_eq!(s.clock.size, ClockSize::Tiny);
}

#[test]
fn image_opacity_defaults_to_full_and_old_designs_load() {
    let o: ObjectKind = serde_json::from_str(r#"{"Image":{"media":"a.png"}}"#).unwrap();
    assert_eq!(o, ObjectKind::Image { media: "a.png".into(), opacity: 1.0 });
    assert_eq!(serde_json::to_string(&o).unwrap(), r#"{"Image":{"media":"a.png"}}"#, "full opacity is not written");
    let half = ObjectKind::Image { media: "a.png".into(), opacity: 0.5 };
    assert_eq!(serde_json::from_str::<ObjectKind>(&serde_json::to_string(&half).unwrap()).unwrap(), half);
}

#[test]
fn output_goes_on_the_last_used_screen_else_another_one() {
    use crate::screens::{ScreenId, output_screen};
    let primary = monitor(0, 0, 1920, 1080, true);
    let second = monitor(1920, 0, 1280, 720, false);
    let third = monitor(-1920, 0, 1600, 900, false);
    let mirror = monitor(0, 0, 1920, 1080, false);
    let all = vec![primary.clone(), second.clone(), third.clone()];
    assert_eq!(output_screen(&all, Some(0), None), Some(1), "the first other screen");
    assert_eq!(output_screen(&all, Some(0), Some(&ScreenId::of(&third))), Some(2), "the last used one");
    assert_eq!(output_screen(&all, Some(2), Some(&ScreenId::of(&third))), Some(0), "never the program's screen");
    // the program was started on the second screen: still another screen than its own
    assert_eq!(output_screen(&all, Some(1), None), Some(0));
    // a remembered screen found by its name after it moved
    let moved = ScreenId { pos: (5000, 0), ..ScreenId::of(&third) };
    assert_eq!(output_screen(&all, Some(0), Some(&moved)), Some(2));
    // a gone screen is forgotten
    let gone = ScreenId { name: "gone".into(), pos: (9000, 0), size: (800, 600) };
    assert_eq!(output_screen(&all, Some(0), Some(&gone)), Some(1));
    // one screen, or only a mirror of it: none
    assert_eq!(output_screen(std::slice::from_ref(&primary), Some(0), None), None);
    assert_eq!(output_screen(&[primary.clone(), mirror], Some(0), None), None);
    assert_eq!(output_screen(&[], None, None), None);
}

#[test]
fn output_fullscreen_steps_recover_from_the_windows_quarter_screen() {
    use crate::screens::{Seen, Step, fullscreen_ok, fullscreen_step, windowed_step};
    let second = monitor(1920, 0, 1280, 720, false);
    let fine = Seen { outer: Some((1920, 0, 1280, 720)), inner_size: Some((1280, 720)), fullscreen: Some(true) };
    assert!(fullscreen_ok(&second, &fine));
    assert_eq!(fullscreen_step(&second, &fine, 0), None, "already there: nothing to do");
    // a new (windowed) window on the primary screen: first the direct way
    let new = Seen { outer: Some((480, 270, 960, 540)), inner_size: Some((960, 540)), fullscreen: Some(false) };
    assert_eq!(fullscreen_step(&second, &new, 0), Some(Step::SetMonitor));
    // what Windows did: "full screen", but on the primary screen at its first size
    let quarter = Seen { fullscreen: Some(true), ..new };
    assert!(!fullscreen_ok(&second, &quarter));
    assert_eq!(fullscreen_step(&second, &quarter, 1), Some(Step::LeaveFullscreen));
    // windowed again on the wrong screen: moved onto the output's screen, centered
    assert_eq!(fullscreen_step(&second, &new, 2), Some(Step::MoveTo((1920 + 160, 90))));
    // there now: full screen where it is
    let moved = Seen { outer: Some((2080, 90, 960, 540)), ..new };
    assert_eq!(fullscreen_step(&second, &moved, 3), Some(Step::SetMonitor));
    assert_eq!(fullscreen_step(&second, &moved, 4), Some(Step::EnterFullscreen));
    // full screen on the right screen but not filling it (a scaling mistake): out and in again
    let small = Seen { outer: Some((1920, 0, 640, 360)), inner_size: Some((640, 360)), fullscreen: Some(true) };
    assert_eq!(fullscreen_step(&second, &small, 5), Some(Step::LeaveFullscreen));
    // Wayland: no positions; the size tells
    let wayland = Seen { outer: None, inner_size: Some((1280, 720)), fullscreen: Some(true) };
    assert!(fullscreen_ok(&second, &wayland));
    // a windowed output (F5) opens on the output's screen, once; moved by the user it stays
    assert_eq!(windowed_step(Some(&second), &new, 0), Some(Step::MoveTo((1920 + 160, 90))));
    assert_eq!(windowed_step(Some(&second), &new, 2), None);
    assert_eq!(windowed_step(Some(&second), &moved, 0), None);
    assert_eq!(windowed_step(None, &quarter, 0), Some(Step::LeaveFullscreen));
}

#[test]
fn last_output_screen_setting_loads() {
    let s: crate::settings::Settings = serde_json::from_str("{}").unwrap();
    assert_eq!(s.output_screen, None);
    let s: crate::settings::Settings =
        serde_json::from_str(r#"{"output_screen":{"name":"HDMI-1","pos":[1920,0],"size":[1280,720]}}"#).unwrap();
    assert_eq!(s.output_screen.unwrap().name, "HDMI-1");
}

// ---------------------------------------------------------------------------
// plan.txt: output full screen on another screen, freeze, video keys, keys on the Service tab

fn sample_video(duration: f64) -> ServiceItem {
    ServiceItem::new(ItemKind::Video(Video {
        title: "clip".into(),
        media: "none.mp4".into(),
        poster: None,
        duration,
        looped: false,
        muted: false,
    }))
}

fn live_video(app: &crate::app::App) -> VideoScene {
    match &app.live {
        Scene::Video(v) => v.clone(),
        other => panic!("live is not a video: {other:?}"),
    }
}

/// A click (press + release) at `pos`, in points of the 1400 x 900 window.
fn click_at(pos: eframe::egui::Pos2) -> Vec<eframe::egui::Event> {
    use eframe::egui::{Event, Modifiers, PointerButton};
    vec![
        Event::PointerMoved(pos),
        Event::PointerButton { pos, button: PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE },
        Event::PointerButton { pos, button: PointerButton::Primary, pressed: false, modifiers: Modifiers::NONE },
    ]
}

#[test]
fn ui_leaving_full_screen_on_another_screen_asks_first() {
    use crate::app::{Dialog, OutputChange};
    use eframe::egui::Key;
    let mut h = Harness::new();
    h.app.output_open = true;
    h.app.output_fullscreen = true;
    h.app.set_screens(Some(0), Some(1));
    h.frame(vec![]);
    assert!(h.app.output_on_other_screen());

    // F11 (from either window): only a question, the output stays full screen. (Without a
    // native backend the output window is drawn inside the main one and gets the key first:
    // it is handled on the next frame, as from the real output window.)
    h.press(Key::F11);
    h.frame(vec![]);
    assert!(matches!(h.app.dialog, Some(Dialog::LeaveOutput(OutputChange::Windowed))), "{:?}", h.app.dialog);
    assert!(h.app.output_fullscreen && h.app.output_open);
    h.frame(vec![]);
    assert!(h.texts.iter().any(|t| t == &crate::i18n::t("dialog.output_leave")), "{:?}", h.texts);
    // cancel: still full screen
    h.app.dialog = None;
    h.frame(vec![]);
    assert!(h.app.output_fullscreen);
    // F5 asks before closing
    h.press(Key::F5);
    h.frame(vec![]);
    assert!(matches!(h.app.dialog, Some(Dialog::LeaveOutput(OutputChange::Close))), "{:?}", h.app.dialog);
    assert!(h.app.output_open);
    h.app.dialog = None;
    // Esc / double click in the output window and its close button ask too
    for (fullscreen, close, change) in
        [(Some(false), false, OutputChange::Windowed), (Some(true), false, OutputChange::Windowed), (None, true, OutputChange::CloseButton)]
    {
        h.app.output_reports(fullscreen, close);
        h.frame(vec![]);
        assert_eq!(h.app.dialog.as_ref().map(|d| format!("{d:?}")), Some(format!("{:?}", Dialog::LeaveOutput(change))));
        assert!(h.app.output_open && h.app.output_fullscreen);
        h.app.dialog = None;
        h.frame(vec![]);
    }
    // confirmed: out of full screen
    h.press(Key::F11);
    h.frame(vec![]);
    let ctx = h.ctx.clone();
    h.app.dialog = None;
    h.app.apply_output_change(OutputChange::Windowed, &ctx);
    assert!(!h.app.output_fullscreen && h.app.output_open);
    // windowed: F11 goes full screen again without a question
    h.press(Key::F11);
    h.frame(vec![]);
    assert!(h.app.dialog.is_none());
    assert!(h.app.output_fullscreen);

    // one screen: no question
    h.app.set_screens(Some(0), None);
    h.press(Key::F11);
    h.frame(vec![]);
    assert!(h.app.dialog.is_none(), "{:?}", h.app.dialog);
    assert!(!h.app.output_fullscreen);
    h.press(Key::F11);
    h.frame(vec![]);
    assert!(h.app.output_fullscreen);
    h.press(Key::F5);
    h.frame(vec![]);
    assert!(h.app.dialog.is_none());
    assert!(!h.app.output_open);
    // the close button of a windowed output closes it right away
    h.press(Key::F5);
    h.frame(vec![]);
    h.app.set_screens(Some(0), Some(1));
    h.app.output_fullscreen = false;
    h.app.output_reports(None, true);
    h.frame(vec![]);
    assert!(h.app.dialog.is_none());
    assert!(!h.app.output_open);
}

#[test]
fn ui_pointer_shows_over_a_windowed_output() {
    let mut h = Harness::new();
    h.app.output_open = true;
    h.app.settings.output_always_on_top = true;
    h.app.output_fullscreen = false;
    h.frame(vec![]);
    assert!(!h.app.output_hides_cursor(), "a window shows the pointer");
    h.app.output_fullscreen = true;
    h.frame(vec![]);
    assert!(h.app.output_hides_cursor(), "full screen + always on top hides it");
    h.app.settings.output_always_on_top = false;
    h.frame(vec![]);
    assert!(!h.app.output_hides_cursor());
}

#[test]
fn ui_freeze_keeps_the_output() {
    use crate::app::ItemRef;
    use eframe::egui::Key;
    let mut h = Harness::new();
    h.app.service.items.push(ServiceItem::new(ItemKind::Text { title: "A".into(), text: "one\n---\ntwo\n---\nthree".into() }));
    h.frame(vec![]);
    h.press(Key::ArrowDown);
    assert_eq!(h.app.live_pos, Some((ItemRef::Service(0), 0)));
    let first = h.app.output_scene();
    // "," freezes: the slides go on, the output stays
    h.press(Key::Comma);
    assert!(h.app.frozen.is_some());
    h.frame(vec![]);
    assert!(h.texts.iter().any(|t| t == &crate::i18n::t("status.frozen")), "{:?}", h.texts);
    h.press(Key::ArrowDown);
    assert_eq!(h.app.live_pos, Some((ItemRef::Service(0), 1)));
    assert_eq!(h.app.output_scene(), first);
    assert_ne!(h.app.live, first);
    // blank still blanks a frozen output
    h.press(Key::Period);
    assert_eq!(h.app.output_scene(), Scene::Blank);
    h.press(Key::Period);
    assert_eq!(h.app.output_scene(), first);
    // "," again: the live slide is shown
    h.press(Key::Comma);
    assert!(h.app.frozen.is_none());
    assert_eq!(h.app.output_scene(), h.app.live);
    assert_ne!(h.app.output_scene(), first);
    // a frozen video keeps playing on the output; blanking pauses it; unfreezing sends the live one anew
    h.app.service.items.push(sample_video(100.0));
    h.app.select(ItemRef::Service(1), 0);
    let token = live_video(&h.app).token;
    h.app.toggle_freeze();
    h.app.select(ItemRef::Service(0), 0);
    assert!(matches!(h.app.output_scene(), Scene::Video(v) if v.token == token));
    h.app.toggle_blank();
    assert!(matches!(&h.app.frozen, Some((Scene::Video(v), _)) if v.paused && v.token != token));
    h.app.toggle_blank();
    h.app.select(ItemRef::Service(1), 0);
    let live_token = live_video(&h.app).token;
    h.app.toggle_freeze();
    assert!(matches!(h.app.output_scene(), Scene::Video(v) if v.token != token && v.token != live_token));
}

#[test]
fn ui_video_keys_on_the_service_tab() {
    use crate::app::ItemRef;
    use crate::app::ServiceArea as area;
    use eframe::egui::{Key, pos2};
    let mut h = Harness::new();
    h.app.service.items.push(ServiceItem::new(ItemKind::Text { title: "A".into(), text: "one\n---\ntwo".into() }));
    h.app.service.items.push(sample_video(100.0));
    h.app.service.items.push(ServiceItem::new(ItemKind::Text { title: "B".into(), text: "three".into() }));
    h.app.settings.live_follows_selection = false;
    h.frame(vec![]);
    // nothing clicked yet: Space is the next slide
    h.press(Key::Space);
    assert_eq!(h.app.sel_item, Some(ItemRef::Service(0)));
    h.app.sel_item = Some(ItemRef::Service(1));
    h.app.sel_slide = 0;
    // a click in the item list (left): the selected video takes Space and the arrows
    h.frame(click_at(pos2(100.0, 450.0)));
    h.frame(vec![]);
    assert_eq!(h.app.service_area, Some(area::List), "click in the item list");
    h.app.sel_item = Some(ItemRef::Service(1));
    h.app.sel_slide = 0;
    // → on a video that is not live does nothing (no jump to the next item)
    h.press(Key::ArrowRight);
    assert_eq!(h.app.sel_item, Some(ItemRef::Service(1)));
    assert!(h.app.live_pos.is_none());
    // Space: the video goes live and plays
    h.press(Key::Space);
    assert_eq!(h.app.live_pos, Some((ItemRef::Service(1), 0)));
    assert!(!live_video(&h.app).paused);
    // → / ←: 5 s forward / back, not before the start nor after the end
    h.press(Key::ArrowRight);
    let p = live_video(&h.app).position;
    assert!((5.0..5.5).contains(&p), "{p}");
    h.press(Key::ArrowLeft);
    h.press(Key::ArrowLeft);
    assert_eq!(live_video(&h.app).position, 0.0);
    for _ in 0..30 {
        h.press(Key::ArrowRight);
    }
    assert_eq!(live_video(&h.app).position, 100.0);
    h.press(Key::ArrowLeft);
    assert!((95.0..95.5).contains(&live_video(&h.app).position));
    // Space pauses, Space plays on
    h.press(Key::Space);
    assert!(live_video(&h.app).paused);
    h.press(Key::Space);
    assert!(!live_video(&h.app).paused);
    // the slides panel (middle) works the same
    h.frame(click_at(pos2(700.0, 600.0)));
    assert_eq!(h.app.service_area, Some(area::Slides));
    h.press(Key::Space);
    assert!(live_video(&h.app).paused);
    // the live panel (right): the live video, whatever is selected
    h.frame(click_at(pos2(1300.0, 700.0)));
    assert_eq!(h.app.service_area, Some(area::Live));
    h.app.sel_item = Some(ItemRef::Service(0));
    h.app.sel_slide = 0;
    h.press(Key::Space);
    assert!(!live_video(&h.app).paused);
    assert_eq!(h.app.sel_item, Some(ItemRef::Service(0)));
    // leaving the tab forgets the click: Space is the next slide again
    h.app.tab = crate::app::Tab::Songs;
    h.frame(vec![]);
    h.app.tab = crate::app::Tab::Service;
    assert_eq!(h.app.service_area, None);
    // a selected text item in the list: Space / → are slide keys again
    h.frame(click_at(pos2(100.0, 450.0)));
    h.app.sel_item = Some(ItemRef::Service(0));
    h.app.sel_slide = 0;
    h.press(Key::ArrowRight);
    assert_eq!(h.app.sel_slide, 1);
    h.press(Key::Space);
    assert_eq!(h.app.sel_item, Some(ItemRef::Service(1)));
    // the video keys are not slide-key conflicts in Settings
    let sc = &h.app.settings.shortcuts;
    let space = crate::shortcuts::KeyBinding::plain(Key::Space);
    assert!(sc.conflicts(&space, crate::shortcuts::Action::VideoPlayPause).is_empty());
    assert!(sc.conflicts(&space, crate::shortcuts::Action::NextSlide).is_empty());
}

#[test]
fn default_shortcuts_have_no_conflicts_and_freeze_is_comma() {
    use crate::shortcuts::{Action, KeyBinding, Shortcuts};
    let sc = Shortcuts::default();
    for a in Action::ALL {
        for b in sc.bindings(*a) {
            assert!(sc.conflicts(&b, *a).is_empty(), "{a:?} {b}: {:?}", sc.conflicts(&b, *a));
        }
    }
    assert_eq!(sc.bindings(Action::Freeze), vec![KeyBinding::plain(eframe::egui::Key::Comma)]);
    assert_eq!(
        KeyBinding::parse(&KeyBinding::plain(eframe::egui::Key::Comma).to_string()),
        Some(KeyBinding::plain(eframe::egui::Key::Comma))
    );
    // a video key on a key that is not a slide key is a real conflict
    let mut v = Shortcuts::default();
    v.map.insert(Action::VideoPlayPause, vec![KeyBinding::plain(eframe::egui::Key::Period)]);
    assert_eq!(v.conflicts(&KeyBinding::plain(eframe::egui::Key::Period), Action::VideoPlayPause), vec![Action::Blank]);
    // a real conflict is still one
    let mut sc = Shortcuts::default();
    sc.map.insert(Action::Freeze, vec![KeyBinding::plain(eframe::egui::Key::Period)]);
    assert_eq!(sc.conflicts(&KeyBinding::plain(eframe::egui::Key::Period), Action::Freeze), vec![Action::Blank]);
}

#[test]
fn ui_service_tab_keys_work_with_a_focused_button() {
    use crate::app::ItemRef;
    use eframe::egui::Key;
    let mut h = Harness::new();
    h.app.service.items.push(ServiceItem::new(ItemKind::Text { title: "A".into(), text: "one\n---\ntwo".into() }));
    h.app.sel_item = Some(ItemRef::Service(0));
    h.frame(vec![]);
    // Tab focuses a button of the Service tab
    h.press(Key::Tab);
    h.frame(vec![]);
    assert!(h.ctx.memory(|m| m.focused()).is_some(), "a widget has the focus");
    assert!(!h.ctx.text_edit_focused());
    // one-letter shortcuts still work
    h.press(Key::Period);
    assert!(h.app.blank, "Blank with a focused button");
    h.press(Key::Comma);
    assert!(h.app.frozen.is_some(), "Freeze with a focused button");
    h.press(Key::End);
    assert_eq!(h.app.sel_slide, 1, "End with a focused button");
    // ...but not in a text field: the text field gets the keys
    h.app.blank = false;
    h.app.frozen = None;
    let id = eframe::egui::Id::unique("plan_test_text");
    let mut text = String::new();
    let run = |h: &mut Harness, events: Vec<eframe::egui::Event>, text: &mut String| {
        let mut input = eframe::egui::RawInput { events, ..Default::default() };
        input.screen_rect = Some(eframe::egui::Rect::from_min_size(eframe::egui::Pos2::ZERO, eframe::egui::vec2(1400.0, 900.0)));
        input.focused = true;
        let app = &mut h.app;
        let _ = h.ctx.run_ui(input, |ui| {
            app.frame(ui);
            eframe::egui::Area::new(eframe::egui::Id::unique("plan_test_area")).show(ui.ctx(), |ui| {
                let r = ui.add(eframe::egui::TextEdit::singleline(text).id(id));
                r.request_focus();
            });
        });
    };
    run(&mut h, vec![], &mut text);
    run(&mut h, vec![], &mut text);
    assert!(h.ctx.text_edit_focused());
    let mut ev = key(Key::Period, eframe::egui::Modifiers::NONE);
    ev.push(eframe::egui::Event::Text(".".into()));
    run(&mut h, ev, &mut text);
    assert!(!h.app.blank, "typing a '.' is no Blank");
    assert_eq!(text, ".");
    // other tabs: a focused button still takes the plain keys (as before)
    h.app.tab = crate::app::Tab::Bible;
    h.ctx.memory_mut(|m| m.request_focus(eframe::egui::Id::unique("some_button")));
    assert_eq!(crate::shortcuts::KeyFocus::of(&h.ctx, false), crate::shortcuts::KeyFocus::Text);
    assert_eq!(crate::shortcuts::KeyFocus::of(&h.ctx, true), crate::shortcuts::KeyFocus::Widget);
}
