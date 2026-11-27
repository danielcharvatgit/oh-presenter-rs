# oh! Presenter web

The web edition of oh! Presenter: songs, bible passages, texts and pictures on a projector,
in the browser. It is a separate program (Rust + WebAssembly, an offline PWA) that shares the
desktop program's data model, bible import and search and song formats (`../src/model.rs`,
`../src/bible.rs`, `../src/song_formats.rs`), so services, songs and bibles behave the same
and `.ohsvc` service files open in both.

* **Service**: songs, bible passages, texts and pictures; slides as thumbnails, the live slide,
  live mode, blank, freeze, alerts running across the output.
* **Output window**: *Output window* (or W) opens the projector output in a second browser
  window. Move it to the projector and double click it (or press F) for full screen. The slide
  keys work there too.
* **Songs**: a library with an editor (parts, order), import of OpenLyrics, ChordPro and plain
  text, export as OpenLyrics, pasting whole lyrics.
* **Bible**: import Zefania, OSIS, OpenSong XML or plain-text bibles; search by reference
  (`John 3:16-18`, `Jan 3,16`) or by words. OpenLP `.sqlite` bibles import in the desktop
  program only.
* **Settings**: Czech / English, theme, text and additional info (the bible reference and the
  song / text title are separate options), alerts, keyboard shortcuts.
* **Data stays in the browser**: settings, songs and the service in local storage, bibles and
  pictures in IndexedDB (persistent storage is requested). *Save as file* downloads the service
  as `.ohsvc`; *Open…* opens one (also one saved by the desktop program).
* **Offline**: after the first visit the service worker serves the whole program from its cache.
  A new build is used from the next page load.

## Keyboard shortcuts

The desktop program's keys that a browser or the system takes are one letter here; no key uses
Ctrl or Alt. They work when no text field is being typed into, and can be changed in Settings ›
Keyboard shortcuts.

| Action | Key | Desktop program |
| ------ | --- | --------------- |
| Next / previous slide | ↓ PgDn → / ↑ PgUp ← | the same |
| First / last slide | Home / End | the same |
| Next / previous item | N / Shift+N | Ctrl+↓ / Ctrl+↑ |
| Go live (Bible tab: the found verses) | Enter, G | Enter |
| Live mode | L | L |
| Blank / freeze | . / , | . / , |
| Output window | W | F5 |
| Output full screen (in the output window) | F, double click | F11 |
| Alert | T | F7 |
| Search | S | Ctrl+F |
| Add to service | A | Ctrl+Enter |
| Rename / remove item | F2 / Delete | F2 |
| Tabs Service, Songs, Bible, Settings | Shift+1 … Shift+4 | Ctrl+1 … Ctrl+5 |
| Song parts / verse N (Service tab) | V C P B I E O / 1–9 | the same |

## Code agents (WebMCP)

There is no command line in the browser: the page itself offers its commands as tools, named
after the buttons like the desktop program's MCP tools (`next-slide`, `add-song`, `bible`,
`add-to-service`, `set-setting`, …). In browsers with WebMCP (`navigator.modelContext`) they
are registered there; scripts and MCP bridges that drive the page can call
`await window.ohPresenter.call("bible", {action: "show", query: "John 3:16"})` and read
`window.ohPresenter.tools` (name, description, JSON schema of each).

## Build

```sh
rustup target add wasm32-unknown-unknown
cargo install wasm-bindgen-cli --version 0.2.130   # the version in Cargo.toml
./build.sh                                          # writes the PWA to dist/
cargo test                                          # the tests run outside the browser
```

`wasm-opt` (binaryen) makes the download smaller when it is installed.

## Serve

`dist/` is static: any web server works (the output window and the service worker need
`http://localhost` or HTTPS).

* **The included server** (Axum, `server/`): serves `dist/` and `/api/health`.
  `cd server && cargo run --release` — `OH_PRESENTER_WEB_DIST` sets the directory (default
  `../dist`), `OH_PRESENTER_WEB_ADDR` the address (default `127.0.0.1:8080`).
* **nginx**: `nginx.conf` (serves `dist/`, `/api/` to the Axum server).
* **Apache 2**: `apache2.conf` (the same; needs `mod_proxy_http` for `/api/`).
