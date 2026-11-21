//! Video playback through an installed VLC (libvlc 3), used when ffmpeg is not there or cannot read a file.
//!
//! The library is loaded at run time (nothing is linked, so the program starts and builds
//! without VLC). VLC decodes the picture into memory (the "vmem" callbacks): every frame is
//! scaled to fit the requested size, letterboxed and handed over like ffmpeg's frames, so
//! it is shown in the program's own output window. A second player plays the sound.
//!
//! Where libvlc is looked for: `OH_PRESENTER_VLC` (the folder of VLC or the library file),
//! next to the oh-presenter executable (also in a `vlc` folder there), then the system: on
//! Windows `C:\Program Files\VideoLAN\VLC` (a 64-bit VLC), on Linux `libvlc.so.5` (the
//! `vlc` package; not inside the flatpak, which brings ffmpeg).

use std::cell::UnsafeCell;
use std::ffi::{CStr, CString, c_char, c_int, c_uint, c_void};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

type Instance = c_void;
type Media = c_void;
type MediaPlayer = c_void;
type LockCb = unsafe extern "C" fn(*mut c_void, *mut *mut c_void) -> *mut c_void;
type UnlockCb = unsafe extern "C" fn(*mut c_void, *mut c_void, *const *mut c_void);
type DisplayCb = unsafe extern "C" fn(*mut c_void, *mut c_void);
type SetupCb = unsafe extern "C" fn(*mut *mut c_void, *mut c_char, *mut c_uint, *mut c_uint, *mut c_uint, *mut c_uint) -> c_uint;
type CleanupCb = unsafe extern "C" fn(*mut c_void);

/// The libvlc 3 functions used here.
struct Api {
    get_version: unsafe extern "C" fn() -> *const c_char,
    new: unsafe extern "C" fn(c_int, *const *const c_char) -> *mut Instance,
    media_new_path: unsafe extern "C" fn(*mut Instance, *const c_char) -> *mut Media,
    media_add_option: unsafe extern "C" fn(*mut Media, *const c_char),
    media_release: unsafe extern "C" fn(*mut Media),
    player_new_from_media: unsafe extern "C" fn(*mut Media) -> *mut MediaPlayer,
    player_release: unsafe extern "C" fn(*mut MediaPlayer),
    play: unsafe extern "C" fn(*mut MediaPlayer) -> c_int,
    stop: unsafe extern "C" fn(*mut MediaPlayer),
    set_pause: unsafe extern "C" fn(*mut MediaPlayer, c_int),
    get_length: unsafe extern "C" fn(*mut MediaPlayer) -> i64,
    get_state: unsafe extern "C" fn(*mut MediaPlayer) -> c_int,
    video_set_callbacks: unsafe extern "C" fn(*mut MediaPlayer, Option<LockCb>, Option<UnlockCb>, Option<DisplayCb>, *mut c_void),
    video_set_format_callbacks: unsafe extern "C" fn(*mut MediaPlayer, Option<SetupCb>, Option<CleanupCb>),
}

/// The loaded library and its one instance (kept for the whole run).
struct Vlc {
    api: Api,
    instance: *mut Instance,
    version: String,
    _lib: libloading::Library,
}

// libvlc instances and players may be used from any thread
unsafe impl Send for Vlc {}
unsafe impl Sync for Vlc {}

/// libvlc_state_t of libvlc 3: Stopped, Ended, Error.
const STATE_STOPPED: c_int = 5;
const STATE_ENDED: c_int = 6;
const STATE_ERROR: c_int = 7;

/// Whether `version` (`libvlc_get_version`, e.g. "3.0.20 Vetinari") is a libvlc this
/// module speaks: 3.x (libvlc 4 changed the functions used here).
pub fn supported_version(version: &str) -> bool {
    version.trim().split('.').next().and_then(|m| m.parse::<u32>().ok()) == Some(3)
}

