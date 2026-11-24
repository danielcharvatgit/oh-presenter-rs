//! Videos in the service: adding them, the live playback controls and the sound.

use super::service_tab::ServiceArea;
use super::{App, ItemRef, Tab, files};
use crate::i18n::{t, tf};
use crate::model::*;
use crate::storage;
use crate::video;
use eframe::egui;
use std::path::PathBuf;
use std::time::Instant;

/// Seconds ← / → move a video.
pub const VIDEO_SEEK_STEP: f64 = 5.0;

/// The video the video keys (Space, ← / →) work on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VideoTarget {
    /// The live video.
    Live,
    /// The selected video item, which is not live (Space shows it live).
    Selected,
}

impl App {
    /// The video the video keys work on now: in the Service tab, after a click in the item
    /// list or the slides, the selected video item; after a click in the live panel, the
    /// live video. `None` = Space and the arrows keep their slide meaning.
    pub fn video_key_target(&self) -> Option<VideoTarget> {
        if self.tab != Tab::Service {
            return None;
        }
        let live_video = matches!(self.live, Scene::Video(_));
        match self.service_area? {
            ServiceArea::Live => live_video.then_some(VideoTarget::Live),
            ServiceArea::List | ServiceArea::Slides => {
                let sel = self.sel_item?;
                if !matches!(self.item(sel).map(|i| &i.kind), Some(ItemKind::Video(_))) {
                    return None;
                }
                let is_live = live_video && matches!(self.live_pos, Some((r, _)) if r == sel);
                Some(if is_live { VideoTarget::Live } else { VideoTarget::Selected })
            }
        }
    }

    /// The video Space (play / pause) works on, wherever was clicked last: as
    /// [`App::video_key_target`]; else the selected video item (live or not); else the live
    /// video, whatever is selected (Space is no slide key, so it is always the video's).
    pub fn play_pause_target(&self) -> Option<VideoTarget> {
        if self.tab != Tab::Service {
            return None;
        }
        if let Some(t) = self.video_key_target() {
            return Some(t);
        }
        let live_video = matches!(self.live, Scene::Video(_));
        if let Some(sel) = self.sel_item
            && matches!(self.item(sel).map(|i| &i.kind), Some(ItemKind::Video(_)))
        {
            let is_live = live_video && matches!(self.live_pos, Some((r, _)) if r == sel);
            return Some(if is_live { VideoTarget::Live } else { VideoTarget::Selected });
        }
        live_video.then_some(VideoTarget::Live)
    }

    /// Space on a video: the live video plays / pauses (from the start once it ended; a
    /// blanked screen is shown again); a selected video that is not live goes live.
    pub fn video_play_pause(&mut self, target: VideoTarget) {
        if target == VideoTarget::Selected {
            // a video item has one slide
            self.sel_slide = 0;
            self.go_live();
            return;
        }
        let Scene::Video(v) = &self.live else { return };
        let elapsed = self.video_since.elapsed().as_secs_f64();
        let (playing, ended) = (!v.paused && !v.ended_after(elapsed), v.ended_after(elapsed));
        if self.blank {
            self.toggle_blank();
        }
        if ended {
            self.video_update(|v| {
                v.position = 0.0;
                v.paused = false;
            });
        } else {
            self.video_update(|v| v.paused = playing);
        }
    }

    /// Moves the live video by `secs` (negative = back), within its length.
    pub fn video_seek(&mut self, secs: f64) {
        self.video_update(|v| {
            let end = if v.duration > 0.0 { v.duration } else { f64::INFINITY };
            v.position = (v.position + secs).clamp(0.0, end);
        });
    }

    pub fn import_videos(&mut self) {
        if !video::video_available() {
            self.error(t("video.no_ffmpeg_long"));
            return;
        }
        self.open_file_dialog(
            files::Spec::new(files::Mode::PickFiles, t("import.videos")).filter(t("file.video_filter"), video::VIDEO_EXTS),
            files::Purpose::ImportVideos,
        );
    }

