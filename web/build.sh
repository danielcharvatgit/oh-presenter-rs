#!/bin/sh
# Builds the web edition into dist/ (a static PWA: serve it with server/ or nginx / apache2).
# Needs: rustup target add wasm32-unknown-unknown; cargo install wasm-bindgen-cli --version 0.2.130
set -eu
cd "$(dirname "$0")"
TARGET_DIR="${CARGO_TARGET_DIR:-target}"
cargo build --release --target wasm32-unknown-unknown
rm -rf dist
mkdir -p dist
wasm-bindgen --target web --no-typescript --out-dir dist --out-name oh-presenter-web \
    "$TARGET_DIR/wasm32-unknown-unknown/release/oh-presenter-web.wasm"
if command -v wasm-opt >/dev/null 2>&1; then
    wasm-opt -Os -o dist/oh-presenter-web_bg.wasm dist/oh-presenter-web_bg.wasm
fi
cp index.html manifest.json icon.svg dist/
# the service worker caches every file of this build, under a name that changes with the build
FILES=$(cd dist && find . -type f ! -name sw.js | sed 's|^\./||' | sort | sed 's/.*/"&"/' | paste -sd, -)
VERSION=$(cd dist && find . -type f | sort | xargs cat | sha256sum | cut -c1-12)
sed -e "s|__FILES__|$FILES|" -e "s|__VERSION__|$VERSION|" sw.js > dist/sw.js
echo "built dist/ (version $VERSION); ./install.sh puts it in /var/www/oh-presenter-web"
