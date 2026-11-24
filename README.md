# oh! Presenter (Rust)

A church presentation program for songs, bible passages, videos and designed slides. It shows them
in an output window on this computer and on other computers in the LAN. It is a Rust/[egui](https://github.com/emilk/egui)
rewrite of [oh-presenter](../oh-presenter), licensed under [GPL-3.0-or-later](LICENSE).

Current version: **0.2.0**

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
* **Pictures as an overlay**: a picture with transparent parts added to the service (➕ Image) gets a transparent
  background. With Settings → *Advanced settings* → *Transparent output* (and *keep above all other windows*), the
  output window shows what is behind it there, for example a trailer played full screen in another video player.
* **Bible**: search by reference (`J 3,16-18`, `John 3:16`) or by words. No bible text is included, so import one.
* **Output window**: opens full screen on a second screen (projector), or press F5 to open it as a normal window.
  Leaving full screen on the projector from the output window itself (F11, Esc, double click) asks first; Esc
  answers *Cancel* there too, even when the output window has the keyboard.
* **Desktop kept off the projector** (Settings → *Advanced settings*, on by default): while the output is full screen
  on another screen, the Super key does not open the overview / menu on GNOME / Ubuntu, Cinnamon and KDE Plasma
  (their setting is put back afterwards, also after a crash), and Win+Tab, Win+D, Win+M and Win+, are blocked on
  Windows. On every desktop (also Hyprland, X11 and Wayland) the output goes back to full screen when a key of the
  window manager takes it out, and the program says so.
* **Network outputs**: off by default. They use TCP/UDP port 50200.
* **Alerts** (F7) and an optional **clock** on the output, encoded in coloured dots (see below).
* **Import / export**: `.ohsvc`, OpenLP, OpenLyrics, ChordPro, plain text, Zefania / OSIS / OpenSong / plain-text bibles
  (a bible can also be exported as `.txt`), `.ohbackup` backups and files of the old oh-presenter.
* **Languages**: Czech and English. To add another, use Settings → *Create translation template*.
* **Keyboard**: every shortcut can be changed in Settings → *Keyboard shortcuts*. Alt+1 / Alt+2 open the File /
  Output menu. Tab moves the keyboard focus forward and Shift+Tab back, both starting at the open tab. In the Service
  tab every key works wherever you clicked (except in a text field):
  `,` freezes the output (it keeps its picture while you change slides, also on Windows, where it was F8 before 0.2;
  an old F8 setting is changed to `,`), F2 renames the selected item, and Space plays / pauses the live video (or
  shows the selected video live) — also with the focus on the item list; Space never moves to another slide or item.
  ← / → move a video 5 seconds once you clicked it, its preview or the live panel. Ctrl+Shift+← → moves between the
  columns and ← → move a focused column border (Shift: bigger steps).
* **Output on a second screen**: leaving full screen or closing the output there (F11, F5, Esc, a double click) is confirmed in the program window first, because you do not see the projector.

## Command line (`--cli`)

`oh-presenter-rs --help` lists every option and command. `--cli` opens only the presenting output window (full screen,
on the second screen when there is one) and reads commands from the command line. The options and commands are named
after the buttons of the program:

```sh
oh-presenter-rs --cli --service sunday.ohsvc --service-item video.mp4 --playback-control=play-loop
```

| Option | What it does |
| ------ | ------------ |
| `--service FILE` | Open service… |
| `--service-item ITEM` | select the item (number from 1, title, or video file name) and *Go live* |
| `--slide N` | the slide of that item |
| `--playback-control ACTION` | video: `play`, `pause`, `play-pause`, `play-loop`, `play-once`, `restart`, `seek`, `forward`, `back`, `mute`, `unmute` |
| `--live-mode on\|off` | Live mode |
| `--windowed` | the output as a window instead of full screen |
| `--command "COMMAND"` | run a command after the start (repeatable) |
| `--mcp`, `--mcp-port PORT` | start the MCP server (below) |

Typed commands (one per line, `help` lists them all, `quit` ends): `next-slide`, `prev-slide`, `select-item 3`,
`select-slide 2`, `go-live`, `blank on`, `freeze`, `playback-control pause`, `playback-control seek seconds=90`,
`alert The owner of car 1AB 2345 is asked to come out`, `output windowed`, `bible search John 3:16`, `bible show`,
`list-items`, `status`, `set-setting language "cs"`, … On Windows, start it from a command prompt with
`start /wait oh-presenter-rs --cli …` so the prompt does not read the same keys.

## MCP server (code agents)

Settings → *Advanced settings* → *MCP server* (off by default) lets code agents on this computer control the program
like its operator: every command of the command line is an MCP tool (all of the program except the slide designer).
It listens on `http://127.0.0.1:7779/mcp` (the port can be changed) and refuses connections from web pages and other
computers. For example, with Claude Code:

```sh
claude mcp add --transport http oh-presenter http://127.0.0.1:7779/mcp
```

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
