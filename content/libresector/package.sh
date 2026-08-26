#!/usr/bin/env bash
set -euo pipefail

LIBRE_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
OUTPUT="$LIBRE_DIR/bundle"

python3 - "$LIBRE_DIR" "$OUTPUT" <<'PY'
import hashlib
import json
import os
import shutil
import subprocess
import struct
import sys
import tempfile

source, output = sys.argv[1:]
manifest_path = os.path.join(source, "manifest.json")
generator_path = os.path.join(source, "generate.py")
license_path = os.path.join(source, "LICENSE")

with open(manifest_path, "rb") as handle:
    manifest_bytes = handle.read()
manifest = json.loads(manifest_bytes)
expected = manifest["items"][0]["integrity"]["sha256"]

parent = os.path.dirname(output)
os.makedirs(parent, exist_ok=True)
staging = tempfile.mkdtemp(prefix=".libresector-bundle.", dir=parent)
try:
    grp_path = os.path.join(staging, "duke3d.grp")
    subprocess.run([sys.executable, generator_path, grp_path], check=True)
    with open(grp_path, "rb") as handle:
        grp = handle.read()
    digest = hashlib.sha256(grp).hexdigest()
    if digest != expected:
        raise SystemExit(
            f"manifest GRP SHA-256 mismatch: expected {expected}, got {digest}"
        )
    if len(grp) < 16 or grp[:12] != b"KenSilverman":
        raise SystemExit("generated content is not a Duke GRP")
    entry_count = struct.unpack_from("<I", grp, 12)[0]
    directory_end = 16 + entry_count * 16
    if directory_end > len(grp):
        raise SystemExit("generated GRP directory is out of bounds")
    names = set()
    data_offset = directory_end
    for index in range(entry_count):
        offset = 16 + index * 16
        name = grp[offset : offset + 12].split(b"\0", 1)[0].decode("ascii")
        size = struct.unpack_from("<I", grp, offset + 12)[0]
        if not name or name.upper() in names or size > len(grp) - data_offset:
            raise SystemExit("generated GRP directory is invalid")
        names.add(name.upper())
        data_offset += size
    required = {
        "LIBRE.PACK",
        "GAME.CON",
        "TABLES.DAT",
        "PALETTE.DAT",
        "LOOKUP.DAT",
        "TILES000.ART",
        "E1L1.MAP",
    }
    if names != required or data_offset != len(grp):
        raise SystemExit("generated GRP contents are incomplete")
    for name, path in (
        ("manifest.json", manifest_path),
        ("LICENSE", license_path),
        ("generate.py", generator_path),
    ):
        shutil.copyfile(path, os.path.join(staging, name))
    for name in ("manifest.json", "LICENSE", "generate.py", "duke3d.grp"):
        os.chmod(os.path.join(staging, name), 0o444)
    shutil.rmtree(output, ignore_errors=True)
    os.replace(staging, output)
finally:
    shutil.rmtree(staging, ignore_errors=True)

print(f"Created {output}")
PY
