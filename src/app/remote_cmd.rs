//! The commands of the command line (`--cli`) and the MCP server (see [`crate::remote`]),
//! carried out as the buttons of the program do it.

use super::{App, Dialog, ItemRef, OutputChange, Tab};
use crate::model::*;
use crate::remote;
use eframe::egui;
use serde_json::{Map, Value};
use std::path::PathBuf;

type Args = Map<String, Value>;

fn text<'a>(args: &'a Args, name: &str) -> &'a str {
    args.get(name).and_then(Value::as_str).unwrap_or_default()
}

fn flag(args: &Args, name: &str) -> bool {
    args.get(name).and_then(Value::as_bool).unwrap_or(false)
}

/// `on` / `off`, or `None` (toggle).
fn state(args: &Args) -> Option<bool> {
    args.get("state").and_then(Value::as_str).map(|s| s == "on")
}

fn kind_name(k: &ItemKind) -> &'static str {
    match k {
        ItemKind::Song(_) => "song",
        ItemKind::Design(_) => "design",
        ItemKind::Bible(_) => "bible",
        ItemKind::Text { .. } => "text",
        ItemKind::Video(_) => "video",
    }
}

impl App {
    /// The error a command left in a message dialog (taken out of the dialog: the caller gets it).
    fn take_error(&mut self) -> Option<String> {
        match self.dialog.take() {
            Some(Dialog::Message(m)) => Some(m),
            other => {
                self.dialog = other;
                None
            }
        }
    }

    fn selected_index(&self) -> Result<usize, String> {
        match self.sel_item {
            Some(ItemRef::Service(i)) if i < self.service.items.len() => Ok(i),
            _ => Err("no service item is selected (select-item first)".into()),
        }
    }

    fn unsaved_check(&self, args: &Args) -> Result<(), String> {
        if self.service_dirty && !flag(args, "discard") {
            return Err("the service has unsaved changes: save-service first, or add discard=true".into());
        }
        Ok(())
    }

    /// Finds a service item by number (from 1), title or video file name (case and accents
    /// ignored; a title starting with it counts when nothing matches exactly).
    pub fn find_item(&self, what: &str) -> Option<usize> {
        let what = what.trim();
        if let Ok(n) = what.parse::<usize>() {
            return (n >= 1 && n <= self.service.items.len()).then(|| n - 1);
        }
        let q = crate::storage::fold(what);
        let names = |it: &ServiceItem| {
            let mut v = vec![crate::storage::fold(&it.title()), crate::storage::fold(&it.own_title())];
            if let ItemKind::Video(video) = &it.kind {
                v.push(crate::storage::fold(&video.media));
                v.push(crate::storage::fold(&format!("{}.", video.title)));
            }
            v
        };
        let items = &self.service.items;
        items
            .iter()
            .position(|it| names(it).contains(&q))
            .or_else(|| {
                items.iter().position(|it| {
                    names(it).iter().any(|n| n.starts_with(&q) || (q.contains('.') && q.starts_with(n.as_str()) && !n.is_empty()))
                })
            })
            .or_else(|| {
                // a file name: "video.mp4" for the item titled "video"
                let stem = std::path::Path::new(what).file_stem().map(|s| crate::storage::fold(&s.to_string_lossy()))?;
                items.iter().position(|it| names(it).contains(&stem))
            })
    }

