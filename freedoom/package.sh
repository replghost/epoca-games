#!/usr/bin/env bash
set -euo pipefail

FREEDOOM_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ARCHIVE="${FREEDOOM_ARCHIVE:-$FREEDOOM_DIR/freedoom-0.13.0.zip}"
URL="${FREEDOOM_URL:-https://github.com/freedoom/freedoom/releases/download/v0.13.0/freedoom-0.13.0.zip}"

python3 - "$ARCHIVE" "$URL" "$FREEDOOM_DIR/manifest.json" "$FREEDOOM_DIR/bundle" <<'PY'
import hashlib
import json
import os
import shutil
import struct
import sys
import tempfile
import urllib.request
import zipfile

archive_path, url, manifest_path, output = sys.argv[1:]
archive_sha256 = "3f9b264f3e3ce503b4fb7f6bdcb1f419d93c7b546f4df3e874dd878db9688f59"
wad_sha256 = "7323bcc168c5a45ff10749b339960e98314740a734c30d4b9f3337001f9e703d"
prefix = "freedoom-0.13.0/"
files = {
    "freedoom1.wad": prefix + "freedoom1.wad",
    "COPYING.txt": prefix + "COPYING.txt",
    "CREDITS.txt": prefix + "CREDITS.txt",
    "CREDITS-MUSIC.txt": prefix + "CREDITS-MUSIC.txt",
}

if not os.path.exists(archive_path):
    os.makedirs(os.path.dirname(archive_path), exist_ok=True)
    descriptor, temporary = tempfile.mkstemp(
        prefix=".freedoom-0.13.0.", suffix=".zip", dir=os.path.dirname(archive_path)
    )
    os.close(descriptor)
    try:
        urllib.request.urlretrieve(url, temporary)
        os.replace(temporary, archive_path)
    finally:
        if os.path.exists(temporary):
            os.unlink(temporary)

with open(archive_path, "rb") as handle:
    archive_digest = hashlib.sha256(handle.read()).hexdigest()
if archive_digest != archive_sha256:
    raise SystemExit(
        f"Freedoom archive SHA-256 mismatch: expected {archive_sha256}, got {archive_digest}"
    )

with zipfile.ZipFile(archive_path) as archive:
    extracted = {name: archive.read(source) for name, source in files.items()}

wad = extracted["freedoom1.wad"]
if hashlib.sha256(wad).hexdigest() != wad_sha256:
    raise SystemExit("Freedoom Phase 1 IWAD SHA-256 mismatch")
if len(wad) < 12 or wad[:4] != b"IWAD":
    raise SystemExit("Freedoom Phase 1 is not an IWAD")
lump_count, directory_offset = struct.unpack_from("<II", wad, 4)
directory_size = lump_count * 16
if directory_offset > len(wad) or directory_size > len(wad) - directory_offset:
    raise SystemExit("Freedoom Phase 1 IWAD directory is out of bounds")
for index in range(lump_count):
    lump_offset, lump_size = struct.unpack_from(
        "<II", wad, directory_offset + index * 16
    )
    if lump_offset > len(wad) or lump_size > len(wad) - lump_offset:
        raise SystemExit("Freedoom Phase 1 IWAD lump is out of bounds")

with open(manifest_path, "rb") as handle:
    manifest_bytes = handle.read()
manifest = json.loads(manifest_bytes)
if manifest["items"][0]["integrity"]["sha256"] != wad_sha256:
    raise SystemExit("Freedoom manifest does not pin the packaged IWAD")

parent = os.path.dirname(output)
os.makedirs(parent, exist_ok=True)
staging = tempfile.mkdtemp(prefix=".freedoom-bundle.", dir=parent)
try:
    package_files = {"manifest.json": manifest_bytes, **extracted}
    for name, data in package_files.items():
        path = os.path.join(staging, name)
        with open(path, "wb") as handle:
            handle.write(data)
        os.chmod(path, 0o444)
    shutil.rmtree(output, ignore_errors=True)
    os.replace(staging, output)
finally:
    shutil.rmtree(staging, ignore_errors=True)

print(f"Created {output}")
PY
