#!/bin/sh
# Rebuilds all packages of oh! Presenter on Ubuntu / Debian and puts them into dist/:
#   oh-presenter-rs_<version>-1_amd64.deb   Ubuntu / Debian package
#   oh-presenter-rs.flatpak                 flatpak bundle
#   oh-presenter-rs.exe                     Windows program (MinGW-w64 cross build)
#   oh-presenter-rs-setup-<version>.exe     Windows installer (Inno Setup run in Wine,
#                                           with ffmpeg.exe / ffplay.exe for videos)
#
# Only builds - never installs the app (installing is done by the other scripts,
# e.g. packaging/flatpak/build.sh).
#
# Run:  sh build.sh
# Needs once:  sudo apt install build-essential mingw-w64 curl unzip wine
#              rustup target add x86_64-pc-windows-gnu
#              flatpak + org.freedesktop.Sdk//26.08 (missing flatpak parts -
#              flathub remote, Platform, rust-stable extension, org.flatpak.Builder -
#              are added below for the current user)
# Inno Setup is installed by this script into its own Wine prefix ($WINE_PREFIX,
# default ~/.cache/oh-presenter-wine); without wine the installer is skipped
# (then make it on Windows: iscc packaging\windows\oh-presenter.iss).
set -e
cd "$(dirname "$0")"

# the runtime branch the manifest builds against
FP_BRANCH=26.08
FLATHUB=https://dl.flathub.org/repo/flathub.flatpakrepo
INNO_URL=https://jrsoftware.org/download.php/is.exe
FFMPEG_URL=https://www.gyan.dev/ffmpeg/builds/ffmpeg-release-essentials.zip
VERSION=$(sed -n 's/^version *= *"\(.*\)"/\1/p' Cargo.toml | head -n 1)

# build files on a local disk (a network drive breaks the build scripts of sqlite / zstd)
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$HOME/.cache/oh-presenter-target}"
mkdir -p dist

echo "== .deb"
cargo deb --version >/dev/null 2>&1 || cargo install cargo-deb
cargo deb
cp "$CARGO_TARGET_DIR"/debian/oh-presenter-rs_*.deb dist/

echo "== Windows .exe"
cargo build --release --target x86_64-pc-windows-gnu
EXE="$CARGO_TARGET_DIR/x86_64-pc-windows-gnu/release/oh-presenter-rs.exe"
cp "$EXE" dist/
# where packaging/windows/oh-presenter.iss looks for it
mkdir -p target/x86_64-pc-windows-gnu/release
cp "$EXE" target/x86_64-pc-windows-gnu/release/

echo "== Windows installer"
if ! command -v wine >/dev/null 2>&1; then
    echo "wine is not installed - skipping the Windows installer"
else
    export WINEPREFIX="${WINE_PREFIX:-$HOME/.cache/oh-presenter-wine}"
    export WINEDEBUG=-all
    # no Mono / Gecko install dialogs (Inno Setup needs neither)
    export WINEDLLOVERRIDES="mscoree,mshtml="
    CACHE="$HOME/.cache/oh-presenter-downloads"
    mkdir -p "$CACHE"

    # Inno Setup, installed once into the Wine prefix
    ISCC=$(find "$WINEPREFIX/drive_c" -iname ISCC.exe 2>/dev/null | head -n 1)
    if [ -z "$ISCC" ]; then
        [ -f "$CACHE/innosetup.exe" ] || curl -fL -o "$CACHE/innosetup.exe" "$INNO_URL"
        wine "$CACHE/innosetup.exe" /VERYSILENT /SUPPRESSMSGBOXES /NORESTART /SP- </dev/null
        wineserver -w
        ISCC=$(find "$WINEPREFIX/drive_c" -iname ISCC.exe 2>/dev/null | head -n 1)
        [ -n "$ISCC" ] || { echo "Inno Setup did not install into $WINEPREFIX"; exit 1; }
    fi

    # ffmpeg.exe / ffplay.exe for videos (what get-ffmpeg.ps1 does on Windows)
    FF=packaging/windows/ffmpeg
    if [ ! -f "$FF/ffmpeg.exe" ] || [ ! -f "$FF/ffplay.exe" ]; then
        [ -f "$CACHE/ffmpeg.zip" ] || curl -fL -o "$CACHE/ffmpeg.zip" "$FFMPEG_URL"
        TMP=$(mktemp -d)
        unzip -q "$CACHE/ffmpeg.zip" -d "$TMP"
        mkdir -p "$FF"
        BIN=$(dirname "$(find "$TMP" -name ffmpeg.exe | head -n 1)")
        cp "$BIN/ffmpeg.exe" "$BIN/ffplay.exe" "$FF/"
        LIC=$(find "$TMP" -name 'LICENSE*' | head -n 1)
        [ -z "$LIC" ] || cp "$LIC" "$FF/LICENSE.txt"
        rm -rf "$TMP"
    fi

    wine "$ISCC" /Q "$(winepath -w packaging/windows/oh-presenter.iss)" </dev/null
    cp "target/installer/oh-presenter-rs-setup-$VERSION.exe" dist/
fi

echo "== flatpak"
if ! command -v flatpak >/dev/null 2>&1; then
    echo "flatpak is not installed - skipping the flatpak bundle"
else
    # flathub for the current user (also needed by --install-deps-from=flathub)
    flatpak remote-add --user --if-not-exists flathub "$FLATHUB"
    # install only what is missing (user or system install both count)
    for REF in org.freedesktop.Sdk//$FP_BRANCH \
               org.freedesktop.Platform//$FP_BRANCH \
               org.freedesktop.Sdk.Extension.rust-stable//$FP_BRANCH \
               org.flatpak.Builder; do
        flatpak info "$REF" >/dev/null 2>&1 ||
            flatpak install --user -y --noninteractive flathub "$REF"
    done
    # built here, not with packaging/flatpak/build.sh, because that one also installs
    # the app; without --install the builder only exports it into $WORK/repo
    WORK="${FLATPAK_WORK:-$HOME/.cache/oh-presenter-flatpak}"
    mkdir -p "$WORK"
    python3 packaging/flatpak/generate-cargo-sources.py
    # the builder's exit code is kept in $WORK/rc because a pipe would hide it
    { flatpak run org.flatpak.Builder --user --install-deps-from=flathub --force-clean \
        --disable-rofiles-fuse \
        --state-dir="$WORK/state" --repo="$WORK/repo" "$WORK/build" \
        packaging/flatpak/cz.oh.Presenter.yml 2>&1 \
        && echo 0 >"$WORK/rc" || echo $? >"$WORK/rc"; } \
      | grep -v -E '^(Dependency (Sdk|Runtime|Extension): |Updating org\.|Nothing to do\.$|Emptying app dir |$)' || true
    RC=$(cat "$WORK/rc")
    [ "$RC" = 0 ] || exit "$RC"
    flatpak build-bundle --runtime-repo="$FLATHUB" \
        "$WORK/repo" dist/oh-presenter-rs.flatpak cz.oh.Presenter
fi

echo "Done:"
ls -l dist/