    /// What is live, selected, the output (for `status` and the CLI).
    pub fn status_text(&self) -> String {
        let item = |r: ItemRef| match r {
            ItemRef::Service(i) => self.service.items.get(i).map(|it| format!("{} {}", i + 1, it.title())).unwrap_or_default(),
            ItemRef::Quick => self.quick.as_ref().map(|q| format!("quick {}", q.title())).unwrap_or_default(),
        };
        let live = match self.live_pos {
            Some((r, s)) => format!("{} / slide {}", item(r), s + 1),
            None => "nothing".into(),
        };
        let video = match &self.live {
            Scene::Video(v) => {
                let elapsed = self.video_since.elapsed().as_secs_f64();
                let st = if v.ended_after(elapsed) {
                    "ended"
                } else if v.paused {
                    "paused"
                } else {
                    "playing"
                };
                format!(
                    "\nvideo: {st} at {} / {}{}{}",
                    crate::video::format_time(v.position_after(elapsed)),
                    crate::video::format_time(v.duration),
                    if v.looped { ", loop" } else { "" },
                    if v.muted { ", without sound" } else { "" }
                )
            }
            _ => String::new(),
        };
        let selected = match self.sel_item {
            Some(r) => format!("{} / slide {}", item(r), self.sel_slide + 1),
            None => "nothing".into(),
        };
        let output = match (self.output_open, self.output_fullscreen) {
            (false, _) => "closed",
            (true, true) => "full screen",
            (true, false) => "window",
        };
        let file = self.service_path.as_ref().map(|p| p.to_string_lossy().to_string()).unwrap_or_else(|| "(not saved yet)".into());
        format!(
            "service: {file}{} ({} items)\nselected: {selected}\nlive: {live}{video}\nlive mode: {}\nblank: {}\nfrozen: {}\noutput window: {output}\ntab: {:?}",
            if self.service_dirty { " *unsaved*" } else { "" },
            self.service.items.len(),
            if self.settings.live_follows_selection { "on" } else { "off" },
            if self.blank { "on" } else { "off" },
            if self.frozen.is_some() { "on" } else { "off" },
            self.tab,
        )
    }

    /// Runs one command (from the command line or the MCP server), as the buttons do.
    pub fn run_command(&mut self, name: &str, args: &Args, ctx: &egui::Context) -> Result<String, String> {
        if name == "quit" {
            self.quit_confirmed = true;
            ctx.request_repaint();
            return Ok("quitting".into());
        }
        let spec = remote::spec(name).ok_or_else(|| format!("unknown command {name:?}"))?;
        remote::check_args(spec, args)?;
        // a question waiting for the operator (rename, unsaved changes, …) is answered first:
        // a command changing things under it would make it act on something else
        let read_only = name.starts_with("list-") || matches!(name, "status" | "get-settings");
        if self.dialog.is_some() && !read_only {
            return Err("a dialog is open in the program window: it has to be answered there first (status shows the rest)".into());
        }
        let had_dialog = self.dialog.is_some();
        let out = self.command(name, args, ctx);
        // an error message the command left (not one the operator had open already)
        if !had_dialog && let Some(e) = self.take_error() {
            return Err(e);
        }
        ctx.request_repaint();
        out
    }

