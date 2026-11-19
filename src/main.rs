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
//! Usage:
//!   oh-presenter-rs                      the presenter
//!   oh-presenter-rs canvas [host[:port]] [--name NAME]
//!                                        an output window that receives slides over the network

// a GUI program: no console window next to it on Windows
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod app;
mod bible;
mod client_app;
mod formats;
mod i18n;
mod model;
mod net;
mod render;
mod settings;
mod shortcuts;
mod storage;
mod video;
#[cfg(test)]
mod tests;

fn main() -> eframe::Result {
    let args: Vec<String> = std::env::args().skip(1).collect();
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
            wgpu_options: wgpu_options(),
            ..Default::default()
        };
        return eframe::run_native(
            "oh-presenter-canvas",
            options,
            Box::new(move |cc| Ok(Box::new(client_app::ClientApp::new(cc, server, name)))),
        );
    }
    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_title("oh! Presenter")
            // matches the .desktop file (StartupWMClass), so Linux docks show the right icon
            .with_app_id(app_id("oh-presenter-rs"))
            .with_icon(icon())
            .with_inner_size([1280.0, 800.0])
            .with_min_inner_size([800.0, 500.0]),
        wgpu_options: wgpu_options(),
        ..Default::default()
    };
    eframe::run_native("oh-presenter-rs", options, Box::new(|cc| Ok(Box::new(app::App::new(cc)))))
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
fn wgpu_options() -> eframe::egui_wgpu::WgpuConfiguration {
    let vsync = std::env::var("OH_PRESENTER_VSYNC").is_ok_and(|v| v == "1");
    eframe::egui_wgpu::WgpuConfiguration {
        surface: eframe::egui_wgpu::SurfaceConfig {
            present_mode: if vsync { eframe::wgpu::PresentMode::AutoVsync } else { eframe::wgpu::PresentMode::AutoNoVsync },
            desired_maximum_frame_latency: Some(2),
        },
        ..Default::default()
    }
}

fn hostname() -> String {
    std::env::var("HOSTNAME")
        .ok()
        .or_else(|| std::fs::read_to_string("/etc/hostname").ok().map(|s| s.trim().to_string()))
        .or_else(|| std::env::var("COMPUTERNAME").ok())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "canvas".into())
}