/// The library files to try, in order.
fn candidates() -> Vec<PathBuf> {
    let file = if cfg!(windows) { "libvlc.dll" } else { "libvlc.so.5" };
    let mut out = Vec::new();
    if let Some(p) = std::env::var_os("OH_PRESENTER_VLC").filter(|p| !p.is_empty()).map(PathBuf::from) {
        out.push(if p.is_dir() { p.join(file) } else { p });
    }
    if let Some(dir) = std::env::current_exe().ok().and_then(|e| e.parent().map(Path::to_path_buf)) {
        out.push(dir.join(file));
        out.push(dir.join("vlc").join(file));
    }
    if cfg!(windows) {
        for var in ["ProgramW6432", "ProgramFiles"] {
            if let Some(pf) = std::env::var_os(var) {
                out.push(PathBuf::from(pf).join("VideoLAN").join("VLC").join(file));
            }
        }
    } else {
        // found by the system's library search
        out.push(PathBuf::from(file));
        out.push(PathBuf::from("libvlc.so"));
    }
    out
}

fn open(path: &Path) -> Result<libloading::Library, libloading::Error> {
    #[cfg(windows)]
    {
        // libvlccore.dll and the plugins are found next to libvlc.dll
        const LOAD_WITH_ALTERED_SEARCH_PATH: u32 = 0x0000_0008;
        if path.is_absolute() {
            return unsafe { libloading::os::windows::Library::load_with_flags(path, LOAD_WITH_ALTERED_SEARCH_PATH) }.map(Into::into);
        }
    }
    unsafe { libloading::Library::new(path) }
}

fn load_api(lib: &libloading::Library) -> Result<Api, libloading::Error> {
    unsafe {
        Ok(Api {
            get_version: *lib.get(b"libvlc_get_version\0")?,
            new: *lib.get(b"libvlc_new\0")?,
            media_new_path: *lib.get(b"libvlc_media_new_path\0")?,
            media_add_option: *lib.get(b"libvlc_media_add_option\0")?,
            media_release: *lib.get(b"libvlc_media_release\0")?,
            player_new_from_media: *lib.get(b"libvlc_media_player_new_from_media\0")?,
            player_release: *lib.get(b"libvlc_media_player_release\0")?,
            play: *lib.get(b"libvlc_media_player_play\0")?,
            stop: *lib.get(b"libvlc_media_player_stop\0")?,
            set_pause: *lib.get(b"libvlc_media_player_set_pause\0")?,
            get_length: *lib.get(b"libvlc_media_player_get_length\0")?,
            get_state: *lib.get(b"libvlc_media_player_get_state\0")?,
            video_set_callbacks: *lib.get(b"libvlc_video_set_callbacks\0")?,
            video_set_format_callbacks: *lib.get(b"libvlc_video_set_format_callbacks\0")?,
        })
    }
}

fn load() -> Result<Vlc, String> {
    let mut errors = Vec::new();
    for path in candidates() {
        let lib = match open(&path) {
            Ok(l) => l,
            Err(e) => {
                errors.push(format!("{}: {e}", path.display()));
                continue;
            }
        };
        let api = match load_api(&lib) {
            Ok(a) => a,
            Err(e) => {
                errors.push(format!("{}: {e}", path.display()));
                continue;
            }
        };
        let version = unsafe { CStr::from_ptr((api.get_version)()) }.to_string_lossy().into_owned();
        if !supported_version(&version) {
            errors.push(format!("{}: VLC {version} (needs 3.x)", path.display()));
            continue;
        }
        let mut args = vec!["--quiet", "--no-video-title-show", "--no-osd", "--no-stats"];
        if cfg!(target_os = "linux") {
            // the pictures go to memory: VLC must not talk to the X server itself
            args.push("--no-xlib");
        }
        let args: Vec<CString> = args.into_iter().map(|a| CString::new(a).expect("no NUL")).collect();
        let argv: Vec<*const c_char> = args.iter().map(|a| a.as_ptr()).collect();
        let mut instance = unsafe { (api.new)(argv.len() as c_int, argv.as_ptr()) };
        if instance.is_null() {
            instance = unsafe { (api.new)(0, std::ptr::null()) };
        }
        if instance.is_null() {
            errors.push(format!("{}: libvlc_new failed", path.display()));
            continue;
        }
        return Ok(Vlc { api, instance, version, _lib: lib });
    }
    Err(errors.join("; "))
}

