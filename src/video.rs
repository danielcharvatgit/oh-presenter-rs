//! Video playback through the `ffmpeg` and `ffplay` programs, or through an installed VLC
//! when ffmpeg is not there (see [`crate::vlc`]; `OH_PRESENTER_VIDEO=vlc` / `ffmpeg` forces one).
//!
//! `ffmpeg` decodes the video in real time into raw RGBA frames on a pipe; a thread
//! reads them and the newest frame is shown as a texture (output window, live preview,
//! network outputs). The sound is played by `ffplay` on the presenter computer only.
//! Using the programs instead of linking the libraries keeps the build free of native
//! dependencies and plays whatever formats the installed ffmpeg knows.
//!
//! The programs are looked for in this order: `OH_PRESENTER_FFMPEG` / `OH_PRESENTER_FFPLAY`,
//! next to the oh-presenter executable (the Windows installer puts them there), then `PATH`.
//!
//! A file the installed ffmpeg cannot read (an unsupported format) is played by VLC when
//! VLC is installed: [`probe`] tries VLC after ffmpeg failed, and a playback in which ffmpeg
//! delivered no picture marks the file ([`uses_vlc`]), so it is decoded (and its sound
//! played) by VLC from then on.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

/// File extensions offered when adding a video.
pub const VIDEO_EXTS: &[&str] = &["mp4", "m4v", "mov", "mkv", "webm", "avi", "wmv", "mpg", "mpeg", "ts", "ogv", "flv", "3gp"];

pub fn is_video_name(name: &str) -> bool {
    let ext = name.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase()).unwrap_or_default();
    VIDEO_EXTS.contains(&ext.as_str())
}

fn tool(name: &str, env: &str) -> PathBuf {
    if let Some(p) = std::env::var_os(env).filter(|p| !p.is_empty()) {
        return PathBuf::from(p);
    }
    let file = if cfg!(windows) { format!("{name}.exe") } else { name.to_string() };
    if let Some(dir) = std::env::current_exe().ok().and_then(|e| e.parent().map(Path::to_path_buf)) {
        for candidate in [dir.join(&file), dir.join("ffmpeg").join(&file)] {
            if candidate.is_file() {
                return candidate;
            }
        }
    }
    PathBuf::from(file)
}

pub fn ffmpeg_path() -> PathBuf {
    tool("ffmpeg", "OH_PRESENTER_FFMPEG")
}

pub fn ffplay_path() -> PathBuf {
    tool("ffplay", "OH_PRESENTER_FFPLAY")
}

/// A command without a console window (the release build on Windows has none of its own).
fn command(program: PathBuf) -> Command {
    #[allow(unused_mut)]
    let mut c = Command::new(program);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        c.creation_flags(CREATE_NO_WINDOW);
    }
    c.stdin(Stdio::null());
    c
}

fn runs(program: PathBuf) -> bool {
    command(program).arg("-version").stdout(Stdio::null()).stderr(Stdio::null()).status().is_ok_and(|s| s.success())
}

/// Is `ffmpeg` installed? (checked once)
pub fn ffmpeg_available() -> bool {
    static OK: OnceLock<bool> = OnceLock::new();
    *OK.get_or_init(|| runs(ffmpeg_path()))
}

/// Is `ffplay` (for the sound) installed? (checked once)
pub fn ffplay_available() -> bool {
    static OK: OnceLock<bool> = OnceLock::new();
    *OK.get_or_init(|| runs(ffplay_path()))
}

/// What plays the videos.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Backend {
    Ffmpeg,
    Vlc,
}

/// The player to use: ffmpeg when found, else VLC (`OH_PRESENTER_VIDEO` can force one).
pub fn choose(forced: Option<&str>, ffmpeg: bool, vlc: impl FnOnce() -> bool) -> Option<Backend> {
    match forced.map(|f| f.trim().to_ascii_lowercase()).as_deref() {
        Some("vlc") => return vlc().then_some(Backend::Vlc),
        Some("ffmpeg") => return ffmpeg.then_some(Backend::Ffmpeg),
        _ => {}
    }
    if ffmpeg {
        Some(Backend::Ffmpeg)
    } else if vlc() {
        Some(Backend::Vlc)
    } else {
        None
    }
}

fn forced() -> Option<String> {
    std::env::var("OH_PRESENTER_VIDEO").ok().filter(|v| !v.trim().is_empty())
}