    fn command(&mut self, name: &str, args: &Args, ctx: &egui::Context) -> Result<String, String> {
        match name {
            "new-service" => {
                self.unsaved_check(args)?;
                self.new_service();
                Ok("new service".into())
            }
            "open-service" => {
                self.unsaved_check(args)?;
                let path = PathBuf::from(text(args, "file"));
                if !path.is_file() {
                    return Err(format!("no such file: {}", path.display()));
                }
                self.open_service(path);
                Ok(format!("opened, {} items", self.service.items.len()))
            }
            "save-service" => {
                let Some(path) = self.service_path.clone() else { return Err("a new service: use save-service-as <file>".into()) };
                self.save_service();
                if let Some(Dialog::ChangedOnDisk { .. }) = self.dialog {
                    self.dialog = None;
                    return Err(format!("{} was changed by someone else since it was opened: use save-service-as", path.display()));
                }
                Ok(format!("saved to {}", path.display()))
            }
            "save-service-as" => {
                let path = crate::storage::with_extension(PathBuf::from(text(args, "file")), crate::formats::SERVICE_EXT);
                if path.exists() && self.service_path.as_ref() != Some(&path) {
                    return Err(format!("{} exists already", path.display()));
                }
                self.write(super::WriteKind::Service, path.clone());
                Ok(format!("saved to {}", path.display()))
            }
            "add-song" => {
                let q = crate::storage::fold(text(args, "title"));
                let song = self
                    .lib
                    .songs
                    .iter()
                    .find(|s| crate::storage::fold(&s.title) == q)
                    .or_else(|| self.lib.songs.iter().find(|s| crate::storage::fold(&s.title).starts_with(&q)))
                    .cloned()
                    .ok_or_else(|| format!("no song {:?} in the library (list-songs)", text(args, "title")))?;
                let title = song.title.clone();
                self.add_to_service(ItemKind::Song(song));
                Ok(format!("added song {title}"))
            }
            "add-design" => {
                let q = crate::storage::fold(text(args, "name"));
                let d = self
                    .lib
                    .designs
                    .iter()
                    .find(|d| crate::storage::fold(&d.name) == q)
                    .cloned()
                    .ok_or_else(|| format!("no design {:?} in the library (list-designs)", text(args, "name")))?;
                let n = d.name.clone();
                self.add_to_service(ItemKind::Design(d));
                Ok(format!("added design {n}"))
            }
            "add-text" => {
                self.add_to_service(ItemKind::Text {
                    title: text(args, "title").to_string(),
                    text: text(args, "text").replace("\\n", "\n"),
                });
                Ok("added text".into())
            }
            "add-image" => {
                let f = PathBuf::from(text(args, "file"));
                let n = self.service.items.len();
                self.import_image_files(vec![f]);
                if self.service.items.len() == n {
                    return Err(self.take_error().unwrap_or_else(|| "the picture could not be added".into()));
                }
                Ok("added picture".into())
            }
            "add-to-service" => {
                if args.contains_key("tab") {
                    self.tab = tab_named(text(args, "tab"));
                }
                let title = self.add_selected_to_service()?;
                Ok(format!("added {title}"))
            }
            "add-video" => {
                let f = PathBuf::from(text(args, "file"));
                if !f.is_file() {
                    return Err(format!("no such file: {}", f.display()));
                }
                if !crate::video::video_available() {
                    return Err(crate::i18n::t("video.no_ffmpeg"));
                }
                self.import_video_files(vec![f]);
                Ok("adding the video (it appears in list-items when copied)".into())
            }
            "list-items" => {
                let mut out = String::new();
                for (i, it) in self.service.items.iter().enumerate() {
                    let sel = if self.sel_item == Some(ItemRef::Service(i)) { "*" } else { " " };
                    let live = if matches!(self.live_pos, Some((ItemRef::Service(l), _)) if l == i) { " LIVE" } else { "" };
                    out.push_str(&format!("{sel}{:>3}  {}  ({}){live}\n", i + 1, it.title(), kind_name(&it.kind)));
                }
                if out.is_empty() {
                    out = "the service is empty".into();
                }
                Ok(out.trim_end().to_string())
            }
            "select-item" => {
                let i = self.find_item(text(args, "item")).ok_or_else(|| format!("no item {:?} (list-items)", text(args, "item")))?;
                self.tab = Tab::Service;
                self.select(ItemRef::Service(i), 0);
                Ok(format!("selected {} {}", i + 1, self.service.items[i].title()))
            }
            "rename-item" => {
                let i = self.selected_index()?;
                self.rename_item(i, text(args, "name"));
                Ok(format!("renamed to {}", self.service.items[i].title()))
            }
            "move-up" | "move-down" | "duplicate-item" | "remove-item" => {
                let i = self.selected_index()?;
                let act = match name {
                    "move-up" => "up",
                    "move-down" => "down",
                    "duplicate-item" => "dup",
                    _ => "remove",
                };
                self.item_action(i, act);
                Ok("done".into())
            }
            "list-slides" => {
                let r = self.sel_item.ok_or("no item is selected")?;
                let slides = self.slides_of(r);
                let mut out = String::new();
                for (n, s) in slides.iter().enumerate() {
                    let sel = if n == self.sel_slide { "*" } else { " " };
                    let first = match &s.scene {
                        Scene::Text(t) => t.text.lines().next().unwrap_or_default().to_string(),
                        _ => String::new(),
                    };
                    out.push_str(&format!("{sel}{:>3}  {}  {first}\n", n + 1, self.slide_label(s)));
                }
                Ok(out.trim_end().to_string())
            }
            "select-slide" => {
                let r = self.sel_item.ok_or("no item is selected")?;
                let n = args.get("slide").and_then(Value::as_i64).unwrap_or(0);
                let count = self.slides_of(r).len() as i64;
                if n < 1 || n > count {
                    return Err(format!("slide {n}: the item has {count} slides"));
                }
                self.select(r, n as usize - 1);
                Ok(format!("slide {n}"))
            }
            "next-slide" | "prev-slide" | "first-slide" | "last-slide" | "next-item" | "prev-item" | "go-live" => {
                let a = match name {
                    "next-slide" => crate::shortcuts::Action::NextSlide,
                    "prev-slide" => crate::shortcuts::Action::PrevSlide,
                    "first-slide" => crate::shortcuts::Action::FirstSlide,
                    "last-slide" => crate::shortcuts::Action::LastSlide,
                    "next-item" => crate::shortcuts::Action::NextItem,
                    "prev-item" => crate::shortcuts::Action::PrevItem,
                    _ => crate::shortcuts::Action::GoLive,
                };
                self.handle_actions(vec![a], ctx);
                Ok(self.status_text())
            }
            "live-mode" => {
                self.set_live_mode(state(args) == Some(true));
                Ok(format!("live mode {}", if self.settings.live_follows_selection { "on" } else { "off" }))
            }
            "blank" => {
                if state(args).is_none_or(|on| on != self.blank) {
                    self.toggle_blank();
                }
                Ok(format!("blank {}", if self.blank { "on" } else { "off" }))
            }
            "freeze" => {
                if state(args).is_none_or(|on| on != self.frozen.is_some()) {
                    self.toggle_freeze();
                }
                Ok(format!("freeze {}", if self.frozen.is_some() { "on" } else { "off" }))
            }
            "playback-control" => self.playback_control(text(args, "action"), args.get("seconds").and_then(Value::as_f64)),
            "output" => {
                match text(args, "state") {
                    "open" => self.output_open = true,
                    "close" => {
                        self.output_open = false;
                    }
                    "fullscreen" => {
                        self.output_open = true;
                        if !self.output_fullscreen {
                            self.output_fullscreen = true;
                            self.send_output_fullscreen(ctx);
                        }
                    }
                    _ => {
                        if self.output_fullscreen {
                            self.apply_output_change(OutputChange::Windowed, ctx);
                        }
                        self.output_open = true;
                    }
                }
                Ok(self.status_text())
            }
            "alert" => {
                self.send_alert(text(args, "text"));
                Ok("alert sent".into())
            }
            "stop-alert" => {
                self.stop_alert();
                Ok("alert stopped".into())
            }
            "show-tab" => {
                self.tab = tab_named(text(args, "tab"));
                Ok(format!("{:?}", self.tab))
            }
            "list-songs" => {
                let q = crate::storage::fold(text(args, "search"));
                let list: Vec<&str> = self
                    .lib
                    .songs
                    .iter()
                    .filter(|s| q.is_empty() || crate::storage::fold(&s.title).contains(&q))
                    .map(|s| s.title.as_str())
                    .collect();
                Ok(if list.is_empty() { "no songs".into() } else { list.join("\n") })
            }
            "list-designs" => {
                let list: Vec<&str> = self.lib.designs.iter().map(|d| d.name.as_str()).collect();
                Ok(if list.is_empty() { "no designs".into() } else { list.join("\n") })
            }
            "list-bibles" => {
                let cur = self.bible.current.as_ref().map(|b| b.meta.id.clone());
                let list: Vec<String> = self
                    .bible
                    .bibles
                    .iter()
                    .map(|b| format!("{}{}  {} ({})", if Some(&b.id) == cur.as_ref() { "*" } else { " " }, b.id, b.name, b.abbrev))
                    .collect();
                Ok(if list.is_empty() { "no bibles".into() } else { list.join("\n") })
            }
            "bible" => self.bible_command(args),
            "list-bible-books" => self.list_bible_books(args),
            "bible-item" => self.bible_item_command(args),
            "set-background" => self.set_background_command(args),
            "status" => Ok(self.status_text()),
            "get-settings" => serde_json::to_string_pretty(&self.settings).map_err(|e| e.to_string()),
            "set-setting" => self.set_setting(text(args, "key"), text(args, "value"), ctx),
            other => Err(format!("unknown command {other:?}")),
        }
    }

