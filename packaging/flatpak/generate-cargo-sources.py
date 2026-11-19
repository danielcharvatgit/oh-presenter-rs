#!/usr/bin/env python3
"""Writes cargo-sources.json for the flatpak build from Cargo.lock.

flatpak-builder builds without network, so every crate is listed as a download
(with its checksum from Cargo.lock) that is unpacked into cargo/vendor, and cargo is
told to use that folder instead of crates.io. Run again after Cargo.lock changes:

    python3 packaging/flatpak/generate-cargo-sources.py

(The same as flatpak-cargo-generator.py from flatpak-builder-tools, for crates.io
dependencies only, and with nothing to install.)
"""
import json
import sys
import tomllib
from pathlib import Path

root = Path(__file__).resolve().parents[2]
lock = tomllib.loads((root / "Cargo.lock").read_text())
sources = []
for pkg in lock["package"]:
    source = pkg.get("source", "")
    if not source:
        continue  # this program itself
    if not source.startswith("registry+https://github.com/rust-lang/crates.io-index"):
        sys.exit(f"{pkg['name']}: only crates.io dependencies are supported, not {source}")
    name, version, checksum = pkg["name"], pkg["version"], pkg["checksum"]
    dest = f"cargo/vendor/{name}-{version}"
    sources.append({
        "type": "archive",
        "archive-type": "tar-gzip",
        "url": f"https://static.crates.io/crates/{name}/{name}-{version}.crate",
        "sha256": checksum,
        "dest": dest,
    })
    sources.append({
        "type": "inline",
        "contents": json.dumps({"package": checksum, "files": {}}),
        "dest": dest,
        "dest-filename": ".cargo-checksum.json",
    })
sources.append({
    "type": "inline",
    "contents": '[source.vendored-sources]\ndirectory = "cargo/vendor"\n\n'
                '[source.crates-io]\nreplace-with = "vendored-sources"\n',
    "dest": "cargo",
    "dest-filename": "config.toml",
})
out = Path(__file__).with_name("cargo-sources.json")
out.write_text(json.dumps(sources, indent=1) + "\n")
print(f"{out.relative_to(root)}: {len(sources) // 2} crates")
