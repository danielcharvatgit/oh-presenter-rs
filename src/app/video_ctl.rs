//! Videos in the service: adding them, the live playback controls and the sound.

use super::{App, ItemRef, files};
use crate::i18n::{t, tf};
use crate::model::*;
use crate::storage;
use crate::video;
use eframe::egui;
use std::path::PathBuf;
use std::time::Instant;

impl App {
    pub fn import_videos(&mut self) {
        if !video::ffmpeg_available() {
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
        if any && !video::ffplay_available() {
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

    /// Starts or stops the live video's sound (ffplay) to match the live state.
    pub(super) fn sync_video_audio(&mut self) {
        let elapsed = self.video_since.elapsed().as_secs_f64();
        let wanted = match &self.live {
            Scene::Video(v) if !self.blank && !v.paused && !v.muted && !v.ended_after(elapsed) => Some(v),
            _ => None,
        };
        match wanted {
            None => self.video_audio = None,
            Some(v) => {
                if self.video_audio.as_ref().is_some_and(|(token, _)| *token == v.token) || !video::ffplay_available() {
                    return;
                }
                let path = storage::media_path(&v.media);
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
            if ui.button(icon).on_hover_text(hint).clicked() {
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