    /// `playback-control`: on the live video, else the selected video goes live first.
    pub fn playback_control(&mut self, action: &str, seconds: Option<f64>) -> Result<String, String> {
        let selected_video = self.sel_item.filter(|r| self.item(*r).is_some_and(|it| matches!(it.kind, ItemKind::Video(_))));
        let live_video = matches!(self.live, Scene::Video(_));
        let selected_is_live = selected_video.is_some() && live_video && self.live_pos.map(|(r, _)| r) == selected_video;
        if selected_video.is_some() && !selected_is_live {
            // the selected video goes live first (as Space on it does)
            self.sel_slide = 0;
            self.go_live();
            // a video goes live paused at its start: the action below plays it
            self.video_update(|v| v.paused = true);
        } else if !live_video {
            return Err("no video is live or selected".into());
        }
        let elapsed = self.video_since.elapsed().as_secs_f64();
        let playing = matches!(&self.live, Scene::Video(v) if !v.paused && !v.ended_after(elapsed));
        let ended = matches!(&self.live, Scene::Video(v) if v.ended_after(elapsed));
        if self.blank && matches!(action, "play" | "play-pause" | "play-loop" | "play-once" | "restart") {
            self.toggle_blank();
        }
        match action {
            "play" | "play-loop" | "play-once" => self.video_update(|v| {
                if ended {
                    v.position = 0.0;
                }
                if action == "play-loop" {
                    v.looped = true;
                } else if action == "play-once" {
                    v.looped = false;
                }
                v.paused = false;
            }),
            "pause" => self.video_update(|v| v.paused = true),
            "play-pause" => {
                if playing {
                    self.video_update(|v| v.paused = true)
                } else {
                    self.video_play_pause(super::video_ctl::VideoTarget::Live)
                }
            }
            "restart" => self.video_update(|v| v.position = 0.0),
            "seek" => {
                let s = seconds.ok_or("seek needs seconds=")?;
                self.video_update(|v| {
                    let end = if v.duration > 0.0 { v.duration } else { f64::INFINITY };
                    v.position = s.clamp(0.0, end);
                })
            }
            "forward" => self.video_seek(super::video_ctl::VIDEO_SEEK_STEP),
            "back" => self.video_seek(-super::video_ctl::VIDEO_SEEK_STEP),
            "mute" | "unmute" => self.video_update(|v| v.muted = action == "mute"),
            other => return Err(format!("unknown playback control {other:?}")),
        }
        // the item's switches follow, as with its checkboxes
        if let (Some((ItemRef::Service(i), _)), Scene::Video(lv)) = (self.live_pos, &self.live)
            && let Some(ItemKind::Video(v)) = self.service.items.get_mut(i).map(|it| &mut it.kind)
            && v.media == lv.media
            && (v.looped != lv.looped || v.muted != lv.muted)
        {
            v.looped = lv.looped;
            v.muted = lv.muted;
            self.service_dirty = true;
        }
        Ok(self.status_text())
    }

