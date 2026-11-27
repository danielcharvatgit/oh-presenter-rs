//! oh! Presenter (Rust edition) — a church presentation program for songs,
//! bible passages, videos and designed slides, with LAN output windows.
//!
//! Copyright (C) the oh! Presenter authors
//!
//! This program is free software: you can redistribute it and/or modify it under the
//! terms of the GNU General Public License as published by the Free Software Foundation,
//! either version 3 of the License, or (at your option) any later version.
//!
//! This program is distributed in the hope that it will be useful, but WITHOUT ANY
//! WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A
//! PARTICULAR PURPOSE. See the GNU General Public License for more details.
//!
//! You should have received a copy of the GNU General Public License along with this
//! program (the LICENSE file). If not, see <https://www.gnu.org/licenses/>.
//!
//! Usage (`oh-presenter-rs --help` lists everything, see [`cli`]):
//!   oh-presenter-rs [options]            the presenter
//!   oh-presenter-rs --cli [options]      only the output window, controlled from the command line
//!   oh-presenter-rs canvas [host[:port]] [--name NAME]
//!                                        an output window that receives slides over the network

// a GUI program: no console window next to it on Windows
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod app;
mod awake;
mod bible;
mod cli;
mod client_app;
mod clock;
mod desktop_guard;
mod formats;
mod i18n;
mod model;
mod net;
mod peek;
mod remote;
mod render;
mod screens;
mod settings;
mod shortcuts;
mod song_formats;
mod start_guard;
mod storage;
#[cfg(test)]
mod tests;
mod video;
mod vlc;

/// Forces one renderer (`wgpu` or `glow`); also set by the start watchdog when it restarts.
pub const RENDERER_ENV: &str = "OH_PRESENTER_RENDERER";

fn main() {
    install_crash_reporter();
    rotate_crash_log();
    let args: Vec<String> = std::env::args().skip(1).collect();
    let canvas = args.first().map(String::as_str) == Some("canvas");
    let options = if canvas {
        cli::Options::default()
    } else {
        let wants_console = !args.is_empty();
        if wants_console {
            cli::attach_console(args.iter().any(|a| a == "--cli"));
        }
        match cli::parse(&args) {
            Ok(cli::Parsed::Help) => {
                print!("{}", cli::help());
                return;
            }
            Ok(cli::Parsed::Version) => {
                println!("oh-presenter-rs {}", env!("CARGO_PKG_VERSION"));
                return;
            }
            Ok(cli::Parsed::Run(o)) => o,
            Err(e) => {
                eprintln!("oh-presenter-rs: {e}");
                std::process::exit(2);
            }
        }
    };
    start_guard::set_canvas(canvas);
    // a broken graphics driver (or a computer without a GPU, e.g. a VM) can make the
    // default wgpu renderer fail; then the OpenGL (glow) renderer gets a chance. A driver
    // that crashed or hung the last start makes this start try the other one first.
    let mut renderers = start_guard::plan();
    let forced = std::env::var(RENDERER_ENV).ok().and_then(|r| start_guard::parse(&r));
    if let Some(r) = forced {
        renderers = vec![r];
    }
    let mut errors = Vec::new();
    for (i, renderer) in renderers.into_iter().enumerate() {
        // only the first attempt of a start that was not forced gets restarted by the watchdog
        start_guard::attempt(renderer, forced.is_none() && i == 0);
        // a renderer can also panic while it starts (the panic is already logged)
        let result = std::panic::catch_unwind(|| run(&args, &options, renderer))
            .unwrap_or_else(|p| Err(eframe::Error::AppCreation(panic_message(&*p).into())));
        match result {
            Ok(()) => return,
            Err(e) => {
                let e = format!("{renderer} renderer: {e}");
                log_error(&e);
                errors.push(e);
            }
        }
    }
    fatal_error(&format!("oh! Presenter could not start.\n\n{}", errors.join("\n\n")));
}

