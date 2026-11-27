//! Tests of the web edition, run outside the browser (`cargo test`): the browser's side is
//! memory then (see `js::native`), the UI runs headless.

use crate::app::{ItemRef, Purpose, Tab, WebApp};
use crate::js::native;
use crate::model::*;
use crate::shortcuts::{Action, KeyBinding, Shortcuts};
use eframe::egui::{self, Event, Key, Modifiers};

struct Harness {
    ctx: egui::Context,
    app: WebApp,
    texts: Vec<String>,
    /// Height of the browser window.
    height: f32,
}

fn key_events(k: Key, modifiers: Modifiers) -> Vec<Event> {
    vec![
        Event::Key { key: k, physical_key: None, pressed: true, repeat: false, modifiers },
        Event::Key { key: k, physical_key: None, pressed: false, repeat: false, modifiers },
    ]
}

impl Harness {
    fn new() -> Self {
        let ctx = egui::Context::default();
        crate::render::install_fonts(&ctx);
        let app = WebApp::new(&ctx);
        let mut h = Self { ctx, app, texts: Vec::new(), height: 900.0 };
        h.frame(vec![]);
        h.frame(vec![]);
        h
    }

    fn frame(&mut self, events: Vec<Event>) {
        let mut input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1400.0, self.height))),
            events,
            ..Default::default()
        };
        eframe::App::raw_input_hook(&mut self.app, &self.ctx, &mut input);
        let app = &mut self.app;
        let mut out = self.ctx.run_ui(input, |ui| app.frame(ui));
        out.textures_delta.clear();
        self.texts.clear();
        fn collect(shape: &egui::Shape, out: &mut Vec<String>) {
            match shape {
                egui::Shape::Text(t) => out.push(t.galley.text().to_string()),
                egui::Shape::Vec(v) => v.iter().for_each(|s| collect(s, out)),
                _ => {}
            }
        }
        for s in &out.shapes {
            collect(&s.shape, &mut self.texts);
        }
    }

    fn press(&mut self, k: Key) {
        self.frame(key_events(k, Modifiers::NONE));
    }

    fn cmd(&mut self, name: &str, args: serde_json::Value) -> Result<String, String> {
        let ctx = self.ctx.clone();
        let out = self.app.run_command(name, args.as_object().unwrap(), &ctx);
        self.frame(vec![]);
        out
    }

    fn has(&self, text: &str) -> bool {
        self.texts.iter().any(|t| t.contains(text))
    }
}

fn sample_song() -> Song {
    let mut s = Song::new();
    s.title = "Test song".into();
    s.author = "Someone".into();
    s.parts = vec![
        SongPart { tag: "V".into(), number: 1, text: "verse one".into(), translations: Vec::new() },
        SongPart { tag: "C".into(), number: 0, text: "chorus".into(), translations: Vec::new() },
        SongPart { tag: "V".into(), number: 2, text: "verse two\n---\nverse two b".into(), translations: Vec::new() },
    ];
    s.order = "V1 C V2 C".into();
    s
}

const ZEFANIA: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<XMLBIBLE biblename="Test Bible">
  <INFORMATION><title>Test Bible</title><identifier>TB</identifier><language>en</language></INFORMATION>
  <BIBLEBOOK bnumber="1" bname="Genesis">
    <CHAPTER cnumber="1">
      <VERS vnumber="1">In the beginning God created the heaven and the earth.</VERS>
      <VERS vnumber="2">And the earth was without form, and void.</VERS>
      <VERS vnumber="3">And God said, Let there be light: and there was light.</VERS>
    </CHAPTER>
  </BIBLEBOOK>
  <BIBLEBOOK bnumber="43" bname="John">
    <CHAPTER cnumber="3">
      <VERS vnumber="16">For God so loved the world, that he gave his only begotten Son.</VERS>
    </CHAPTER>
  </BIBLEBOOK>
</XMLBIBLE>"#;