    /// Copies the videos into the library and reads their length and a still picture,
    /// in the background (a video can be large); they are added when done.
    pub(super) fn import_video_files(&mut self, files: Vec<PathBuf>) {
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            for f in files {
                let title = f.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
                let res = video::probe(&f).and_then(|(duration, poster)| {
                    let media = storage::store_media_file_streamed(&f).map_err(|e| e.to_string())?;
                    let poster = poster.and_then(|png| storage::store_media_bytes(&png, "png").ok());
                    Ok(ItemKind::Video(Video { title, media, poster, duration, looped: false, muted: false }))
                });
                if tx.send(res.map_err(|e| format!("{}: {e}", f.display()))).is_err() {
                    return;
                }
            }
        });
        self.video_import = Some(rx);
        self.notify(t("video.adding"));
    }

    pub(super) fn poll_video_import(&mut self, ctx: &egui::Context) {
        let Some(rx) = &self.video_import else { return };
        let mut added = Vec::new();
        let mut errors = Vec::new();
        let mut done = false;
        loop {
            match rx.try_recv() {
                Ok(Ok(kind)) => added.push(kind),
                Ok(Err(e)) => errors.push(e),
                Err(std::sync::mpsc::TryRecvError::Empty) => break,
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    done = true;
                    break;
                }
            }
        }
        if done {
            self.video_import = None;
        } else {
            ctx.request_repaint_after(std::time::Duration::from_millis(200));
        }
        let any = !added.is_empty();
        for kind in added {
            self.add_to_service(kind);
        }
        if any && !video::sound_available() {
            self.notify(t("video.no_ffplay"));
        }
        if !errors.is_empty() {
            self.error(format!("{}\n\n{}", t("video.add_failed"), errors.join("\n")));
        }
    }

    /// Changes the live video's playback (pause, play, seek...): its state is taken at
    /// the current position and sent to all outputs again.
    pub fn video_update(&mut self, f: impl FnOnce(&mut VideoScene)) {
        let elapsed = self.video_since.elapsed().as_secs_f64();
        let Scene::Video(v) = &mut self.live else { return };
        v.position = v.position_after(elapsed);
        f(v);
        v.token = video::next_token();
        self.video_since = Instant::now();
        self.broadcast();
    }

    /// Starts or stops the sound (ffplay or VLC) of the video on the output (the live one,
    /// or the frozen one while the output is frozen).
    pub(super) fn sync_video_audio(&mut self) {
        let (shown, since) = match &self.frozen {
            Some((scene, since)) => (scene, *since),
            None => (&self.live, self.video_since),
        };
        let elapsed = since.elapsed().as_secs_f64();
        let wanted = match shown {
            Scene::Video(v) if !self.blank && !v.paused && !v.muted && !v.ended_after(elapsed) => Some(v),
            _ => None,
        };
        match wanted {
            None => self.video_audio = None,
            Some(v) => {
                let path = storage::media_path(&v.media);
                // ffmpeg turned out not to read this file: its sound goes over to VLC too
                let switch = self.video_audio.as_ref().is_some_and(|(_, a)| a.is_ffplay()) && video::uses_vlc(&path);
                if (self.video_audio.as_ref().is_some_and(|(token, _)| *token == v.token) && !switch) || !video::sound_available() {
                    return;
                }
                self.video_audio = video::Audio::start(&path, v.position_after(elapsed), v.looped).ok().map(|a| (v.token, a));
            }
        }
    }

    /// Loop / sound switches of a video item (in the slides panel). A change also applies
    /// to the video when it is live.
    pub(super) fn video_item_options(&mut self, ui: &mut egui::Ui, item_ref: ItemRef) {
        let ItemRef::Service(i) = item_ref else { return };
        let Some(ItemKind::Video(v)) = self.service.items.get_mut(i).map(|it| &mut it.kind) else { return };
        let mut changed = false;
        ui.horizontal_wrapped(|ui| {
            ui.label(t("service.text_title"));
            changed |= ui.text_edit_singleline(&mut v.title).changed();
        });
        ui.horizontal_wrapped(|ui| {
            changed |= ui.checkbox(&mut v.looped, t("video.loop")).changed();
            changed |= ui.checkbox(&mut v.muted, t("video.mute")).changed();
            if v.duration > 0.0 {
                ui.weak(tf("video.length", &[("time", &video::format_time(v.duration))]));
            }
        });
        if changed {
            let (looped, muted, media) = (v.looped, v.muted, v.media.clone());
            self.service_dirty = true;
            if matches!(self.live_pos, Some((ItemRef::Service(l), _)) if l == i)
                && matches!(&self.live, Scene::Video(lv) if lv.media == media)
            {
                self.video_update(|lv| {
                    lv.looped = looped;
                    lv.muted = muted;
                });
            }
        }
        ui.separator();
    }

    /// The live preview of a video (Service tab): with the keyboard focus on it, Space
    /// plays / pauses and ← → go 5 seconds back / forward.
    pub(super) fn video_preview_id() -> egui::Id {
        egui::Id::unique("live_video_preview")
    }

    /// Space / ← → for the live video while its preview has the keyboard focus, and the
    /// play / pause keys while a row of the service item list has it (egui would press the
    /// row with Space). Runs before the shortcuts, which would step the slides with ← →.
    pub(super) fn video_keys(&mut self, ctx: &egui::Context) {
        if self.tab != Tab::Service || self.dialog.is_some() || self.file_dialog.is_some() || self.capture.is_some() {
            return;
        }
        let focused = ctx.memory(|m| m.focused());
        if focused.is_some_and(|f| self.service_row_ids.contains(&f))
            && let Some(target) = self.play_pause_target()
        {
            let keys = self.settings.shortcuts.bindings(crate::shortcuts::Action::VideoPlayPause);
            let hit = ctx.input_mut(|i| keys.iter().any(|b| b.consume(i)));
            if hit {
                self.video_play_pause(target);
            }
        }
        if !matches!(self.live, Scene::Video(_)) || focused != Some(Self::video_preview_id()) {
            return;
        }
        let none = egui::Modifiers::NONE;
        let (space, left, right) = ctx.input_mut(|i| {
            (i.consume_key(none, egui::Key::Space), i.consume_key(none, egui::Key::ArrowLeft), i.consume_key(none, egui::Key::ArrowRight))
        });
        if space {
            self.video_play_pause(VideoTarget::Live);
        }
        if left {
            self.video_seek(-VIDEO_SEEK_STEP);
        }
        if right {
            self.video_seek(VIDEO_SEEK_STEP);
        }
    }

    /// Play / pause, back to the start and the position of the live video (live panel).
    pub(super) fn video_controls(&mut self, ui: &mut egui::Ui) {
        let Scene::Video(v) = &self.live else { return };
        let elapsed = self.video_since.elapsed().as_secs_f64();
        let pos = v.position_after(elapsed);
        let (paused, duration, ended, muted) = (v.paused, v.duration, v.ended_after(elapsed), v.muted);
        let seek_id = egui::Id::unique("video_seek");
        ui.horizontal(|ui| {
            let playing = !paused && !ended;
            let (icon, hint) = if playing { ("⏸", t("video.pause")) } else { ("▶", t("video.play")) };
            let sc = &self.settings.shortcuts;
            let hint = format!("{hint} ({})", sc.describe(crate::shortcuts::Action::VideoPlayPause));
            if ui.button(icon).on_hover_text(hint).clicked() {
                self.video_play_pause(VideoTarget::Live);
            }
            if ui.button("⏮").on_hover_text(t("video.restart")).clicked() {
                self.video_update(|v| v.position = 0.0);
            }
            if duration > 0.0 {
                let mut p = ui.data(|d| d.get_temp::<f64>(seek_id)).unwrap_or(pos);
                ui.label(format!("{} / {}", video::format_time(p), video::format_time(duration)));
                ui.spacing_mut().slider_width = (ui.available_width() - 30.0).max(60.0);
                let r = ui.add(egui::Slider::new(&mut p, 0.0..=duration).show_value(false));
                if r.dragged() {
                    ui.data_mut(|d| d.insert_temp(seek_id, p));
                }
                if r.drag_stopped() || (r.changed() && !r.dragged()) {
                    ui.data_mut(|d| d.remove::<f64>(seek_id));
                    self.video_update(|v| v.position = p);
                }
            } else {
                ui.label(video::format_time(pos));
            }
            if muted {
                ui.weak("🔇").on_hover_text(t("video.mute"));
            }
        });
        if !paused && !ended {
            // the time display
            ui.ctx().request_repaint_after(std::time::Duration::from_millis(250));
        }
    }
}