    /// A bible by id, name or abbreviation (case and accents ignored).
    fn find_bible(&self, want: &str) -> Result<String, String> {
        let f = crate::storage::fold(want);
        self.bible
            .bibles
            .iter()
            .find(|b| crate::storage::fold(&b.id) == f || crate::storage::fold(&b.name) == f || crate::storage::fold(&b.abbrev) == f)
            .map(|b| b.id.clone())
            .ok_or_else(|| format!("no bible {want:?} (list-bibles)"))
    }

    /// `list-bible-books`: the books of a bible.
    fn list_bible_books(&mut self, args: &Args) -> Result<String, String> {
        let want = text(args, "bible");
        let id = if want.is_empty() {
            self.bible.current.as_ref().map(|b| b.meta.id.clone()).ok_or("no bible is open (list-bibles, then bible=<id>)")?
        } else {
            self.find_bible(want)?
        };
        let testament = crate::bible::Testament::parse(text(args, "testament")).unwrap_or_default();
        let bible = self.cached_bible(&id).ok_or("the bible could not be read")?;
        let lang = crate::i18n::current_language();
        let list: Vec<String> = bible
            .books
            .iter()
            .enumerate()
            .filter(|(_, b)| testament.contains(b.number))
            .map(|(i, b)| format!("{:>3}  {}  ({} chapters)", b.number, bible.book_name(i, &lang), b.chapters.len()))
            .collect();
        Ok(if list.is_empty() { "no books".into() } else { list.join("\n") })
    }

