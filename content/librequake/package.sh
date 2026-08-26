#!/usr/bin/env bash
set -euo pipefail

LIBREQUAKE_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ARCHIVE="${LIBREQUAKE_ARCHIVE:-$LIBREQUAKE_DIR/librequake-v0.09-beta-lite.zip}"
URL="${LIBREQUAKE_URL:-https://github.com/lavenderdotpet/LibreQuake/releases/download/v0.09-beta/lite.zip}"

python3 - "$ARCHIVE" "$URL" "$LIBREQUAKE_DIR/manifest.json" "$LIBREQUAKE_DIR/SOURCES.json" "$LIBREQUAKE_DIR/bundle" <<'PY'
import hashlib
import json
import os
import shutil
import struct
import sys
import tempfile
import urllib.request
import zipfile

archive_path, url, manifest_path, sources_path, output = sys.argv[1:]
archive_sha256 = "428e736b2f01d953e09a08c60bee975bdc4a0ac2219e97fa095c8af41754da83"
pak_sha256 = {
    "pak0.pak": "0dd895a425e75d9908025dcfe340f75f5b8166ada68b2d7ee54d8d91d29378ff",
    "pak1.pak": "28423c01836341d9e3c6465b407a44ae94e4e29e954544a67739686850e39365",
}
source_suffixes = {
    "pak0.pak": "/id1/pak0.pak",
    "pak1.pak": "/id1/pak1.pak",
    "COPYING-BSD-3-Clause.txt": "/id1/docs/COPYING",
    "COPYING-GPL-2.0.txt": "/id1/docs/misc-docs/COPYING",
    "CREDITS.txt": "/id1/docs/CREDITS",
    "LICENSE-INFO.txt": "/id1/docs/README-IMPORTANT-LICENCE-INFO",
}

if not os.path.exists(archive_path):
    os.makedirs(os.path.dirname(archive_path), exist_ok=True)
    descriptor, temporary = tempfile.mkstemp(
        prefix=".librequake-v0.09-beta.", suffix=".zip", dir=os.path.dirname(archive_path)
    )
    os.close(descriptor)
    try:
        urllib.request.urlretrieve(url, temporary)
        os.replace(temporary, archive_path)
    finally:
        if os.path.exists(temporary):
            os.unlink(temporary)

with open(archive_path, "rb") as handle:
    digest = hashlib.sha256(handle.read()).hexdigest()
if digest != archive_sha256:
    raise SystemExit(
        f"LibreQuake archive SHA-256 mismatch: expected {archive_sha256}, got {digest}"
    )

with zipfile.ZipFile(archive_path) as archive:
    names = archive.namelist()
    extracted = {}
    for destination, suffix in source_suffixes.items():
        matches = [name for name in names if ("/" + name.lstrip("/")).endswith(suffix)]
        if len(matches) != 1:
            raise SystemExit(
                f"LibreQuake archive must contain exactly one {suffix}: found {matches}"
            )
        extracted[destination] = archive.read(matches[0])


def validate_pak(name, data):
    actual = hashlib.sha256(data).hexdigest()
    if actual != pak_sha256[name]:
        raise SystemExit(f"LibreQuake {name} SHA-256 mismatch: got {actual}")
    if len(data) < 12 or data[:4] != b"PACK":
        raise SystemExit(f"LibreQuake {name} is not a Quake PAK")
    directory_offset, directory_size = struct.unpack_from("<II", data, 4)
    if directory_size % 64:
        raise SystemExit(f"LibreQuake {name} has a malformed directory")
    if directory_offset > len(data) or directory_size > len(data) - directory_offset:
        raise SystemExit(f"LibreQuake {name} directory is out of bounds")
    for offset in range(directory_offset, directory_offset + directory_size, 64):
        raw_name = data[offset : offset + 56].split(b"\0", 1)[0]
        try:
            path = raw_name.decode("ascii")
        except UnicodeDecodeError as error:
            raise SystemExit(f"LibreQuake {name} contains a non-ASCII path") from error
        parts = path.split("/")
        if not path or path.startswith("/") or "\\" in path or any(part in ("", ".", "..") for part in parts):
            raise SystemExit(f"LibreQuake {name} contains an unsafe path: {path!r}")
        file_offset, file_size = struct.unpack_from("<II", data, offset + 56)
        if file_offset > len(data) or file_size > len(data) - file_offset:
            raise SystemExit(f"LibreQuake {name} entry is out of bounds: {path}")


for pak_name in pak_sha256:
    validate_pak(pak_name, extracted[pak_name])

with open(manifest_path, "rb") as handle:
    manifest_bytes = handle.read()
manifest = json.loads(manifest_bytes)
manifest_hashes = {
    item["entry"]: item["integrity"]["sha256"] for item in manifest["items"]
}
if manifest_hashes != pak_sha256:
    raise SystemExit("LibreQuake manifest does not pin the packaged PAKs")
with open(sources_path, "rb") as handle:
    sources_bytes = handle.read()
sources = json.loads(sources_bytes)
if sources["upstream"]["sha256"] != archive_sha256 or sources["files"] != pak_sha256:
    raise SystemExit("LibreQuake source record is stale")

parent = os.path.dirname(output)
os.makedirs(parent, exist_ok=True)
staging = tempfile.mkdtemp(prefix=".librequake-bundle.", dir=parent)
try:
    package_files = {
        "manifest.json": manifest_bytes,
        "SOURCES.json": sources_bytes,
        **extracted,
    }
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