#[test]
fn languages_have_the_same_keys_and_cover_the_code() {
    let en = crate::i18n::parse(crate::i18n::source("en"));
    let cs = crate::i18n::parse(crate::i18n::source("cs"));
    let mut missing_cs: Vec<_> = en.keys().filter(|k| !cs.contains_key(*k)).collect();
    let mut missing_en: Vec<_> = cs.keys().filter(|k| !en.contains_key(*k)).collect();
    missing_cs.sort();
    missing_en.sort();
    assert!(missing_cs.is_empty(), "missing in cs.lang: {missing_cs:?}");
    assert!(missing_en.is_empty(), "missing in en.lang: {missing_en:?}");
    // every key the code asks for
    let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut missing = Vec::new();
    for f in std::fs::read_dir(dir).unwrap() {
        let src = std::fs::read_to_string(f.unwrap().path()).unwrap();
        for part in src.split("t(\"").skip(1).chain(src.split("tf(\"").skip(1)) {
            let key: String = part.chars().take_while(|c| *c != '"').collect();
            if key.contains('.') && !key.contains(' ') && !key.contains('{') && !en.contains_key(&key) {
                missing.push(key);
            }
        }
    }
    for a in Action::ALL {
        if !en.contains_key(&a.label_key()) {
            missing.push(a.label_key());
        }
    }
    for (code, _) in crate::app::MARKS {
        if !en.contains_key(&format!("mark.{code}")) {
            missing.push(format!("mark.{code}"));
        }
    }
    assert!(missing.is_empty(), "keys missing in en.lang: {missing:?}");
}

#[test]
fn shortcuts_avoid_the_browser_and_do_not_clash() {
    let s = Shortcuts::default();
    let mut seen = std::collections::HashMap::new();
    for a in Action::ALL {
        for b in s.bindings(*a) {
            assert!(!b.reserved(), "{a:?}: {b} belongs to the browser");
            if let Some(other) = seen.insert(b, *a) {
                panic!("{b} is used by {other:?} and {a:?}");
            }
            // the keys of song parts are not taken
            assert!(!crate::app::MARKS.iter().any(|(_, k)| b == KeyBinding::plain(*k)), "{a:?}: {b} is a song part key");
        }
    }
    // one key each, mostly single letters
    assert_eq!(s.bindings(Action::ToggleOutput), vec![KeyBinding::plain(Key::W)]);
    assert_eq!(s.bindings(Action::Find), vec![KeyBinding::plain(Key::S)]);
    assert_eq!(s.bindings(Action::AddToService), vec![KeyBinding::plain(Key::A)]);
    assert_eq!(KeyBinding::parse("Shift+N"), Some(KeyBinding::shift(Key::N)));
    assert_eq!(KeyBinding::from_event(Key::S, Modifiers::CTRL), None, "Ctrl+S is the browser's");
    let json = serde_json::to_string(&s).unwrap();
    let back: Shortcuts = serde_json::from_str(&json).unwrap();
    assert_eq!(back.bindings(Action::PrevItem), vec![KeyBinding::shift(Key::N)]);
}