/// What decodes the pictures (checked once).
pub fn backend() -> Option<Backend> {
    static B: OnceLock<Option<Backend>> = OnceLock::new();
    *B.get_or_init(|| choose(forced().as_deref(), ffmpeg_available(), crate::vlc::available))
}

/// What plays the sound: ffplay, else VLC (checked once).
pub fn sound_backend() -> Option<Backend> {
    static B: OnceLock<Option<Backend>> = OnceLock::new();
    *B.get_or_init(|| choose(forced().as_deref(), ffplay_available(), crate::vlc::available))
}

/// Files ffmpeg could not decode (they are played by VLC, see [`uses_vlc`]).
fn vlc_files() -> &'static Mutex<std::collections::HashSet<PathBuf>> {
    static F: OnceLock<Mutex<std::collections::HashSet<PathBuf>>> = OnceLock::new();
    F.get_or_init(Default::default)
}

/// Marks a file ffmpeg could not decode: VLC (when installed) plays it from now on.
fn mark_for_vlc(path: &Path) {
    if forced().is_none() && crate::vlc::available() {
        vlc_files().lock().unwrap_or_else(|e| e.into_inner()).insert(path.to_path_buf());
    }
}

/// Whether this file is played by VLC although ffmpeg is used otherwise (ffmpeg could not
/// decode it and VLC is installed).
pub fn uses_vlc(path: &Path) -> bool {
    vlc_files().lock().unwrap_or_else(|e| e.into_inner()).contains(path)
}

/// The backend for pictures of this file.
fn backend_for(path: &Path) -> Option<Backend> {
    match backend() {
        Some(Backend::Ffmpeg) if uses_vlc(path) => Some(Backend::Vlc),
        b => b,
    }
}

/// Can videos be played (ffmpeg or VLC)?
pub fn video_available() -> bool {
    backend().is_some()
}

/// Can the sound of videos be played (ffplay or VLC)?
pub fn sound_available() -> bool {
    sound_backend().is_some()
}

/// A new playback token: unique also across restarts of the presenter, so a network
/// output never mistakes a new playback for the one it already plays.
pub fn next_token() -> u64 {
    static NEXT: OnceLock<AtomicU64> = OnceLock::new();
    NEXT.get_or_init(|| {
        let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos() as u64).unwrap_or(1);
        AtomicU64::new(nanos)
    })
    .fetch_add(1, Ordering::Relaxed)
}

/// Reads the length (seconds) from ffmpeg's `Duration: 00:03:12.48` line.
pub fn parse_duration(ffmpeg_log: &str) -> Option<f64> {
    let rest = ffmpeg_log.split("Duration:").nth(1)?.trim_start();
    let time = rest.split([',', ' ']).next()?;
    let mut secs = 0.0;
    for part in time.split(':') {
        secs = secs * 60.0 + part.trim().parse::<f64>().ok()?;
    }
    Some(secs)
}

/// Length in seconds and a still picture (PNG) of a video file.
pub fn probe(path: &Path) -> Result<(f64, Option<Vec<u8>>), String> {
    match backend() {
        None => return Err(crate::i18n::t("video.no_ffmpeg")),
        Some(Backend::Vlc) => return crate::vlc::probe(path),
        Some(Backend::Ffmpeg) => {}
    }
    match probe_ffmpeg(path) {
        // a format the installed ffmpeg does not know: VLC may
        Err(e) if forced().is_none() && crate::vlc::available() => {
            let r = crate::vlc::probe(path).map_err(|_| e)?;
            mark_for_vlc(path);
            Ok(r)
        }
        r => r,
    }
}

fn probe_ffmpeg(path: &Path) -> Result<(f64, Option<Vec<u8>>), String> {
    let mut duration = 0.0;
    // a frame one second in (often nicer than the first, black one); short videos: the first
    for seek in ["1", "0"] {
        let out = command(ffmpeg_path())
            .args(["-hide_banner", "-nostdin", "-ss", seek, "-i"])
            .arg(path)
            .args(["-an", "-sn", "-frames:v", "1", "-vf", "scale=640:-2", "-f", "image2pipe", "-vcodec", "png", "-"])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .map_err(|e| e.to_string())?;
        let log = String::from_utf8_lossy(&out.stderr);
        if let Some(d) = parse_duration(&log) {
            duration = d;
        }
        if !out.stdout.is_empty() {
            return Ok((duration, Some(out.stdout)));
        }
        if seek == "0" {
            let last = log.lines().rev().find(|l| !l.trim().is_empty()).unwrap_or("").to_string();
            if duration == 0.0 {
                return Err(if last.is_empty() { crate::i18n::t("video.not_video") } else { last });
            }
        }
    }
    Ok((duration, None))
}