fn vlc() -> Option<&'static Vlc> {
    static VLC: OnceLock<Option<Vlc>> = OnceLock::new();
    VLC.get_or_init(|| match load() {
        Ok(v) => {
            crate::log_info(&format!("video: VLC {} found", v.version));
            Some(v)
        }
        Err(e) => {
            crate::log_info(&format!("video: no usable VLC ({e})"));
            None
        }
    })
    .as_ref()
}

/// Is a usable VLC (libvlc 3) installed? (looked for once)
pub fn available() -> bool {
    vlc().is_some()
}

/// The size a `src` picture gets inside a `boxed` one, keeping its shape (at least 1×1).
pub fn fit(src: (u32, u32), boxed: (u32, u32)) -> (u32, u32) {
    if src.0 == 0 || src.1 == 0 {
        return (boxed.0.max(1), boxed.1.max(1));
    }
    let scale = (boxed.0 as f64 / src.0 as f64).min(boxed.1 as f64 / src.1 as f64);
    (((src.0 as f64 * scale).round() as u32).clamp(1, boxed.0.max(1)), ((src.1 as f64 * scale).round() as u32).clamp(1, boxed.1.max(1)))
}

/// VLC's RV32 picture (bytes B G R X, `size`) as RGBA in the middle of a black `out`
/// picture (letterboxed); `out` = `size` copies it as it is.
pub fn bgrx_to_rgba(src: &[u8], size: (u32, u32), out: (u32, u32)) -> Vec<u8> {
    let (w, h) = (size.0 as usize, size.1 as usize);
    let (ow, oh) = (out.0 as usize, out.1 as usize);
    let mut rgba = vec![0u8; ow * oh * 4];
    for px in rgba.as_chunks_mut::<4>().0 {
        px[3] = 255;
    }
    let (x0, y0) = (ow.saturating_sub(w) / 2, oh.saturating_sub(h) / 2);
    for y in 0..h.min(oh) {
        let row = &src[y * w * 4..(y + 1) * w * 4];
        let dst = &mut rgba[((y0 + y) * ow + x0) * 4..];
        for x in 0..w.min(ow) {
            let s = &row[x * 4..x * 4 + 4];
            dst[x * 4..x * 4 + 4].copy_from_slice(&[s[2], s[1], s[0], 255]);
        }
    }
    rgba
}

/// A decoded frame: its number, size and RGBA pixels.
pub type Frame = (u64, (u32, u32), Vec<u8>);

/// The newest frame and how many there were.
#[derive(Default)]
pub struct Frames {
    pub latest: Mutex<Option<Frame>>,
    pub count: AtomicU64,
}

/// Shared with VLC's video thread (the callbacks).
struct Shared {
    frames: Arc<Frames>,
    /// The box the frames go into; `letterbox` pads them to it, else the fitted size is kept.
    boxed: (u32, u32),
    letterbox: bool,
    /// The fitted size and the buffer VLC draws into: only touched by VLC's video thread
    /// (setup, lock and unlock are called one after another there).
    picture: UnsafeCell<((u32, u32), Vec<u8>)>,
}

unsafe impl Sync for Shared {}

unsafe extern "C" fn setup(
    opaque: *mut *mut c_void,
    chroma: *mut c_char,
    width: *mut c_uint,
    height: *mut c_uint,
    pitches: *mut c_uint,
    lines: *mut c_uint,
) -> c_uint {
    unsafe {
        let shared = &*(*opaque as *const Shared);
        let size = fit((*width, *height), shared.boxed);
        std::ptr::copy_nonoverlapping(b"RV32".as_ptr() as *const c_char, chroma, 4);
        *width = size.0;
        *height = size.1;
        *pitches = size.0 * 4;
        *lines = size.1;
        let picture = &mut *shared.picture.get();
        *picture = (size, vec![0u8; size.0 as usize * size.1 as usize * 4]);
        1
    }
}