fn run(args: &[String], options: &cli::Options, renderer: eframe::Renderer) -> eframe::Result {
    if args.first().map(String::as_str) == Some("canvas") {
        let mut server = None;
        let mut name = hostname();
        let mut it = args.iter().skip(1);
        while let Some(a) = it.next() {
            if a == "--name" {
                name = it.next().cloned().unwrap_or(name);
            } else {
                server = Some(a.clone());
            }
        }
        // the client uses the presenter's language setting when there is one
        let settings = settings::Settings::load();
        i18n::load_dir(&storage::paths().lang());
        i18n::set_language(&settings.language);
        let options = eframe::NativeOptions {
            viewport: eframe::egui::ViewportBuilder::default()
                .with_title(format!("oh! Presenter — {name}"))
                .with_app_id(app_id("oh-presenter-rs-canvas"))
                .with_icon(icon())
                .with_inner_size([800.0, 450.0]),
            renderer,
            wgpu_options: wgpu_options(),
            ..Default::default()
        };
        return eframe::run_native(
            "oh-presenter-canvas",
            options,
            Box::new(move |cc| {
                started();
                Ok(Box::new(client_app::ClientApp::new(cc, server, name)))
            }),
        );
    }
    // transparent windows (an overlay output) only when asked for: some drivers make every
    // window of the program a little transparent or slower with it
    let transparent = settings::Settings::load().output_transparent;
    let native = eframe::NativeOptions {
        viewport: main_viewport(options.cli, transparent),
        renderer,
        wgpu_options: wgpu_options(),
        ..Default::default()
    };
    let options = options.clone();
    eframe::run_native(
        "oh-presenter-rs",
        native,
        Box::new(move |cc| {
            started();
            Ok(Box::new(app::App::with_options(cc, options)))
        }),
    )
}

/// The program's window: the presenter, or (`cli`) the output alone. `transparent`: created
/// transparent, so a transparent output shows what is behind it (eframe makes every window
/// of the program like its first one).
pub fn main_viewport(cli: bool, transparent: bool) -> eframe::egui::ViewportBuilder {
    let viewport = if cli {
        // `--cli`: the only window is the output
        eframe::egui::ViewportBuilder::default()
            .with_title("oh! Presenter — output")
            .with_app_id(app_id("oh-presenter-rs"))
            .with_icon(icon())
            .with_inner_size([960.0, 540.0])
    } else {
        eframe::egui::ViewportBuilder::default()
            .with_title("oh! Presenter")
            // matches the .desktop file (StartupWMClass), so Linux docks show the right icon
            .with_app_id(app_id("oh-presenter-rs"))
            .with_icon(icon())
            .with_inner_size([1280.0, 800.0])
            .with_min_inner_size([800.0, 500.0])
    };
    viewport.with_transparent(transparent)
}

/// Set once a renderer works and the app is being created: from then on a panic of the
/// UI thread is a crash of the program, not a reason to try another renderer.
static STARTED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

fn started() {
    STARTED.store(true, std::sync::atomic::Ordering::SeqCst);
}

fn panic_message(p: &(dyn std::any::Any + Send)) -> String {
    p.downcast_ref::<&str>().map(|s| s.to_string()).or_else(|| p.downcast_ref::<String>().cloned()).unwrap_or_else(|| "panic".into())
}

/// Where startup errors and panics are written: the program has no console on Windows,
/// so without this a crash would leave no trace at all. In the cache folder, not in the
/// temp folder: a flatpak's /tmp is private to the sandbox and gone when the program ends.
/// (~/.cache/oh-presenter-rs, ~/.var/app/cz.oh.Presenter/cache/oh-presenter-rs,
/// %LOCALAPPDATA%\oh\oh-presenter-rs\cache)
fn crash_log() -> std::path::PathBuf {
    storage::paths().cache.join("crash.log")
}

fn append_line(path: &std::path::Path, msg: &str) {
    use std::io::Write;
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
        let _ = writeln!(f, "[unix time {now}] {msg}");
    }
}

/// An error: into crash.log (and start.log, which tells the whole story of this start).
pub fn log_error(msg: &str) {
    eprintln!("{msg}");
    append_line(&crash_log(), msg);
    append_line(&start_guard::start_log(), msg);
}