#[test]
fn ui_presenting_with_keys_and_the_output_window() {
    let mut h = Harness::new();
    h.app.service.items.push(ServiceItem::new(ItemKind::Song(sample_song())));
    h.app.service.items.push(ServiceItem::new(ItemKind::Text { title: "News".into(), text: "one\n---\ntwo".into() }));
    h.frame(vec![]);
    // ↓ selects the first slide and (live mode) shows it
    h.press(Key::ArrowDown);
    assert_eq!((h.app.sel_item, h.app.sel_slide), (Some(ItemRef::Service(0)), 0));
    assert_eq!(h.app.live_pos, Some((ItemRef::Service(0), 0)));
    // slides: V1 C V2a V2b C — C jumps to the chorus, 2 to verse two
    h.press(Key::C);
    assert_eq!(h.app.sel_slide, 1);
    h.press(Key::Num2);
    assert_eq!(h.app.sel_slide, 2);
    h.press(Key::End);
    assert_eq!(h.app.sel_slide, 4);
    h.press(Key::ArrowDown);
    assert_eq!(h.app.sel_item, Some(ItemRef::Service(1)), "on to the next item");
    h.frame(key_events(Key::N, Modifiers::SHIFT));
    assert_eq!(h.app.sel_item, Some(ItemRef::Service(0)), "Shift+N: previous item");
    h.press(Key::N);
    assert_eq!(h.app.sel_item, Some(ItemRef::Service(1)));
    // blank, freeze
    h.press(Key::Period);
    assert!(h.app.blank && h.app.shown() == Scene::Blank);
    h.press(Key::Period);
    h.press(Key::Comma);
    let frozen = h.app.shown();
    h.press(Key::ArrowDown);
    assert_eq!(h.app.shown(), frozen, "frozen: the output keeps its picture");
    h.press(Key::Comma);
    assert_ne!(h.app.shown(), frozen);
    // live mode off: selecting only previews; Enter shows it
    h.press(Key::L);
    assert!(!h.app.settings.live_follows_selection);
    let live = h.app.live_pos;
    h.press(Key::ArrowUp);
    assert_eq!(h.app.live_pos, live);
    h.press(Key::Enter);
    assert_eq!(h.app.live_pos, Some((ItemRef::Service(1), 0)));
    // tabs: Shift+1 … Shift+4 from left to right; the Czech layout's 1 key is "+"
    for (k, tab) in [(Key::Num2, Tab::Songs), (Key::Num3, Tab::Bible), (Key::Num4, Tab::Settings), (Key::Num1, Tab::Service)] {
        h.frame(key_events(k, Modifiers::SHIFT));
        assert_eq!(h.app.tab, tab);
    }
    h.frame(vec![Event::Key { key: Key::Plus, physical_key: Some(Key::Num2), pressed: true, repeat: false, modifiers: Modifiers::SHIFT }]);
    assert_eq!(h.app.tab, Tab::Songs, "by the key's position");
    h.frame(key_events(Key::Num1, Modifiers::SHIFT));
    // the output window: opened with W, gets the live slide; its keys come back
    native::SENT.with(|s| s.borrow_mut().clear());
    h.press(Key::W);
    assert!(crate::js::output_open());
    h.frame(vec![]);
    let sent = native::SENT.with(|s| s.borrow().last().cloned()).expect("the live slide is sent");
    let msg: crate::output::OutputMsg = serde_json::from_str(&sent).unwrap();
    assert_eq!(msg.scene, h.app.shown());
    let n = native::SENT.with(|s| s.borrow().len());
    h.frame(vec![]);
    assert_eq!(native::SENT.with(|s| s.borrow().len()), n, "nothing new: nothing sent");
    native::INBOX.with(|i| i.borrow_mut().push(serde_json::to_string(&crate::output::FromOutput::Hello).unwrap()));
    h.frame(vec![]);
    assert_eq!(native::SENT.with(|s| s.borrow().len()), n + 1, "a new output window gets the slide");
    let before = h.app.sel_slide;
    native::INBOX.with(|i| i.borrow_mut().push(serde_json::to_string(&crate::output::FromOutput::Action(Action::NextSlide)).unwrap()));
    h.frame(vec![]);
    assert_eq!(h.app.sel_slide, before + 1, "a slide key pressed in the output window");
    h.press(Key::W);
    assert!(!crate::js::output_open());
    // keys never fire while typing
    h.app.tab = Tab::Songs;
    h.app.focus_song_search = true;
    h.frame(vec![]);
    h.frame(vec![Event::Text("w".into())]);
    h.press(Key::W);
    assert!(!crate::js::output_open(), "W typed into the search field");
}

#[test]
fn ui_add_to_service_with_a() {
    let mut h = Harness::new();
    // Songs: the open song
    h.app.tab = Tab::Songs;
    h.app.song = sample_song();
    h.frame(vec![]);
    h.press(Key::A);
    assert_eq!(h.app.service.items.len(), 1);
    // Bible: import, search, add
    h.app.open_file(Purpose::Bible, "test.xml", ZEFANIA.as_bytes());
    assert_eq!(h.app.bibles.len(), 1);
    assert_eq!(h.app.tab, Tab::Bible);
    h.app.bible.query = "Genesis 1:2-3".into();
    h.app.run_bible_search();
    assert_eq!(h.app.bible.results.len(), 2);
    h.frame(vec![]);
    h.press(Key::A);
    assert_eq!(h.app.service.items.len(), 2);
    assert!(matches!(&h.app.service.items[1].kind, ItemKind::Bible(p) if p.verses.len() == 2));
    // Enter in the Bible tab: the verses live, as a quick passage; A in the Service tab adds it
    h.press(Key::Enter);
    assert_eq!(h.app.sel_item, Some(ItemRef::Quick));
    h.app.tab = Tab::Service;
    h.frame(vec![]);
    assert!(h.has("⚡"));
    h.press(Key::A);
    assert_eq!((h.app.service.items.len(), h.app.quick.is_none()), (3, true));
    h.press(Key::A);
    assert_eq!(h.app.service.items.len(), 3, "no quick passage: nothing");
}