unsafe extern "C" fn lock(opaque: *mut c_void, planes: *mut *mut c_void) -> *mut c_void {
    unsafe {
        let shared = &*(opaque as *const Shared);
        let picture = &mut *shared.picture.get();
        *planes = picture.1.as_mut_ptr() as *mut c_void;
        std::ptr::null_mut()
    }
}

unsafe extern "C" fn unlock(opaque: *mut c_void, _picture: *mut c_void, _planes: *const *mut c_void) {
    unsafe {
        // VLC fills the picture right before it is due on the screen: it is handed over now
        let shared = &*(opaque as *const Shared);
        let (size, buf) = &*shared.picture.get();
        let out = if shared.letterbox { shared.boxed } else { *size };
        let rgba = bgrx_to_rgba(buf, *size, out);
        let n = shared.frames.count.fetch_add(1, Ordering::SeqCst) + 1;
        *shared.frames.latest.lock().unwrap_or_else(|e| e.into_inner()) = Some((n, out, rgba));
    }
}

/// The pointers of a player, handed to a thread that stops and frees it.
struct Owned {
    player: *mut MediaPlayer,
    shared: *const Shared,
}

unsafe impl Send for Owned {}

impl Owned {
    /// Stops the player (this waits for VLC's threads) and frees it, then the callback data.
    fn close(self) {
        let Some(v) = vlc() else { return };
        unsafe {
            (v.api.stop)(self.player);
            (v.api.player_release)(self.player);
            if !self.shared.is_null() {
                drop(Box::from_raw(self.shared as *mut Shared));
            }
        }
    }
}

/// One VLC player of one file.
struct Player {
    owned: Option<Owned>,
}

impl Player {
    /// `options`: VLC media options (`:start-time=…`, `:no-audio`…). `video`: where the
    /// pictures go (box size, letterbox), else none are decoded.
    fn start(path: &Path, options: &[String], video: Option<((u32, u32), bool, Arc<Frames>)>) -> std::io::Result<Self> {
        let v = vlc().ok_or_else(|| std::io::Error::other("VLC not found"))?;
        let c_path = CString::new(path.to_string_lossy().as_bytes()).map_err(std::io::Error::other)?;
        unsafe {
            let media = (v.api.media_new_path)(v.instance, c_path.as_ptr());
            if media.is_null() {
                return Err(std::io::Error::other("VLC could not open the file"));
            }
            for o in options {
                if let Ok(o) = CString::new(o.as_str()) {
                    (v.api.media_add_option)(media, o.as_ptr());
                }
            }
            let player = (v.api.player_new_from_media)(media);
            (v.api.media_release)(media);
            if player.is_null() {
                return Err(std::io::Error::other("VLC could not create a player"));
            }
            let mut shared: *const Shared = std::ptr::null();
            if let Some((boxed, letterbox, frames)) = video {
                let s = Box::new(Shared { frames, boxed, letterbox, picture: UnsafeCell::new(((0, 0), Vec::new())) });
                shared = Box::into_raw(s);
                (v.api.video_set_callbacks)(player, Some(lock), Some(unlock), None, shared as *mut c_void);
                (v.api.video_set_format_callbacks)(player, Some(setup), None);
            }
            let owned = Owned { player, shared };
            if (v.api.play)(player) != 0 {
                owned.close();
                return Err(std::io::Error::other("VLC could not play the file"));
            }
            Ok(Self { owned: Some(owned) })
        }
    }

    fn state(&self) -> c_int {
        match (vlc(), &self.owned) {
            (Some(v), Some(o)) => unsafe { (v.api.get_state)(o.player) },
            _ => STATE_ERROR,
        }
    }

    /// At the end of the file, stopped or failed.
    fn done(&self) -> bool {
        matches!(self.state(), STATE_STOPPED | STATE_ENDED | STATE_ERROR)
    }

    /// The file's length (seconds), once VLC knows it.
    fn length(&self) -> Option<f64> {
        let (v, o) = (vlc()?, self.owned.as_ref()?);
        let ms = unsafe { (v.api.get_length)(o.player) };
        (ms > 0).then(|| ms as f64 / 1000.0)
    }

