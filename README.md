# oh! Presenter (Rust)

A church presentation program for songs, bible passages, videos and designed slides. It shows them
in an output window on this computer and on other computers in the LAN. It is a Rust/[egui](https://github.com/emilk/egui)
rewrite of [oh-presenter](../oh-presenter), licensed under [GPL-3.0-or-later](LICENSE).

Current version: **0.1.1**

## Install

Download the file for your system from the **[Releases page](https://github.com/danielcharvatgit/oh-presenter-rs/releases)**:

| System          | File                                    | Install                                          |
| --------------- | --------------------------------------- | ------------------------------------------------ |
| Windows 10 / 11 | `oh-presenter-rs-setup-<version>.exe`   | run it (no admin rights needed)                  |
| Ubuntu / Debian | `oh-presenter-rs_<version>-1_amd64.deb` | `sudo apt install ./oh-presenter-rs_*.deb`       |
| Any Linux       | `oh-presenter-rs.flatpak`               | `flatpak install --user oh-presenter-rs.flatpak` |

**To update**, install the new version the same way over the old one. On Flatpak, add `--reinstall`.
Updating or uninstalling never deletes your songs, designs, bibles or settings.

**Videos** need ffmpeg. The Windows installer and the flatpak already include it. Elsewhere, install
`ffmpeg` from your system's packages. If ffmpeg is missing, an installed **VLC 3** is used instead. VLC also plays the videos whose format
the installed ffmpeg cannot read.

## Build it yourself

### Everything at once: `build.sh` → `dist/`

On Ubuntu / Debian, one script builds all packages and puts them in the **`dist/`** directory:

```
sh build.sh
```

| File in `dist/`                         | What it is                            |
| --------------------------------------- | ------------------------------------- |
| `oh-presenter-rs_<version>-1_amd64.deb` | Ubuntu / Debian package               |
| `oh-presenter-rs.flatpak`               | Flatpak bundle                        |
| `oh-presenter-rs.exe`                   | Windows program                       |
| `oh-presenter-rs-setup-<version>.exe`   | Windows installer (includes ffmpeg)   |

The script only builds. It never installs the app. Before the first run, set up the tools once:

```
sudo apt install build-essential mingw-w64 curl unzip wine flatpak
rustup target add x86_64-pc-windows-gnu
```

If `wine` is missing, the script skips the Windows installer. If `flatpak` is missing, it skips the flatpak.

### Run from source

You need Rust 1.95+ ([rustup.rs](https://rustup.rs)) and a C compiler. On Ubuntu / Debian, install
`build-essential`. On Windows, install the Visual Studio Build Tools.

```
cargo run --release                          # the presenter
cargo run --release -- canvas [host[:port]]  # an output window that finds the presenter on the LAN
cargo test
```

### Single packages

| Package                  | Command                                                                                                | Output                         |
| ------------------------ | ------------------------------------------------------------------------------------------------------ | ------------------------------ |
| Windows installer        | `cargo build --release`, `packaging\windows\get-ffmpeg.ps1`, `iscc packaging\windows\oh-presenter.iss` | `target\installer\`            |
| `.deb`                   | `cargo install cargo-deb && cargo deb`                                                                 | `target/debian/`               |
| Flatpak (no Rust needed) | `packaging/flatpak/build.sh [--bundle]`                                                                | installs `cz.oh.Presenter`     |
| Per-user Linux install   | `packaging/linux/install.sh [--uninstall]`                                                             | `~/.local/bin/oh-presenter-rs` |

The Windows installer needs [Inno Setup 6](https://jrsoftware.org/isinfo.php).

## Features

* **Service**: a running order of songs, bible passages, designs, slides and videos, with live view, a blank screen and
  **freeze** (`,`: the output keeps its picture while you work in the program).
* **Songs**: a library and an editor with verse, chorus and bridge marks, verse order and extra text columns (for example, other languages).
* **Slide designer**: text, shapes and images (with adjustable opacity; transparent PNG/WebP stay transparent) on a
  1280×720 slide, with undo and redo.
* **Bible**: search by reference (`J 3,16-18`, `John 3:16`) or by words. No bible text is included, so import one.
* **Output window**: opens full screen on a second screen (projector), or press F5 to open it as a normal window.
  Leaving full screen on the projector from the output window itself (F11, Esc, double click) asks first.
* **Network outputs**: off by default. They use TCP/UDP port 50200.
* **Alerts** (F7) and an optional **clock** on the output, encoded in coloured dots (see below).
* **Import / export**: `.ohsvc`, OpenLP, OpenLyrics, ChordPro, plain text, Zefania / OSIS / OpenSong / plain-text bibles
  (a bible can also be exported as `.txt`), `.ohbackup` backups and files of the old oh-presenter.
* **Languages**: Czech and English. To add another, use Settings → *Create translation template*.
* **Keyboard**: every shortcut can be changed in Settings. Alt+1 / Alt+2 open the File / Output menu. Tab and
  Shift+Tab start at the open tab. In the Service tab every key works wherever you clicked (except in a text field):
  `,` freezes the output (it keeps its picture while you change slides), and on a video Space plays / pauses it and
  ← / → move it 5 seconds. Ctrl+Shift+← → moves between the columns and ← → move a focused column border
  (Shift: bigger steps).
* **Output on a second screen**: leaving full screen or closing the output there (F11, F5, Esc, a double click) is confirmed in the program window first, because you do not see the projector.

## Data locations

| System | Library | Settings | Cache (`start.log`, `crash.log`) |
| --- | --- | --- | --- |
| Linux | `~/.local/share/oh-presenter-rs/` | `~/.config/oh-presenter-rs/` | `~/.cache/oh-presenter-rs/` |
| Flatpak | `~/.var/app/cz.oh.Presenter/data/…` | `~/.var/app/cz.oh.Presenter/config/…` | `~/.var/app/cz.oh.Presenter/cache/…` |
| Windows | `%APPDATA%\oh\oh-presenter-rs\data\` | `%APPDATA%\oh\oh-presenter-rs\config\` | `%LOCALAPPDATA%\oh\oh-presenter-rs\cache\` |

* Settings → *Data* shows the folders in use.
* Set `OH_PRESENTER_HOME=/some/dir` to keep everything in one folder (portable use).
* To move your library to another computer or install, use File → Export → library backup (`.ohbackup`).

## If it does not start

* If the graphics renderer crashes, the next start switches to the other one (wgpu ⇄ OpenGL) automatically.
* `start.log` in the cache folder describes the last start step by step. `crash.log` collects errors.
* `OH_PRESENTER_RENDERER=glow` (or `wgpu`) forces a renderer.

## Reading the encoded clock

Each dot is red (0), green (1), blue (2) or brown (3). Read the dots from left to right:

| Dot | Means | red | green | blue | brown |
| --- | --- | --- | --- | --- | --- |
| 1st | hours | 12–3 | 3–6 | 6–9 | 9–12 |
| 2nd | + | 0:00 | 0:45 | 1:30 | 2:15 |
| 3rd | + minutes | 0 | 11 | 22 | 34 |

For example, green, blue, red = 3:00 + 1:30 = **4:30** (until 4:41).

## License

GNU General Public License v3.0 or later, see [LICENSE](LICENSE). The Windows installer and the
flatpak include ffmpeg, which is also GPL licensed (<https://ffmpeg.org/legal.html>).
