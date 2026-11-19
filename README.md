# oh! Presenter (Rust)

A church presentation program: songs, bible passages, videos and designed slides, shown in an
output window on this computer and on other computers in the LAN. A Rust/[egui](https://github.com/emilk/egui)
rewrite of [oh-presenter](../oh-presenter). Licensed under [GPL-3.0-or-later](LICENSE).

## Download and install

Get the file for your system from the **[Releases page](https://github.com/danielcharvatgit/oh-presenter-rs/releases)**:

| System          | File                                    | Install                                            |
| --------------- | --------------------------------------- | -------------------------------------------------- |
| Windows 10 / 11 | `oh-presenter-rs-setup-<version>.exe`   | run it (per-user, no admin rights needed)          |
| Ubuntu / Debian | `oh-presenter-rs_<version>-1_amd64.deb` | `sudo apt install ./oh-presenter-rs_*.deb`         |
| Any Linux       | `oh-presenter-rs.flatpak`               | `flatpak install --user oh-presenter-rs.flatpak`   |

Videos need **ffmpeg** and **ffplay**; the Windows installer and the flatpak include them,
elsewhere install ffmpeg from your system's packages (`brew install ffmpeg` on macOS).

## Build

Needs Rust 1.95+ ([rustup.rs](https://rustup.rs)) and a C compiler (bundled SQLite).

```
cargo run --release                          # the presenter
cargo run --release -- canvas [host[:port]]  # an output window; finds the presenter on the LAN
cargo test
```

| Package               | Command                                                                     | Output                                          |
| --------------------- | --------------------------------------------------------------------------- | ----------------------------------------------- |
| Windows installer     | `cargo build --release`, `packaging\windows\get-ffmpeg.ps1`, `iscc packaging\windows\oh-presenter.iss` | `target\installer\oh-presenter-rs-setup-<version>.exe` |
| `.deb`                | `cargo install cargo-deb && cargo deb`                                      | `target/debian/`                                |
| Flatpak (no Rust needed) | `packaging/flatpak/build.sh [--bundle]`                                  | installs `cz.oh.Presenter`; `--bundle` also writes `oh-presenter-rs.flatpak` |
| Per-user Linux install | `packaging/linux/install.sh [--uninstall]`                                 | `~/.local/bin/oh-presenter-rs`                  |

Windows builds need the MSVC toolchain and [Inno Setup 6](https://jrsoftware.org/isinfo.php).
The flatpak manifest builds ffmpeg from source; after changing `Cargo.lock`, `build.sh`
regenerates `cargo-sources.json`.

## Features

* **Service** – running order of songs, bible passages, designs, text/image slides and videos, with live view and blank screen.
* **Songs** – library and editor with verse/chorus/bridge marks, verse order, extra text columns (e.g. other languages).
* **Slide designer** – text, shapes and images on a 1280×720 slide, with undo/redo; designs can be backgrounds.
* **Bible** – search by reference (`J 3,16-18`, `John 3:16`) or by words, browse, pin passages. No bible text is included; import one.
* **Alerts** (F7) – a message running across the output.
* **Network outputs** – off by default; TCP/UDP port 50200, clients find the presenter automatically.
* **Import / export** – `.ohsvc`, OpenLP `.osz` and `songs.sqlite`, OpenLyrics, ChordPro, plain text,
  Zefania / OSIS / OpenSong bibles, `.ohbackup` library backups and files of the old oh-presenter.
* **Safe saving** – songs and designs are separate files, writes are atomic, replaced files are backed up.
* **Languages** – Czech and English; add more by copying `lang/en.lang` (Settings → *Create translation template*).
* **Keyboard** – all shortcuts are customisable in Settings (arrows/PgUp/PgDn/Space to step slides, `.` blank,
  F5/F11 output window, V/C/B… jump to song parts, Ctrl+1…5 tabs, hold Alt + letters for menus).

## Data locations

* Linux: `~/.local/share/oh-presenter-rs` (library), `~/.config/oh-presenter-rs` (settings); flatpak: `~/.var/app/cz.oh.Presenter/`.
* Windows: `%APPDATA%\oh\oh-presenter-rs\`.
* `OH_PRESENTER_HOME=/some/dir` keeps everything in one folder (portable use).

## License

GNU General Public License v3.0 or later, see [LICENSE](LICENSE). The Windows installer and the
flatpak include ffmpeg, also GPL licensed (<https://ffmpeg.org/legal.html>).