/// Converts a picture ffmpeg can read (HEIC / HEIF, AVIF, ...) to PNG.
pub fn image_to_png(path: &Path) -> Result<Vec<u8>, String> {
    if !ffmpeg_available() {
        return Err(crate::i18n::t("image.needs_ffmpeg"));
    }
    let out = command(ffmpeg_path())
        .args(["-hide_banner", "-loglevel", "error", "-nostdin", "-i"])
        .arg(path)
        .args(["-an", "-frames:v", "1", "-f", "image2pipe", "-vcodec", "png", "-"])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .map_err(|e| e.to_string())?;
    if out.stdout.is_empty() {
        let log = String::from_utf8_lossy(&out.stderr);
        return Err(log.lines().rev().find(|l| !l.trim().is_empty()).unwrap_or("ffmpeg").to_string());
    }
    Ok(out.stdout)
}

/// The frame size decoded for a picture `width` pixels wide (fewer pixels for previews).
pub fn frame_size(width: f32) -> (u32, u32) {
    if width > 1300.0 {
        (1920, 1080)
    } else if width > 700.0 {
        (1280, 720)
    } else {
        (640, 360)
    }
}

struct Frames {
    /// The newest decoded frame and its number.
    latest: Mutex<Option<(u64, Vec<u8>)>>,
    finished: AtomicBool,
}

enum Decoder {
    Ffmpeg { child: Child, frames: Arc<Frames> },
    Vlc { video: crate::vlc::Video, frames: Arc<crate::vlc::Frames> },
}

/// Decodes one video (ffmpeg or VLC), in real time from `position` (or a single frame when paused).
pub struct Player {
    decoder: Decoder,
    size: (u32, u32),
    shown: u64,
    texture: Option<eframe::egui::TextureHandle>,
}

impl Player {
    pub fn start(path: &Path, position: f64, looped: bool, paused: bool, size: (u32, u32)) -> std::io::Result<Self> {
        match backend_for(path) {
            Some(Backend::Ffmpeg) => Self::start_ffmpeg(path, position, looped, paused, size),
            Some(Backend::Vlc) => {
                let frames = Arc::new(crate::vlc::Frames::default());
                let video = crate::vlc::Video::start(path, position, looped, paused, size, frames.clone())?;
                Ok(Self { decoder: Decoder::Vlc { video, frames }, size, shown: 0, texture: None })
            }
            None => Err(std::io::Error::new(std::io::ErrorKind::NotFound, "no ffmpeg or VLC")),
        }
    }

    fn start_ffmpeg(path: &Path, position: f64, looped: bool, paused: bool, size: (u32, u32)) -> std::io::Result<Self> {
        let (w, h) = size;
        let mut c = command(ffmpeg_path());
        c.args(["-hide_banner", "-loglevel", "error", "-nostdin"]);
        if !paused {
            // read at the video's own speed: the pipe delivers frames in real time
            c.arg("-re");
        }
        if looped && !paused {
            c.args(["-stream_loop", "-1"]);
        }
        c.args(["-ss", &format!("{:.3}", position.max(0.0))]).arg("-i").arg(path);
        // letterboxed into the 16:9 frame, black around
        let vf = format!("scale={w}:{h}:force_original_aspect_ratio=decrease,pad={w}:{h}:(ow-iw)/2:(oh-ih)/2:black,format=rgba");
        c.args(["-an", "-sn", "-vf", &vf]);
        if paused {
            c.args(["-frames:v", "1"]);
        }
        c.args(["-f", "rawvideo", "-pix_fmt", "rgba", "-"]);
        let mut child = c.stdout(Stdio::piped()).stderr(Stdio::null()).spawn()?;
        let mut stdout = child.stdout.take().expect("piped stdout");
        let frames = Arc::new(Frames { latest: Mutex::new(None), finished: AtomicBool::new(false) });
        let f = frames.clone();
        let file = path.to_path_buf();
        std::thread::spawn(move || {
            let len = w as usize * h as usize * 4;
            let mut n = 0;
            loop {
                let mut buf = vec![0u8; len];
                if stdout.read_exact(&mut buf).is_err() {
                    break;
                }
                n += 1;
                *f.latest.lock().unwrap_or_else(|e| e.into_inner()) = Some((n, buf));
            }
            if n == 0 {
                // not a single picture: ffmpeg cannot decode this file, VLC may
                mark_for_vlc(&file);
            }
            f.finished.store(true, Ordering::Relaxed);
        });
        Ok(Self { decoder: Decoder::Ffmpeg { child, frames }, size, shown: 0, texture: None })
    }

