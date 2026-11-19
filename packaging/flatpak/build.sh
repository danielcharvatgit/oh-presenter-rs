#!/bin/sh
# Builds the flatpak with org.flatpak.Builder and installs it for the current user.
#   packaging/flatpak/build.sh           build and install
#   packaging/flatpak/build.sh --bundle  also write oh-presenter-rs.flatpak (one file
#                                        to copy to other computers:
#                                        flatpak install --user oh-presenter-rs.flatpak)
# Build files go to $FLATPAK_WORK (default ~/.cache/oh-presenter-flatpak).
set -e
cd "$(dirname "$0")/../.."
WORK="${FLATPAK_WORK:-$HOME/.cache/oh-presenter-flatpak}"
MANIFEST=packaging/flatpak/cz.oh.Presenter.yml
mkdir -p "$WORK"

flatpak info org.flatpak.Builder >/dev/null 2>&1 || flatpak install --user -y flathub org.flatpak.Builder
python3 packaging/flatpak/generate-cargo-sources.py
flatpak run org.flatpak.Builder --user --install --install-deps-from=flathub --force-clean \
    --state-dir="$WORK/state" --repo="$WORK/repo" "$WORK/build" "$MANIFEST"
if [ "$1" = "--bundle" ]; then
    flatpak build-bundle --runtime-repo=https://dl.flathub.org/repo/flathub.flatpakrepo \
        "$WORK/repo" oh-presenter-rs.flatpak cz.oh.Presenter
    echo "Written: oh-presenter-rs.flatpak"
fi
echo "Installed. Start it from the applications menu or: flatpak run cz.oh.Presenter"