/// A step of the start, into start.log (rewritten at every start).
pub fn log_info(msg: &str) {
    eprintln!("{msg}");
    append_line(&start_guard::start_log(), msg);
}

/// crash.log only grows; over 1 MB it becomes crash.old.log (replacing an older one).
fn rotate_crash_log() {
    let log = crash_log();
    if std::fs::metadata(&log).is_ok_and(|m| m.len() > 1024 * 1024) {
        let _ = std::fs::rename(&log, log.with_file_name("crash.old.log"));
    }
}

/// Shows the error (it would be invisible on Windows otherwise) and exits.
fn fatal_error(msg: &str) -> ! {
    let text = format!("{msg}\n\nDetails: {}", crash_log().display());
    rfd::MessageDialog::new()
        .set_level(rfd::MessageLevel::Error)
        .set_title("oh! Presenter")
        .set_description(text)
        .set_buttons(rfd::MessageButtons::Ok)
        .show();
    std::process::exit(1);
}

/// Panics are logged and shown in a message box instead of silently closing the program.
fn install_crash_reporter() {
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let thread = std::thread::current().name().unwrap_or("?").to_string();
        let backtrace = std::backtrace::Backtrace::force_capture();
        log_error(&format!("panic in thread '{thread}': {info}\n{backtrace}"));
        default_hook(info);
        // only a panic of the main (UI) thread ends the program; before the app runs,
        // main() catches it and tries the next renderer
        if thread == "main" && STARTED.load(std::sync::atomic::Ordering::SeqCst) {
            fatal_error(&format!("oh! Presenter crashed:\n{info}"));
        }
    }));
}

/// The window's app id, which must match the name of the .desktop file so that docks show
/// the right icon: in a flatpak that is the flatpak id (cz.oh.Presenter).
fn app_id(default: &str) -> String {
    std::env::var("FLATPAK_ID").ok().filter(|id| !id.is_empty()).unwrap_or_else(|| default.to_string())
}

/// The window icon (the same picture as the installed launcher icon).
fn icon() -> eframe::egui::IconData {
    eframe::icon_data::from_png_bytes(include_bytes!("../assets/icon.png")).unwrap_or_default()
}

/// Presenting without waiting for vsync. With vsync every window's repaint blocks until
/// that window's screen refreshes; with the main window and the output window on two
/// screens (projector) the waits add up, and a covered window can block for much longer,
/// so the program almost froze. The UI only repaints when something changes, so this
/// does not spin. `OH_PRESENTER_VSYNC=1` switches vsync back on.
fn vsync() -> bool {
    std::env::var("OH_PRESENTER_VSYNC").is_ok_and(|v| v == "1")
}

fn wgpu_options() -> eframe::egui_wgpu::WgpuConfiguration {
    #[allow(unused_mut)]
    let mut config = eframe::egui_wgpu::WgpuConfiguration {
        surface: eframe::egui_wgpu::SurfaceConfig {
            present_mode: if vsync() { eframe::wgpu::PresentMode::AutoVsync } else { eframe::wgpu::PresentMode::AutoNoVsync },
            desired_maximum_frame_latency: Some(2),
        },
        ..Default::default()
    };
    // Windows: only Direct3D 12. With the default (Vulkan too) the Vulkan loader also loads
    // the "layers" of other programs (screen recorders, game overlays, some GPU tools),
    // which crash or hang some starts and not others, depending on what else runs.
    // OpenGL stays as the fallback renderer (glow). WGPU_BACKEND=vulkan etc. still works.
    #[cfg(windows)]
    if let eframe::egui_wgpu::WgpuSetup::CreateNew(setup) = &mut config.wgpu_setup {
        setup.instance_descriptor.backends = eframe::wgpu::Backends::from_env().unwrap_or(eframe::wgpu::Backends::DX12);
    }
    config
}

fn hostname() -> String {
    std::env::var("HOSTNAME")
        .ok()
        .or_else(|| std::fs::read_to_string("/etc/hostname").ok().map(|s| s.trim().to_string()))
        .or_else(|| std::env::var("COMPUTERNAME").ok())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "canvas".into())
}