#[test]
fn bible_and_song_references_have_their_own_options() {
    let mut h = Harness::new();
    h.app.open_file(Purpose::Bible, "test.xml", ZEFANIA.as_bytes());
    h.app.bible.query = "John 3:16".into();
    h.app.run_bible_search();
    let p = h.app.bible_selected_passage().unwrap();
    h.app.service.items = vec![
        ServiceItem::new(ItemKind::Bible(p)),
        ServiceItem::new(ItemKind::Song(sample_song())),
        ServiceItem::new(ItemKind::Text { title: "News".into(), text: "one".into() }),
    ];
    let shown = |h: &Harness| -> Vec<(bool, String)> {
        h.app
            .service
            .items
            .iter()
            .map(|i| match &h.app.item_slides(i)[0].scene {
                Scene::Text(t) => (t.style.show_footer, t.footer.clone()),
                _ => panic!(),
            })
            .collect()
    };
    let s = shown(&h);
    assert_eq!(s.iter().map(|x| x.0).collect::<Vec<_>>(), vec![true, true, true]);
    assert_eq!(s[0].1, "John 3:16 (TB)");
    assert_eq!(s[1].1, "Test song — Someone");
    h.app.settings.show_bible_reference = false;
    assert_eq!(shown(&h).iter().map(|x| x.0).collect::<Vec<_>>(), vec![false, true, true]);
    h.app.settings.show_bible_reference = true;
    h.app.settings.text_style.show_footer = false;
    assert_eq!(shown(&h).iter().map(|x| x.0).collect::<Vec<_>>(), vec![true, false, false]);
    h.app.tab = Tab::Settings;
    h.frame(vec![]);
    assert!(h.has("Bible: show the reference") && h.has("Songs and texts: show the title"));
}

