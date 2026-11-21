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
# --disable-rofiles-fuse: rofiles-fuse fails where fuse is not usable (containers,
# some VMs: "fusermount: file descriptor 4 is not a socket")
# the filter drops the per-dependency "Updating ... Nothing to do." chatter;
# the builder's exit code is kept in $WORK/rc because a pipe would hide it
{ flatpak run org.flatpak.Builder --user --install --install-deps-from=flathub --force-clean \
    --disable-rofiles-fuse \
    --state-dir="$WORK/state" --repo="$WORK/repo" "$WORK/build" "$MANIFEST" 2>&1 \
    && echo 0 >"$WORK/rc" || echo $? >"$WORK/rc"; } \
  | grep -v -E '^(Dependency (Sdk|Runtime|Extension): |Updating org\.|Nothing to do\.$|Emptying app dir |$)' || true
RC=$(cat "$WORK/rc")
[ "$RC" = 0 ] || exit "$RC"
if [ "$1" = "--bundle" ]; then
    flatpak build-bundle --runtime-repo=https://dl.flathub.org/repo/flathub.flatpakrepo \
        "$WORK/repo" oh-presenter-rs.flatpak cz.oh.Presenter
    echo "Written: oh-presenter-rs.flatpak"
fi
echo "Installed. Start it from the applications menu or: flatpak run cz.oh.Presenter"
