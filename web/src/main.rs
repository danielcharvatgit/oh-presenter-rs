//! oh! Presenter for the web: songs, bible passages and texts on an output window, in the
//! browser and offline (a PWA). Everything is kept in this browser (local storage and
//! IndexedDB). Code agents control it with the WebMCP tools of the page ([`commands`]).
//!
//! The data model, the bible import and search and the song formats are the program's own
//! (`../src`), so services, songs and bibles behave the same as in the desktop program.

// outside the browser only the tests use the code
#![cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]

// the program's modules, shared (some of their functions are for the desktop only)
#[allow(dead_code)]
#[path = "../../src/bible.rs"]
mod bible;
#[allow(dead_code)]
#[path = "../../src/model.rs"]
mod model;
#[allow(dead_code)]
#[path = "../../src/song_formats.rs"]
mod song_formats;

mod app;
mod commands;
mod i18n;
mod js;
mod output;
mod render;
mod service_file;
mod shortcuts;
mod storage;

#[cfg(test)]
mod tests;
mod ui;

#[cfg(target_arch = "wasm32")]
fn main() {
    use wasm_bindgen::JsCast;
    wasm_bindgen_futures::spawn_local(async {
        let document = web_sys::window().and_then(|w| w.document()).expect("no browser document");
        let canvas = document
            .get_element_by_id("app")
            .expect("missing #app canvas")
            .dyn_into::<web_sys::HtmlCanvasElement>()
            .expect("#app must be a canvas");
        let output = js::is_output_window();
        let started = eframe::WebRunner::new()
            .start(
                canvas,
                eframe::WebOptions::default(),
                Box::new(move |cc| {
                    render::install_fonts(&cc.egui_ctx);
                    if output { Ok(Box::new(output::OutputApp::new(&cc.egui_ctx))) } else { Ok(Box::new(app::WebApp::new(&cc.egui_ctx))) }
                }),
            )
            .await;
        if let Err(e) = started {
            web_sys::console::error_1(&format!("oh! Presenter could not start: {e:?}").into());
        }
    });
}

#[cfg(not(target_arch = "wasm32"))]
fn main() {
    eprintln!("oh! Presenter web runs in the browser: build it with ./build.sh (see README.md)");
}
