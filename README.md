# oh! Presenter (Rust)

A church presentation program. It projects songs, bible passages, videos and designed slides
in an output window on this computer and in output windows on other computers in the LAN.
It is a rewrite of [oh-presenter](../oh-presenter) (Electron/JavaScript) in Rust with
[egui](https://github.com/emilk/egui).

Free software under the [GNU General Public License v3.0 or later](LICENSE).

## Download

Ready-made files are attached to each release on the
**[Releases page](https://github.com/danielcharvatgit/oh-presenter-rs/releases)**. Under
*Assets*, download the file for your system:

| Operating system          | Installation method          | File on the Releases page              | Where your own build writes it                         |
| ------------------------- | ---------------------------- | -------------------------------------- | ------------------------------------------------------ |
| Windows 10 / 11 (64-bit)  | Installer                    | `oh-presenter-rs-setup-<version>.exe`  | `target\installer\oh-presenter-rs-setup-<version>.exe` |
| Ubuntu / Debian (64-bit)  | `.deb` package               | `oh-presenter-rs_<version>-1_amd64.deb` | `target/debian/oh-presenter-rs_<version>-1_amd64.deb`  |
| Any Linux distribution    | Flatpak bundle               | `oh-presenter-rs.flatpak`              | `oh-presenter-rs.flatpak` (in the project folder)      |
| Any Linux distribution    | Install script, for one user | — (built from source)                  | `~/.local/bin/oh-presenter-rs`                         |
| macOS / others            | Plain program                | — (built from source)                  | `target/release/oh-presenter-rs`                       |

Each section below explains how to install that file and how to build it yourself.

### Common to all builds

* Rust 1.95 or newer ([rustup.rs](https://rustup.rs)). The flatpak build doesn't need it.
* The source: `git clone https://github.com/danielcharvatgit/oh-presenter-rs.git && cd oh-presenter-rs`
* Try it without installing:

  ```
  cargo run --release                      # the presenter
  cargo run --release -- canvas            # an output window; finds the presenter on the LAN
  cargo run --release -- canvas 192.168.1.10[:50200] --name "Projector"
  ```

* Videos are played with **ffmpeg** and their sound with **ffplay** (see [Videos](#videos)).
  ffmpeg also converts HEIC / HEIF and AVIF pictures.
  The Windows installer and the flatpak include both. For the other methods, install them
  from the system's packages.

---

## Windows 10 / 11

### Install

1. Download `oh-presenter-rs-setup-<version>.exe` from the Releases page.
2. Run it. It installs for the current user by default, so it needs no administrator rights.
3. Start *oh! Presenter* from the Start menu.

The installer adds Start menu entries (the presenter and *output window (network)*), an
optional desktop icon and an uninstaller. When network outputs are switched on, Windows asks
once to allow the program through the firewall.

### Build it yourself

Needs Rust with the MSVC toolchain, Visual Studio Build Tools ("Desktop development with C++")
and [Inno Setup 6](https://jrsoftware.org/isinfo.php) (`iscc`).

```
cargo build --release                                                  # -> target\release\oh-presenter-rs.exe
powershell -ExecutionPolicy Bypass -File packaging\windows\get-ffmpeg.ps1   # optional: video support
iscc packaging\windows\oh-presenter.iss                                # -> target\installer\oh-presenter-rs-setup-<version>.exe
```

`get-ffmpeg.ps1` downloads `ffmpeg.exe` and `ffplay.exe` (the GPL "essentials" build from
gyan.dev) into `packaging\windows\ffmpeg\`. The installer includes them when that folder exists.
Without them, the installer is built without video playback. The `.exe` carries the program
icon, and the release build opens no console window.

---

## Ubuntu / Debian — `.deb` package

### Install

```
sudo apt install ./oh-presenter-rs_<version>-1_amd64.deb
```

`apt` also installs `ffmpeg` (recommended by the package) for videos. Uninstall with
`sudo apt remove oh-presenter-rs`.

### Build it yourself

```
sudo apt install build-essential         # a C compiler, for the bundled SQLite and zstd
cargo install cargo-deb
cargo deb                                # -> target/debian/oh-presenter-rs_<version>-1_amd64.deb
```

---

## Any Linux — Flatpak

### Install

```
flatpak install --user oh-presenter-rs.flatpak   # needs the Flathub remote for the runtime
flatpak run cz.oh.Presenter                      # or start it from the applications menu
flatpak run cz.oh.Presenter canvas               # an output window, like `oh-presenter-rs canvas`
```

The flatpak includes ffmpeg and ffplay. It may use the network (network outputs), the home
folder and the sound server. Its library and settings are in `~/.var/app/cz.oh.Presenter/`.

### Build it yourself

Built with [org.flatpak.Builder](https://flathub.org/apps/org.flatpak.Builder); `build.sh` installs it
if it is missing. No Rust is needed on the computer.

```
packaging/flatpak/build.sh               # builds and installs cz.oh.Presenter for the current user
packaging/flatpak/build.sh --bundle      # also writes oh-presenter-rs.flatpak (in the project folder)
```

The manifest is `packaging/flatpak/cz.oh.Presenter.yml` (runtime `org.freedesktop.Platform`
26.08; ffmpeg is built from source as a module). The build runs without network, so the
crates are listed in `cargo-sources.json`. `packaging/flatpak/generate-cargo-sources.py`
remakes it after `Cargo.lock` changes (`build.sh` does that by itself). Intermediate build
files go to `~/.cache/oh-presenter-flatpak` (change it with `FLATPAK_WORK`).

---

## Any Linux — install script (current user, from source)

No package and no root. It puts the program into `~/.local/bin` and the launcher with its
icon into the applications menu.

```
sudo apt install build-essential ffmpeg  # Ubuntu / Debian; elsewhere a C compiler and ffmpeg
packaging/linux/install.sh               # builds the release version first if needed
packaging/linux/install.sh --uninstall   # songs, designs and settings are kept
```

---

## macOS and other systems

No package is made. Build and run the plain program:

```
cargo build --release                    # -> target/release/oh-presenter-rs
```

For videos, install ffmpeg (e.g. `brew install ffmpeg`, which includes ffplay).

---

## What it does

| Tab                       |                                                                                                                                                                                                                                                                                                                               |
| ------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| **Service** (Shromáždění) | The running order: songs, bible passages, designs, text and image slides, videos. Item list with a small preview of each item, slide grid, live view, blank screen, output window.                                                                                                                                                    |
| **Songs** (Písně)         | Song library and editor: parts marked as verse / chorus (refrén) / bridge / … or custom marks, verse order, `---` splits a part into more slides. **Columns**: add text columns (e.g. other languages); each part gets one text per column, and the columns marked *on slides* are shown side by side or one below the other. |
| **Slide designer**        | Text, rectangles, ellipses and images on a 1280×720 slide; drag, resize, colors, z-order, undo/redo. Designs can also serve as the background of text slides.                                                                                                                                                                 |
| **Bible**                 | Search by reference (`J 3,16-18`, `Jan 3:16`, `1 Kor 13`, `Ž 23,1.3.5`, `John 3:16,18; 4:1`) or by words (case and diacritics insensitive), browse by book/chapter, show live without adding to the service, pin passages (with a note) and return to visited ones.                                                           |
| **Settings**              | Language, look of text slides, output window always on top, network outputs and connected clients, slide marks and their keys, all keyboard shortcuts, about oh! Presenter.                                                                                                                                                   |

### Saving never overwrites by accident

* Separate buttons: **Save song**, **Save design** (only that item, into the library) and
  **Save service** / **Save service as…** (the whole service, into its file).
* Every song and design is its own file named by an id, so saving one can never replace
  another one — even with the same title (you get a warning about the duplicate name).
* **Save as new song / design** keeps the original.
* Choosing an existing file in a save dialog asks: *Save as "name (2)"*, *Replace* or *Cancel*.
* If the service file was changed by someone else since it was opened, saving asks first.
* A service imported from another format (OpenLP `.osz`, old `.ohpres`) is never written back
  into that file; it is saved as a new `.ohsvc`.
* Every replaced or deleted file is first copied to the backups folder (20 newest per name),
  and files are written atomically (temp file + rename).
* Opening another service or switching songs/designs with unsaved changes asks.
* Quitting (Ctrl+Q, the menu or closing the window) always asks; with unsaved changes it offers to save them.

### Languages

Czech and English are built in (`lang/cs.lang`, `lang/en.lang`). To add a language, copy
`en.lang` (Settings → *Create translation template*) to `<code>.lang`, e.g. `sk.lang`, into
the `lang` folder shown in Settings, translate the values and restart. Missing keys fall back
to English. A file with the same code as a built-in language overrides its texts.

### Keyboard (all customisable in Settings)

|                                 |                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                |
| ------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| Ctrl+N / Ctrl+O                 | new / open service                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                             |
| Ctrl+S                          | save — the service, the song or the design, depending on the tab                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                               |
| Ctrl+Shift+S                    | save service as… / save song or design as new                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                  |
| ↓ PgDn → Space / ↑ PgUp ←       | next / previous slide (continues into the next item)                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                           |
| Home / End                      | first / last slide                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                             |
| Ctrl+↓ / Ctrl+↑                 | next / previous service item                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                   |
| Enter                           | show the selected slide live                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                   |
| L                               | live mode on/off (on: selecting a slide shows it immediately)                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                  |
| .                               | blank screen                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                   |
| F5 / F11                        | output window / output full screen (double click and Esc in the output window too)                                                                                                                                                                                                                                                                                                                                                                                                                                                                                             |
| F7                              | alert: a message running across the output (opens with the default message; Enter sends)                                                                                                                                                                                                                                                                                                                                                                                                                                                                                       |
| V C P B I E O                   | jump to verse / chorus / pre-chorus / bridge / intro / ending / other of the current song (again = next one)                                                                                                                                                                                                                                                                                                                                                                                                                                                                   |
| 1 – 9                           | jump to verse 1 – 9                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                            |
| Ctrl+F                          | search (songs, bible, designs)                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                 |
| Ctrl+1 … Ctrl+5                 | switch tabs                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                    |
| Ctrl+D                          | pin the bible passage                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                          |
| Ctrl+Z / Ctrl+Y                 | undo / redo in the designer                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                    |
| Ctrl+Q                          | quit (asks first)                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                              |
| hold Alt + letters              | choose a menu or tab by the beginning of its name, chosen when Alt is released: `Alt+s` Service, `Alt+f` File, `Alt+set` Settings; repeating the letter cycles through the items starting with it, `Alt+sss` = Service › Songs › Slide designer; a number first picks too, `Alt+3s` = the 3rd item starting with "s", and a number alone picks the n-th item of the bar (`Alt+1` File, `Alt+3` Service). Holding Alt a moment lists the items; Esc cancels. An open menu closes when a tab is chosen this way or any other shortcut is used (`Alt+1` then `Alt+3` or `Ctrl+1`) |
| Tab / Shift+Tab                 | move between buttons and fields; the focused one gets a clear frame (until the mouse is clicked)                                                                                                                                                                                                                                                                                                                                                                                                                                                                               |
| ↑ ↓ PgUp PgDn Home End          | in the Songs and Settings tabs: scroll the page (in Service and Bible they step through slides, in the designer they move objects)                                                                                                                                                                                                                                                                                                                                                                                                                                             |
| Ctrl+Plus / Ctrl+Minus / Ctrl+0 | zoom the window in / out / back to 100 % (the status bar shows the zoom when it is not 100 %)                                                                                                                                                                                                                                                                                                                                                                                                                                                                                  |

Single-letter keys never fire while typing in a text field. Digit shortcuts use the key position, so
`Ctrl+1` and `1` – `9` work on layouts whose digit row types other characters (Czech `+ ě š č ř ž ý á í é`);
on the Czech layout zoom in with the numeric keypad `+`. Slide marks are configurable:
add your own (e.g. `T` = "Tag" with key `T`), rename them, change their keys.

### Import / export (File menu)

| Format                                                                                                | Import                     | Export                             |
| ----------------------------------------------------------------------------------------------------- | -------------------------- | ---------------------------------- |
| `.ohsvc` service (zip: `service.json` + images)                                                       | ✓                          | ✓                                  |
| OpenLP service `.osz` (songs, custom slides, bibles, images)                                          | ✓                          | ✓ (designs are skipped)            |
| OpenLyrics XML songs (OpenLP, OpenSong, …)                                                            | ✓                          | ✓ one song / all songs to a folder |
| OpenLP `songs.sqlite` database                                                                        | ✓                          |                                    |
| ChordPro (`.cho`, `.chordpro`, `.pro`) and plain text songs                                           | ✓                          |                                    |
| Bibles: Zefania XML, OSIS XML, OpenSong XML, OpenLP bible `.sqlite`                                   | ✓                          |                                    |
| `.ohdesign` slide design                                                                              | ✓                          | ✓                                  |
| Pictures as slides or in designs: PNG, JPEG, GIF, BMP, WebP, TIFF, ICO, TGA, PNM, QOI, DDS, HDR, OpenEXR, farbfeld; HEIC / HEIF (phone photos) and AVIF are converted to PNG with ffmpeg | ✓ | |
| Videos (mp4, mkv, webm, mov, avi, … — whatever ffmpeg plays)                                        | ✓                          |                                    |
| `.ohbackup` — whole library (songs, designs, images, bibles, pins)                                    | ✓ (merges, never replaces) | ✓                                  |
| old oh-presenter `.ohpres`, `.ohdata`, `.ohcanvas` (File → Import → *Files of the old oh! Presenter*) | ✓                          |                                    |

On the first start the presenter searches the home folder (in the background, skipping hidden
and build folders) for files of the old oh-presenter. If it finds any, it offers to import them:
songs and designs go into the library (identical ones are skipped), and each `.ohpres` presentation is also
converted into a new `.ohsvc` next to the original. Nothing is changed or deleted. *Later* asks again
on the next start, and *Don't ask again* stops the offer (it stays available in the File → Import menu).

Song columns are exported to OpenLyrics as `<verse name="v1" lang="…">` elements, one per column,
and read back the same way.

Plain-text songs: parts are separated by empty lines; a part starting with `R:`, `Ref.:`,
`Refrén`, `Chorus`, `Bridge:`, `1.`, `2)` … gets the matching mark.

Word search uses a search index per bible, made once in the background. The first word search
asks for which language the index should be made (only bibles in that language get one); other
languages can be switched on or off in Settings → Bible, where each bible's language can also be
corrected (it is read from the file or guessed from the text). Reference search never needs an index.
A query without a number (`Jan`, `jak`) is a word search; when it is also a book name, opening the
book is offered.

No bible text is included (licensing) — import one, e.g. a Zefania XML file of the Bible kralická.

### Alerts

F7 (or Output → Alert…) opens a small window with the default message; **Send message** runs it
across the output — over whatever is shown, also on the network outputs — and the status bar shows
it with a **Stop** button. Settings → Alerts sets the default message, text size, text color, font
(sans, serif, monospace), the color of the strip under the text (it can be transparent), position
(top, middle, bottom), direction (right to left or left to right), speed and timeout (0 = until
stopped; an alert always runs across at least once), with a live preview and a test button.

### Videos

**+ Video** in the Service tab (or File → Import → *Videos…*) adds video files (mp4, mkv, webm,
mov, avi, …) to the service. They are copied into the library (by content, like images), and a
still picture and the length are taken for the previews. The item has **Loop** and **Without sound**
switches. Live, the video plays in the output window, the live preview and on network outputs,
and the live panel shows play / pause, back to the start and a position slider. Blanking the
screen pauses the video.

Playback uses the **ffmpeg** program (it decodes the video into the window) and the sound uses
**ffplay**, so every format the installed ffmpeg knows works, and the build needs no extra
libraries. They are found next to the program (the Windows installer puts them there), in
`PATH`, or where `OH_PRESENTER_FFMPEG` / `OH_PRESENTER_FFPLAY` point. The sound plays on the
presenter computer only. Network outputs show the picture: the video file is sent to them once,
in pieces, and they need their own ffmpeg. A `.ohsvc` service file carries its videos.

### Network outputs

Network outputs are **off by default** (Settings → Network outputs → *Allow outputs on other computers*);
only then does the presenter listen on the network.

The presenter listens on TCP port 50200 (configurable) and answers UDP broadcasts on the
same port, so `oh-presenter-rs canvas` on another computer finds it automatically, reconnects
when the connection drops and shows an indicator while searching. Slides are sent as JSON
lines; images are sent once per client and cached.

## Data locations

Linux: `~/.local/share/oh-presenter-rs` (library), `~/.config/oh-presenter-rs` (settings,
`lang`); in the flatpak the same under `~/.var/app/cz.oh.Presenter/`. Windows: `%APPDATA%\oh\oh-presenter-rs\data` (library) and
`%APPDATA%\oh\oh-presenter-rs\config` (settings, `lang`). Set `OH_PRESENTER_HOME=/some/dir` to keep everything in one folder (portable use).

## Development

```
cargo test     # unit tests, OpenLP sample files (../OpenLP-3.1.7), headless UI and network tests
```

| Module                                   |                                                                                         |
| ---------------------------------------- | --------------------------------------------------------------------------------------- |
| `model.rs`                               | songs, designs, service, scenes                                                         |
| `render.rs`                              | drawing scenes (shared by previews, output window, network clients)                     |
| `storage.rs`                             | data folders, library, media store, safe writing                                        |
| `formats.rs`                             | all import / export formats                                                             |
| `bible.rs`                               | bible import, Czech/English book names, references, search, pins                        |
| `net.rs`, `client_app.rs`                | network outputs                                                                         |
| `video.rs`, `app/video_ctl.rs`           | video playback (ffmpeg / ffplay), adding videos, live video controls                   |
| `shortcuts.rs`, `settings.rs`, `i18n.rs` | customisable keys, settings, translations                                               |
| `app/`                                   | the UI, one file per tab (`keynav.rs`: Alt navigation, keyboard focus frame, scrolling) |
| `build.rs`, `assets/`                    | the icon (put into the Windows `.exe`, used as the window icon)                         |
| `packaging/`                             | Linux `.desktop` file and install script, flatpak manifest, Windows Inno Setup script and ffmpeg download |

Not carried over from the original: video objects inside designs (they were disabled in its UI;
videos are service items here) and the system font picker (text uses a system sans font or a
monospace one).

## License

Copyright (C) the oh! Presenter authors.

oh! Presenter is free software: you can redistribute it and/or modify it under the terms of the
GNU General Public License as published by the Free Software Foundation, either version 3 of
the License, or (at your option) any later version. It is distributed in the hope that it will
be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or
FITNESS FOR A PARTICULAR PURPOSE. See [LICENSE](LICENSE) for the full text.

The Windows installer and the flatpak include ffmpeg, which is also GPL licensed
(<https://ffmpeg.org/legal.html>).