    pub fn size(&self) -> (u32, u32) {
        self.size
    }

    /// The decoder has ended (end of the video, a single paused frame delivered, or an error).
    pub fn finished(&self) -> bool {
        match &self.decoder {
            Decoder::Ffmpeg { frames, .. } => frames.finished.load(Ordering::Relaxed),
            Decoder::Vlc { video, .. } => video.finished(),
        }
    }

    /// Decoded by ffmpeg (not VLC).
    pub fn is_ffmpeg(&self) -> bool {
        matches!(self.decoder, Decoder::Ffmpeg { .. })
    }

    /// No picture has been shown yet.
    pub fn no_frame_yet(&self) -> bool {
        self.shown == 0
    }

    /// The newest frame as a texture (None until the first frame arrives).
    pub fn texture(&mut self, ctx: &eframe::egui::Context) -> Option<&eframe::egui::TextureHandle> {
        let newest = match &self.decoder {
            Decoder::Ffmpeg { frames, .. } => frames.latest.lock().unwrap_or_else(|e| e.into_inner()).take(),
            Decoder::Vlc { frames, .. } => frames
                .latest
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .take()
                .filter(|(_, s, _)| *s == self.size)
                .map(|(n, _, rgba)| (n, rgba)),
        };
        if let Some((n, rgba)) = newest
            && n != self.shown
        {
            self.shown = n;
            let image = eframe::egui::ColorImage::from_rgba_unmultiplied([self.size.0 as usize, self.size.1 as usize], &rgba);
            match &mut self.texture {
                Some(t) => t.set(image, eframe::egui::TextureOptions::LINEAR),
                None => self.texture = Some(ctx.load_texture("video-frame", image, eframe::egui::TextureOptions::LINEAR)),
            }
        }
        self.texture.as_ref()
    }
}

impl Drop for Player {
    fn drop(&mut self) {
        if let Decoder::Ffmpeg { child, .. } = &mut self.decoder {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

/// The sound of a video, played by ffplay or VLC (no window).
pub enum Audio {
    Ffplay(Child),
    /// (stops when dropped)
    Vlc(#[allow(dead_code)] crate::vlc::Sound),
}

impl Audio {
    pub fn start(path: &Path, position: f64, looped: bool) -> std::io::Result<Self> {
        let backend = match sound_backend() {
            Some(Backend::Ffmpeg) if uses_vlc(path) => Some(Backend::Vlc),
            b => b,
        };
        match backend {
            Some(Backend::Ffmpeg) => {}
            Some(Backend::Vlc) => return crate::vlc::Sound::start(path, position, looped).map(Self::Vlc),
            None => return Err(std::io::Error::new(std::io::ErrorKind::NotFound, "no ffplay or VLC")),
        }
        let mut c = command(ffplay_path());
        c.args(["-hide_banner", "-loglevel", "error", "-nodisp", "-vn", "-sn"]);
        c.args(["-ss", &format!("{:.3}", position.max(0.0))]);
        if looped {
            c.args(["-loop", "0"]);
        } else {
            c.arg("-autoexit");
        }
        c.arg(path);
        let child = c.stdout(Stdio::null()).stderr(Stdio::null()).spawn()?;
        Ok(Self::Ffplay(child))
    }
}

impl Audio {
    /// Played by ffplay (not VLC).
    pub fn is_ffplay(&self) -> bool {
        matches!(self, Self::Ffplay(_))
    }
}

impl Drop for Audio {
    fn drop(&mut self) {
        if let Self::Ffplay(child) = self {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

/// Formats seconds as `m:ss` (or `h:mm:ss`).
pub fn format_time(secs: f64) -> String {
    let s = secs.max(0.0) as u64;
    if s >= 3600 { format!("{}:{:02}:{:02}", s / 3600, s / 60 % 60, s % 60) } else { format!("{}:{:02}", s / 60, s % 60) }
}
