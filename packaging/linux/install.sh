#!/bin/sh
# Installs oh! Presenter for the current user (no root needed) so it is listed among
# the applications. Run from the project folder after `cargo build --release`,
# or with --uninstall to remove it again.
#   packaging/linux/install.sh [--uninstall]
set -e
cd "$(dirname "$0")/../.."
BIN="$HOME/.local/bin"
APPS="${XDG_DATA_HOME:-$HOME/.local/share}/applications"
ICONS="${XDG_DATA_HOME:-$HOME/.local/share}/icons/hicolor"

if [ "$1" = "--uninstall" ]; then
    rm -f "$BIN/oh-presenter-rs" "$APPS/oh-presenter-rs.desktop" \
        "$ICONS/256x256/apps/oh-presenter-rs.png" "$ICONS/512x512/apps/oh-presenter-rs.png"
    echo "Removed. (Your songs, designs and settings were kept.)"
    exit 0
fi

EXE="${CARGO_TARGET_DIR:-target}/release/oh-presenter-rs"
[ -x "$EXE" ] || cargo build --release
mkdir -p "$BIN" "$APPS" "$ICONS/256x256/apps" "$ICONS/512x512/apps"
install -m 755 "$EXE" "$BIN/oh-presenter-rs"
install -m 644 assets/icon.png "$ICONS/256x256/apps/oh-presenter-rs.png"
install -m 644 assets/icon-512.png "$ICONS/512x512/apps/oh-presenter-rs.png"
# the full path, so the launcher works even when ~/.local/bin is not on PATH
sed "s|^Exec=oh-presenter-rs|Exec=$BIN/oh-presenter-rs|" packaging/linux/oh-presenter-rs.desktop \
    > "$APPS/oh-presenter-rs.desktop"
chmod 644 "$APPS/oh-presenter-rs.desktop"
update-desktop-database "$APPS" 2>/dev/null || true
gtk-update-icon-cache -q "$ICONS" 2>/dev/null || true
echo "Installed: oh! Presenter is now among your applications."