    fn pause(&self) {
        if let (Some(v), Some(o)) = (vlc(), &self.owned) {
            unsafe { (v.api.set_pause)(o.player, 1) };
        }
    }
}

impl Drop for Player {
    fn drop(&mut self) {
        if let Some(o) = self.owned.take() {
            // stopping waits for VLC's threads: not on the UI thread
            // (if the thread cannot start, the player is leaked rather than freed in use)
            let _ = std::thread::Builder::new().name("vlc-stop".into()).spawn(move || o.close());
        }
    }
}

fn start_options(position: f64, looped: bool) -> Vec<String> {
    let mut o = vec![format!(":start-time={:.3}", position.max(0.0)), ":no-sub-autodetect-file".to_string(), ":no-spu".to_string()];
    if looped {
        o.push(":input-repeat=65535".to_string());
    }
    o
}

/// The picture of a video, decoded by VLC into frames of `size` (letterboxed).
pub struct Video {
    player: Player,
    frames: Arc<Frames>,
    paused: bool,
    paused_done: AtomicBool,
}

impl Video {
    pub fn start(path: &Path, position: f64, looped: bool, paused: bool, size: (u32, u32), frames: Arc<Frames>) -> std::io::Result<Self> {
        let mut options = start_options(position, looped && !paused);
        options.push(":no-audio".to_string());
        let player = Player::start(path, &options, Some((size, true, frames.clone())))?;
        Ok(Self { player, frames, paused, paused_done: AtomicBool::new(false) })
    }

    /// The end of the video (or, when paused, its one frame is there).
    pub fn finished(&self) -> bool {
        if self.paused && self.frames.count.load(Ordering::SeqCst) > 0 {
            if !self.paused_done.swap(true, Ordering::SeqCst) {
                self.player.pause();
            }
            return true;
        }
        self.player.done()
    }
}

/// The sound of a video, played by VLC (no picture).
pub struct Sound {
    _player: Player,
}

impl Sound {
    pub fn start(path: &Path, position: f64, looped: bool) -> std::io::Result<Self> {
        let mut options = start_options(position, looped);
        options.push(":no-video".to_string());
        Ok(Self { _player: Player::start(path, &options, None)? })
    }
}

/// Length in seconds and a still picture (PNG, 640 wide) of a video file.
pub fn probe(path: &Path) -> Result<(f64, Option<Vec<u8>>), String> {
    if !available() {
        return Err(crate::i18n::t("video.no_ffmpeg"));
    }
    let mut duration = 0.0;
    // a frame one second in (often nicer than the first, black one); short videos: the first
    for start in [1.0, 0.0] {
        let frames = Arc::new(Frames::default());
        let options = vec![format!(":start-time={start}"), ":no-audio".to_string(), ":no-spu".to_string()];
        let player = Player::start(path, &options, Some(((640, 4096), false, frames.clone()))).map_err(|e| e.to_string())?;
        let begin = Instant::now();
        let mut frame = None;
        while begin.elapsed() < Duration::from_secs(10) {
            if let Some(d) = player.length() {
                duration = d;
            }
            if frame.is_none() {
                frame = frames.latest.lock().unwrap_or_else(|e| e.into_inner()).take();
            }
            if frame.is_some() && duration > 0.0 {
                break;
            }
            if player.done() || (frame.is_some() && begin.elapsed() > Duration::from_secs(2)) {
                break;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        drop(player);
        if let Some((_, (w, h), rgba)) = frame {
            let png = image::RgbaImage::from_raw(w, h, rgba).and_then(|img| {
                let mut out = std::io::Cursor::new(Vec::new());
                image::DynamicImage::ImageRgba8(img).write_to(&mut out, image::ImageFormat::Png).ok().map(|_| out.into_inner())
            });
            return Ok((duration, png));
        }
    }
    if duration > 0.0 { Ok((duration, None)) } else { Err(crate::i18n::t("video.not_video")) }
}
