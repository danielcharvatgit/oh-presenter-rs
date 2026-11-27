#!/bin/sh
# Installs the built web edition under /var/www (run ./build.sh first; usually as root):
#   /var/www/oh-presenter-web/          the PWA (dist/), served by nginx / Apache 2 / the Axum server
#   /var/www/oh-presenter-web-server/   the Axum server binary (when built), next to the served
#                                       directory, never inside it
# Only the web server configs (nginx.conf, apache2.conf) go outside /var/www (/etc/nginx, /etc/apache2).
# OH_PRESENTER_WEB_ROOT installs somewhere else instead of /var/www.
set -eu
# relative paths in the variables mean relative to where the script was started
absolute() { case "$1" in /*) printf '%s\n' "$1" ;; *) printf '%s/%s\n' "$PWD" "$1" ;; esac; }
ROOT="$(absolute "${OH_PRESENTER_WEB_ROOT:-/var/www}")"
TARGET="${CARGO_TARGET_DIR:+$(absolute "$CARGO_TARGET_DIR")}"
cd "$(dirname "$0")"
SITE="$ROOT/oh-presenter-web"
SERVER_DIR="$ROOT/oh-presenter-web-server"
SERVER_BIN="${TARGET:-server/target}/release/oh-presenter-web-server"

if [ ! -f dist/index.html ]; then
    echo "dist/ is not built: run ./build.sh first" >&2
    exit 1
fi

# a fresh copy: files of an older build do not stay behind (the service worker lists every file)
mkdir -p "$ROOT"
rm -rf "$SITE.new"
cp -R dist "$SITE.new"
chmod -R a+rX "$SITE.new"
rm -rf "$SITE"
mv "$SITE.new" "$SITE"
echo "installed the web edition to $SITE"

if [ -f "$SERVER_BIN" ]; then
    mkdir -p "$SERVER_DIR"
    install -m 755 "$SERVER_BIN" "$SERVER_DIR/oh-presenter-web-server"
    echo "installed the server to $SERVER_DIR (it serves $SITE by default)"
else
    echo "no server binary at $SERVER_BIN: the optional Axum server was not installed" \
        "(build it with: cd server && cargo build --release; sudo drops CARGO_TARGET_DIR, use sudo -E)"
fi