    fn bible_command(&mut self, args: &Args) -> Result<String, String> {
        let want = text(args, "bible");
        if !want.is_empty() {
            let id = self.find_bible(want)?;
            if self.bible.current.as_ref().map(|b| &b.meta.id) != Some(&id) {
                self.switch_bible(&id);
            }
        }
        if self.bible.current.is_none() {
            return Err("no bible is open (list-bibles, then bible=<id>)".into());
        }
        // the word search's filter (it stays set, as in the Bible tab)
        if let Some(tm) = args.get("testament").and_then(Value::as_str) {
            self.bible.filter.testament =
                crate::bible::Testament::parse(tm).ok_or_else(|| format!("testament: all, old or new, not {tm:?}"))?;
            if self.bible.filter.book.is_some_and(|b| !self.bible.filter.testament.contains(b)) {
                self.bible.filter.book = None;
            }
        }
        if let Some(book) = args.get("book").and_then(Value::as_str) {
            self.bible.filter.book = if book.trim().is_empty() || crate::storage::fold(book) == "all" {
                None
            } else {
                let bible = self.bible.current.as_ref().ok_or("no bible")?;
                let i = bible
                    .find_book(book, &crate::i18n::current_language())
                    .ok_or_else(|| format!("no book {book:?} (list-bible-books)"))?;
                Some(bible.books[i].number)
            };
        }
        let query = text(args, "query");
        let filtered = args.contains_key("testament") || args.contains_key("book");
        if !query.is_empty() {
            self.bible.query = query.to_string();
            self.run_bible_search();
        } else if filtered && self.bible.word_search && !self.bible.query.trim().is_empty() {
            // a changed filter runs the last word search again (as in the Bible tab)
            self.run_bible_search();
        }
        match text(args, "action") {
            "search" => {
                let bible = self.bible.current.as_ref().ok_or("no bible")?;
                let lang = crate::i18n::current_language();
                let sep = if lang == "cs" { "," } else { ":" };
                let limit = args.get("limit").and_then(Value::as_u64).unwrap_or(50).clamp(1, 500) as usize;
                let mut out = String::new();
                for h in self.bible.results.iter().take(limit) {
                    if let Some((_, c, v)) = bible.verse(h) {
                        out.push_str(&format!("{} {}{sep}{}  {}\n", bible.book_name(h.book, &lang), c.number, v.number, v.text));
                    }
                }
                if out.is_empty() {
                    out = if self.bible.info.is_empty() { "nothing found".into() } else { self.bible.info.clone() };
                } else if self.bible.results.len() > limit {
                    out.push_str(&format!("… {} verses found, {limit} listed (limit=)", self.bible.results.len()));
                }
                Ok(out.trim_end().to_string())
            }
            "show" => {
                if self.bible.results.is_empty() {
                    return Err("nothing found to show (bible search <query> first)".into());
                }
                self.bible_show_live();
                Ok(self.status_text())
            }
            _ => {
                let p = self.bible_selected_passage().ok_or("nothing found to add (bible search <query> first)")?;
                let r = p.reference.clone();
                self.add_to_service(ItemKind::Bible(p));
                Ok(format!("added {r}"))
            }
        }
    }

    /// `set-setting`: changes one setting (a `.` path into the settings' JSON), as Settings does.
    fn set_setting(&mut self, key: &str, value: &str, ctx: &egui::Context) -> Result<String, String> {
        let value: Value = serde_json::from_str(value).map_err(|e| format!("value is not JSON ({e}); a text needs quotes: \"cs\""))?;
        let mut all = serde_json::to_value(&self.settings).map_err(|e| e.to_string())?;
        let mut at = &mut all;
        for part in key.split('.') {
            at = at.get_mut(part).ok_or_else(|| format!("no setting {key:?} (get-settings lists them)"))?;
        }
        *at = value;
        let new: crate::settings::Settings = serde_json::from_value(all).map_err(|e| format!("{key}: {e}"))?;
        let language_changed = new.language != self.settings.language;
        let theme_changed = new.dark_mode != self.settings.dark_mode;
        self.settings = new;
        if language_changed {
            crate::i18n::set_language(&self.settings.language);
        }
        if theme_changed {
            ctx.set_visuals(if self.settings.dark_mode { egui::Visuals::dark() } else { egui::Visuals::light() });
        }
        self.save_settings();
        Ok(format!("{key} set"))
    }