#[test]
fn ui_every_command_works() {
    use serde_json::json;
    let mut h = Harness::new();
    let mut song = sample_song();
    song.title = "Command song".into();
    h.app.songs.push(song);
    let mut done = std::collections::BTreeSet::new();
    let mut run = |h: &mut Harness, name: &str, args: serde_json::Value| {
        done.insert(name.to_string());
        h.cmd(name, args)
    };
    run(&mut h, "new-service", json!({})).unwrap();
    run(&mut h, "add-text", json!({"title": "Welcome", "text": "one\n---\ntwo"})).unwrap();
    run(&mut h, "add-song", json!({"title": "command"})).unwrap();
    assert!(run(&mut h, "add-song", json!({"title": "nothing like it"})).is_err());
    assert!(run(&mut h, "add-song", json!({})).is_err(), "a required parameter");
    assert!(run(&mut h, "add-song", json!({"title": 3})).is_err(), "a wrong type");
    assert!(run(&mut h, "list-items", json!({})).unwrap().contains("Command song"));
    run(&mut h, "select-item", json!({"item": "1"})).unwrap();
    assert!(run(&mut h, "list-slides", json!({})).unwrap().contains("one"));
    run(&mut h, "select-slide", json!({"slide": 2})).unwrap();
    assert_eq!(h.app.sel_slide, 1);
    assert!(run(&mut h, "select-slide", json!({"slide": 9})).is_err());
    run(&mut h, "first-slide", json!({})).unwrap();
    run(&mut h, "next-slide", json!({})).unwrap();
    run(&mut h, "prev-slide", json!({})).unwrap();
    run(&mut h, "last-slide", json!({})).unwrap();
    run(&mut h, "next-item", json!({})).unwrap();
    assert_eq!(h.app.sel_item, Some(ItemRef::Service(1)));
    run(&mut h, "prev-item", json!({})).unwrap();
    run(&mut h, "live-mode", json!({"state": "off"})).unwrap();
    run(&mut h, "go-live", json!({})).unwrap();
    run(&mut h, "rename-item", json!({"name": "Hello"})).unwrap();
    assert_eq!(h.app.service.items[0].title(), "Hello");
    run(&mut h, "move-down", json!({})).unwrap();
    run(&mut h, "move-up", json!({})).unwrap();
    assert_eq!(h.app.service.items[0].title(), "Hello");
    run(&mut h, "blank", json!({"state": "on"})).unwrap();
    assert!(h.app.blank);
    run(&mut h, "freeze", json!({})).unwrap();
    assert!(h.app.frozen.is_some());
    run(&mut h, "freeze", json!({"state": "off"})).unwrap();
    run(&mut h, "output", json!({"state": "open"})).unwrap();
    assert!(crate::js::output_open());
    run(&mut h, "output", json!({"state": "close"})).unwrap();
    run(&mut h, "alert", json!({"text": "Car 1AB"})).unwrap();
    assert!(run(&mut h, "status", json!({})).unwrap().contains("Car 1AB"));
    run(&mut h, "stop-alert", json!({})).unwrap();
    run(&mut h, "show-tab", json!({"tab": "songs"})).unwrap();
    assert_eq!(h.app.tab, Tab::Songs);
    assert!(run(&mut h, "list-songs", json!({"search": "comm"})).unwrap().contains("Command song"));
    assert!(run(&mut h, "list-bibles", json!({})).unwrap().contains("no bibles"));
    h.app.open_file(Purpose::Bible, "test.xml", ZEFANIA.as_bytes());
    assert!(run(&mut h, "list-bibles", json!({})).unwrap().contains("Test Bible"));
    assert!(run(&mut h, "bible", json!({"action": "search", "query": "light"})).unwrap().contains("Let there be light"));
    run(&mut h, "bible", json!({"action": "show", "query": "Genesis 1:1"})).unwrap();
    assert_eq!(h.app.sel_item, Some(ItemRef::Quick));
    let n = h.app.service.items.len();
    run(&mut h, "bible", json!({"action": "add", "query": "John 3:16"})).unwrap();
    run(&mut h, "add-to-service", json!({"tab": "service"})).unwrap();
    assert_eq!(h.app.service.items.len(), n + 2);
    assert!(run(&mut h, "remove-item", json!({})).is_err(), "the quick passage was selected");
    run(&mut h, "select-item", json!({"item": "Hello"})).unwrap();
    run(&mut h, "remove-item", json!({})).unwrap();
    assert_eq!(h.app.service.items.len(), n + 1);
    assert!(run(&mut h, "get-settings", json!({})).unwrap().contains("verses_per_slide"));
    run(&mut h, "set-setting", json!({"key": "verses_per_slide", "value": "2"})).unwrap();
    assert_eq!(h.app.settings.verses_per_slide, 2);
    run(&mut h, "set-setting", json!({"key": "show_bible_reference", "value": "false"})).unwrap();
    assert!(!h.app.settings.show_bible_reference);
    run(&mut h, "set-setting", json!({"key": "language", "value": "\"cs\""})).unwrap();
    assert_eq!(crate::i18n::current_language(), "cs");
    run(&mut h, "set-setting", json!({"key": "language", "value": "\"en\""})).unwrap();
    assert!(run(&mut h, "set-setting", json!({"key": "nonsense", "value": "1"})).is_err());
    assert!(run(&mut h, "fly", json!({})).is_err());
    for c in crate::commands::COMMANDS {
        assert!(done.contains(c.name), "{} untested", c.name);
    }
    // the tools as WebMCP gets them; a call through the queue is answered
    let tools: Vec<serde_json::Value> = serde_json::from_str(&crate::commands::tools_json()).unwrap();
    assert_eq!(tools.len(), crate::commands::COMMANDS.len());
    assert!(tools.iter().all(|t| t["inputSchema"]["type"] == "object"));
    assert_eq!(h.app.tools, crate::commands::COMMANDS.len() as u32);
    native::COMMANDS.with(|c| c.borrow_mut().push(r#"{"id": 7, "name": "status", "args": {}}"#.into()));
    native::COMMANDS.with(|c| c.borrow_mut().push(r#"{"id": 8, "name": "select-slide", "args": {"slide": "x"}}"#.into()));
    h.frame(vec![]);
    let answers = native::ANSWERS.with(|a| a.borrow_mut().drain(..).collect::<Vec<_>>());
    assert_eq!(answers.len(), 2);
    assert!(answers[0].0 == 7.0 && answers[0].1 && answers[0].2.contains("live mode"));
    assert!(answers[1].0 == 8.0 && !answers[1].1);
}

#[test]
fn data_is_kept_in_the_browser() {
    let mut h = Harness::new();
    h.app.song = sample_song();
    h.app.save_song();
    h.app.add_to_service(ItemKind::Text { title: "Kept".into(), text: "x".into() });
    h.app.settings.verses_per_slide = 4;
    h.app.open_file(Purpose::Bible, "test.xml", ZEFANIA.as_bytes());
    h.frame(vec![]);
    // a new page (reload): everything is back, the bible from IndexedDB
    let h2 = Harness::new();
    assert_eq!(h2.app.songs.len(), 1);
    assert_eq!(h2.app.service.items.last().map(|i| i.title()), Some("Kept".into()));
    assert_eq!(h2.app.settings.verses_per_slide, 4);
    assert_eq!(h2.app.bible.current.as_ref().map(|b| b.meta.name.as_str()), Some("Test Bible"));
    // the first web version's slides become text items once
    crate::js::local_set(crate::app::KEY_SERVICE, "{}").unwrap();
    crate::js::local_set(crate::app::KEY_V1, r#"{"slides":[{"title":"Old","text":"old text"}]}"#).unwrap();
    let h3 = Harness::new();
    assert_eq!(h3.app.service.items.len(), 1);
    assert_eq!(h3.app.service.items[0].title(), "Old");
    let h4 = Harness::new();
    assert_eq!(h4.app.service.items.len(), 1, "not imported again");
}

#[test]
fn services_open_and_save_as_ohsvc_with_pictures() {
    let mut h = Harness::new();
    // a picture added to the service
    let mut png = Vec::new();
    let img = image::RgbaImage::from_pixel(4, 2, image::Rgba([10, 200, 30, 255]));
    image::DynamicImage::ImageRgba8(img).write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png).unwrap();
    h.app.open_file(Purpose::Image, "photo.png", &png);
    h.app.add_to_service(ItemKind::Song(sample_song()));
    assert_eq!(h.app.service.items.len(), 2);
    h.app.service_name = "sunday".into();
    native::SAVED_FILES.with(|f| f.borrow_mut().clear());
    h.app.save_service_file();
    let (name, bytes) = native::SAVED_FILES.with(|f| f.borrow().last().cloned()).unwrap();
    assert_eq!(name, "sunday.ohsvc");
    let (service, media) = crate::service_file::read_service(&bytes).unwrap();
    assert_eq!(service, h.app.service);
    assert_eq!(media.len(), 1, "the picture goes with the service");
    // open it in a new page
    let mut h2 = Harness::new();
    h2.app.open_file(Purpose::Service, "sunday.ohsvc", &bytes);
    assert_eq!(h2.app.service, service);
    assert!(h2.app.tex.media.contains_key(&media[0].0));
    // the picture slide draws
    h2.app.select(ItemRef::Service(0), 0);
    h2.frame(vec![]);
    assert!(matches!(h2.app.live, Scene::Canvas(_)));
    // not a service
    h2.app.open_file(Purpose::Service, "x.ohsvc", b"nope");
    assert!(matches!(h2.app.dialog, Some(crate::app::Dialog::Message(_))));
}

#[test]
fn songs_import_like_the_program() {
    let s = crate::service_file::import_song("amazing.txt", "Verse one line\n\nR: Chorus line\n\n2. Second verse".as_bytes()).unwrap();
    assert_eq!(s.title, "amazing");
    assert_eq!(s.parts.iter().map(|p| p.code()).collect::<Vec<_>>(), vec!["V1", "C", "V2"]);
    let xml = crate::song_formats::song_to_openlyrics(&sample_song());
    let back = crate::service_file::import_song("x.xml", xml.as_bytes()).unwrap();
    assert_eq!(back.title, "Test song");
    assert_eq!(back.parts.len(), 3);
    assert!(crate::service_file::import_song("x.txt", b"  ").is_err());
    assert!(crate::service_file::import_bible("x.sqlite", b"").is_err());
}

#[test]
fn ui_every_tab_draws_in_both_languages() {
    let mut h = Harness::new();
    h.app.service.items.push(ServiceItem::new(ItemKind::Song(sample_song())));
    h.app.select(ItemRef::Service(0), 0);
    for lang in ["en", "cs"] {
        crate::i18n::set_language(lang);
        for (tab, key, _) in crate::app::TABS {
            h.app.tab = tab;
            h.frame(vec![]);
            h.frame(vec![]);
            assert!(h.has(&crate::i18n::t(key)), "{lang} {tab:?}");
        }
    }
    crate::i18n::set_language("en");
    h.app.tab = Tab::Service;
    h.frame(vec![]);
    // the tabs from left to right, as their keys Shift+1 … Shift+4
    let at = |name: &str| h.texts.iter().position(|t| t == name).unwrap_or_else(|| panic!("{name}: {:?}", h.texts));
    assert!(at("Service") < at("Songs") && at("Songs") < at("Bible") && at("Bible") < at("Settings"));
    // every action is listed in Settings › Keyboard shortcuts (all of the page in view)
    h.app.tab = Tab::Settings;
    h.height = 5000.0;
    h.frame(vec![]);
    h.frame(vec![]);
    for a in Action::ALL {
        assert!(h.has(&crate::i18n::t(&a.label_key())), "{a:?}");
    }
    // a key can be bound, the browser's cannot
    h.app.capture = Some(Action::Blank);
    h.frame(key_events(Key::K, Modifiers::NONE));
    assert!(h.app.settings.shortcuts.bindings(Action::Blank).contains(&KeyBinding::plain(Key::K)));
    h.app.capture = Some(Action::Blank);
    h.frame(key_events(Key::F5, Modifiers::NONE));
    assert!(!h.app.settings.shortcuts.bindings(Action::Blank).contains(&KeyBinding::plain(Key::F5)));
}

#[test]
fn the_output_window_shows_what_the_main_window_sends() {
    use crate::output::{FromOutput, OutputApp, OutputMsg};
    let ctx = egui::Context::default();
    crate::render::install_fonts(&ctx);
    native::SENT.with(|s| s.borrow_mut().clear());
    let mut out = OutputApp::new(&ctx);
    // it says hello to the main window
    let hello = native::SENT.with(|s| s.borrow_mut().drain(..).collect::<Vec<_>>());
    assert_eq!(hello, vec![serde_json::to_string(&FromOutput::Hello).unwrap()]);
    let frame = |out: &mut OutputApp, events: Vec<Event>| -> Vec<String> {
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(960.0, 540.0))),
            events,
            ..Default::default()
        };
        let mut o = ctx.run_ui(input, |ui| out.frame(ui));
        o.textures_delta.clear();
        let mut texts = Vec::new();
        fn collect(shape: &egui::Shape, out: &mut Vec<String>) {
            match shape {
                egui::Shape::Text(t) => out.push(t.galley.text().to_string()),
                egui::Shape::Vec(v) => v.iter().for_each(|s| collect(s, out)),
                _ => {}
            }
        }
        for s in &o.shapes {
            collect(&s.shape, &mut texts);
        }
        texts
    };
    assert!(frame(&mut out, vec![]).iter().any(|t| t.contains("Waiting")));
    let scene = Scene::Text(TextScene {
        text: "Amazing grace".into(),
        extra: vec![],
        layout: ColumnLayout::default(),
        footer: "Hymn".into(),
        style: TextStyle::default(),
        background: None,
    });
    let alert = Alert { text: "Car 1AB".into(), style: AlertStyle::default(), duration: 0.0 };
    let msg = OutputMsg {
        scene,
        alert: Some((alert, 0.5)),
        media: vec![],
        fullscreen_keys: vec![KeyBinding::plain(Key::F)],
        language: "en".into(),
    };
    native::INBOX.with(|i| i.borrow_mut().push(serde_json::to_string(&msg).unwrap()));
    let texts = frame(&mut out, vec![]);
    assert!(texts.iter().any(|t| t == "Amazing grace") && texts.iter().any(|t| t == "Hymn"), "{texts:?}");
    assert!(texts.iter().any(|t| t.contains("Car 1AB")), "the alert runs");
    // slide keys go to the main window
    frame(&mut out, key_events(Key::ArrowDown, Modifiers::NONE));
    let sent = native::SENT.with(|s| s.borrow_mut().drain(..).collect::<Vec<_>>());
    assert_eq!(sent, vec![serde_json::to_string(&FromOutput::Action(Action::NextSlide)).unwrap()]);
}
