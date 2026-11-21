//! Keeps the program starting when a graphics driver does not cooperate.
//!
//! A graphics driver can crash the whole process, or hang it, while the renderer starts;
//! then nothing appears and nothing can be caught. So before a renderer is tried its name
//! is written to `start-attempt` in the cache folder, and a frame on the screen (the
//! third, once the window is visible) removes that file and remembers the renderer in
//! `renderer`. A start that leaves `start-attempt` behind never showed a window: the next start tries the other renderer
//! first (and keeps using it while it works). A start that shows no frame within
//! [`WATCHDOG`] is restarted with the other renderer.
//! `start.log` (rewritten at every start) tells how the last start went.

use eframe::Renderer;
use std::path::PathBuf;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::Duration;

pub const WATCHDOG: Duration = Duration::from_secs(30);

/// Set once [`CONFIRM_FRAMES`] frames were shown.
static FIRST_FRAME: AtomicBool = AtomicBool::new(false);
/// Frames run so far.
static FRAMES: AtomicUsize = AtomicUsize::new(0);
/// The start counts as good on this frame: eframe paints the first frame and only then
/// makes the window visible, so the first frame alone does not prove that presenting works.
pub const CONFIRM_FRAMES: usize = 3;
/// The network output window (`canvas`) keeps its own files: it may run next to the presenter.
static CANVAS: AtomicBool = AtomicBool::new(false);
/// Set on the program the watchdog restarted (its start.log then continues the old one).
pub const RESTARTED_ENV: &str = "OH_PRESENTER_RESTARTED";
/// Number of the current renderer attempt (1, 2…), for the watchdog.
static ATTEMPT: AtomicUsize = AtomicUsize::new(0);
/// The renderer being tried.
static CURRENT: Mutex<Option<Renderer>> = Mutex::new(None);

pub fn other(r: Renderer) -> Renderer {
    match r {
        Renderer::Wgpu => Renderer::Glow,
        Renderer::Glow => Renderer::Wgpu,
    }
}

/// The renderers in the order to try them: the one whose last start never showed a
/// frame (`failed`) last, else the one that worked last time (`preferred`) first, else
/// wgpu first.
pub fn order(preferred: Option<Renderer>, failed: Option<Renderer>) -> Vec<Renderer> {
    let first = match (preferred, failed) {
        (_, Some(failed)) => other(failed),
        (Some(preferred), None) => preferred,
        (None, None) => Renderer::Wgpu,
    };
    vec![first, other(first)]
}

/// A renderer name as written in the files (`wgpu` / `glow`).
pub fn parse(s: &str) -> Option<Renderer> {
    s.trim().parse().ok()
}

/// The network output window (`canvas`) uses `canvas-start.log` etc.
pub fn set_canvas(canvas: bool) {
    CANVAS.store(canvas, Ordering::SeqCst);
}

fn file(name: &str) -> PathBuf {
    let prefix = if CANVAS.load(Ordering::SeqCst) { "canvas-" } else { "" };
    crate::storage::paths().cache.join(format!("{prefix}{name}"))
}

fn attempt_file() -> PathBuf {
    file("start-attempt")
}

fn preferred_file() -> PathBuf {
    file("renderer")
}

pub fn start_log() -> PathBuf {
    file("start.log")
}

fn read(path: &std::path::Path) -> Option<Renderer> {
    std::fs::read_to_string(path).ok().and_then(|s| parse(&s))
}

/// The renderer order for this start (see [`order`]); logs a previous start that never
/// showed a window. Starts a fresh `start.log`.
pub fn plan() -> Vec<Renderer> {
    if std::env::var_os(RESTARTED_ENV).is_none() {
        let _ = std::fs::write(start_log(), "");
    }
    let failed = read(&attempt_file());
    let preferred = read(&preferred_file());
    if let Some(f) = failed {
        crate::log_error(&format!(
            "the previous start with the {f} renderer did not show a window (crashed, hung or was ended); trying {} first",
            other(f)
        ));
    }
    let order = order(preferred, failed);
    crate::log_info(&format!("renderer order: {order:?} (last working: {preferred:?}, failed last time: {failed:?})"));
    order
}

/// Called before a renderer is tried. With `restart` the watchdog restarts the program
/// with the other renderer when this attempt shows no frame in time.
pub fn attempt(renderer: Renderer, restart: bool) {
    let n = ATTEMPT.fetch_add(1, Ordering::SeqCst) + 1;
    *CURRENT.lock().unwrap_or_else(|e| e.into_inner()) = Some(renderer);
    let _ = std::fs::write(attempt_file(), renderer.to_string());
    crate::log_info(&format!("attempt {n}: starting the {renderer} renderer"));
    let _ = std::thread::Builder::new().name("start-watchdog".into()).spawn(move || {
        std::thread::sleep(WATCHDOG);
        if FIRST_FRAME.load(Ordering::SeqCst) || ATTEMPT.load(Ordering::SeqCst) != n {
            return;
        }
        crate::log_error(&format!("no window {} s after starting the {renderer} renderer", WATCHDOG.as_secs()));
        // checked once more right before: a window that appeared meanwhile is kept
        if restart && !FIRST_FRAME.load(Ordering::SeqCst) {
            let next = other(renderer);
            let restarted = std::env::current_exe().and_then(|exe| {
                std::process::Command::new(exe)
                    .args(std::env::args_os().skip(1))
                    .env(crate::RENDERER_ENV, next.to_string())
                    .env(RESTARTED_ENV, "1")
                    .spawn()
            });
            match restarted {
                Ok(_) => {
                    crate::log_error(&format!("restarted with the {next} renderer"));
                    // `start-attempt` stays: later starts also begin with the other renderer
                    std::process::exit(3);
                }
                Err(e) => crate::log_error(&format!("could not restart: {e}")),
            }
        }
    });
}

/// Called on every frame; [`CONFIRM_FRAMES`] frames mark the start as good. `false`
/// until then (the caller asks for another frame, so the start is confirmed soon).
pub fn frame() -> bool {
    if FIRST_FRAME.load(Ordering::SeqCst) {
        return true;
    }
    if FRAMES.fetch_add(1, Ordering::SeqCst) + 1 < CONFIRM_FRAMES {
        return false;
    }
    confirm();
    true
}

fn confirm() {
    if FIRST_FRAME.swap(true, Ordering::SeqCst) {
        return;
    }
    let Some(renderer) = *CURRENT.lock().unwrap_or_else(|e| e.into_inner()) else { return };
    let _ = std::fs::write(preferred_file(), renderer.to_string());
    let _ = std::fs::remove_file(attempt_file());
    crate::log_info(&format!("the window shows (frame {CONFIRM_FRAMES}) with the {renderer} renderer"));
}