    /// `bible-item`: the selected bible item's translations, languages and the verse shown.
    fn bible_item_command(&mut self, args: &Args) -> Result<String, String> {
        let r = self.sel_item.ok_or("no item is selected (select-item first)")?;
        if !matches!(self.item(r).map(|it| &it.kind), Some(ItemKind::Bible(_))) {
            return Err("the selected item is not a bible passage".into());
        }
        let p = match self.item(r).map(|it| &it.kind) {
            Some(ItemKind::Bible(p)) => p.clone(),
            _ => unreachable!(),
        };
        self.passage_bible_id(&p).ok_or_else(|| format!("the passage's bible ({}) is not installed (list-bibles)", p.version))?;
        let translation = text(args, "translation");
        if !translation.is_empty() {
            let id = self.find_bible(translation)?;
            self.bible_item_translation(r, &id);
        }
        let language = text(args, "language");
        if !language.is_empty() {
            let code = crate::bible::normalize_language(language);
            if !self.bible.bibles.iter().any(|b| b.language == code) {
                return Err(format!("no bible in language {language:?} (list-bibles)"));
            }
            let on = args.get("show").and_then(Value::as_str).is_none_or(|s| s == "on");
            let shown_langs: Vec<String> = p
                .shown_bibles()
                .iter()
                .filter_map(|id| self.bible.bibles.iter().find(|b| &b.id == id).map(|b| b.language.clone()))
                .collect();
            if !on && shown_langs.len() <= 1 && shown_langs.contains(&code) {
                return Err("the only shown language cannot be turned off".into());
            }
            self.bible_item_language(r, &code, on);
        }
        let chapter = args.get("chapter").and_then(Value::as_u64).map(|n| n as u32);
        let verse = args.get("verse").and_then(Value::as_u64).map(|n| n as u32);
        if chapter.is_some() || verse.is_some() {
            let (book, now_chapter, _) = self.bible_now(r).ok_or("the passage has no verse numbers")?;
            let chapter = chapter.unwrap_or(now_chapter);
            let verse = verse.unwrap_or(1);
            if !self.bible_item_show_verse(r, book, chapter, verse) {
                return Err(format!("no verse {chapter}:{verse} in this book"));
            }
        }
        // what it shows now
        let p = match self.item(r).map(|it| &it.kind) {
            Some(ItemKind::Bible(p)) => p.clone(),
            _ => return Err("the item is gone".into()),
        };
        let shown: Vec<String> = p
            .shown_bibles()
            .iter()
            .filter_map(|id| self.bible.bibles.iter().find(|b| &b.id == id))
            .map(|b| format!("{} ({}, {})", b.abbrev, b.name, b.language))
            .collect();
        let now = match self.bible_now(r) {
            Some((b, c, v)) => {
                let bible = self.passage_bible_id(&p).and_then(|id| self.cached_bible(&id));
                let name = bible
                    .as_ref()
                    .and_then(|bb| bb.books.iter().position(|x| x.number == b).map(|i| bb.book_name(i, &crate::i18n::current_language())))
                    .unwrap_or_default();
                let text =
                    bible.as_ref().and_then(|bb| super::bible_item::find_verse(bb, b, c, v)).map(|x| x.text.clone()).unwrap_or_default();
                format!("{name} {c}:{v}  {text}")
            }
            None => "-".into(),
        };
        Ok(format!("passage: {}\nshown: {}\nnow: {now}\n{}", p.reference, shown.join(", "), self.status_text()))
    }

    /// `set-background`: the service's or the selected item's background picture.
    fn set_background_command(&mut self, args: &Args) -> Result<String, String> {
        use super::backgrounds::BackgroundTarget;
        let target = if text(args, "scope") == "service" {
            BackgroundTarget::Service
        } else {
            BackgroundTarget::Item(self.service.items[self.selected_index()?].id.clone())
        };
        if flag(args, "clear") {
            self.set_background(&target, None)?;
            return Ok("background picture removed".into());
        }
        let f = PathBuf::from(text(args, "file"));
        if text(args, "file").is_empty() {
            return Err("set-background needs file=<picture> or clear=true".into());
        }
        if !f.is_file() {
            return Err(format!("no such file: {}", f.display()));
        }
        let pic = Self::background_picture(&f)?;
        self.set_background(&target, Some(pic))?;
        Ok("background picture set".into())
    }
}

/// A tab by its name in the commands (`show-tab`, `add-to-service`).
fn tab_named(name: &str) -> Tab {
    match name {
        "songs" => Tab::Songs,
        "bible" => Tab::Bible,
        "designer" => Tab::Designer,
        "settings" => Tab::Settings,
        _ => Tab::Service,
    }
}
