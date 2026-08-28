#!/usr/bin/env bash
set -euo pipefail

SHAREWARE_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ARCHIVE="${DUKE3D_SHAREWARE_ARCHIVE:-$SHAREWARE_DIR/3dduke13.zip}"
URL="${DUKE3D_SHAREWARE_URL:-https://archive.org/download/3dduke13/3dduke13.zip}"

python3 - "$ARCHIVE" "$URL" "$SHAREWARE_DIR/manifest.json" "$SHAREWARE_DIR/SOURCES.json" "$SHAREWARE_DIR/index.html" "$SHAREWARE_DIR/bundle" <<'PY'
import hashlib
import io
import json
import os
import shutil
import sys
import tempfile
import urllib.request
import zipfile

archive_path, url, manifest_path, sources_path, index_path, output = sys.argv[1:]
archive_sha256 = "c67efd179022bc6d9bde54f404c707cbcbdc15423c20be72e277bc2bdddf3d0e"
archive_size = 5924374
grp_sha256 = "f943d0c2e2a0803a644a2107c81ea897dec87596d9dd1a6a432131ad6f5818d6"
grp_size = 11035779
outer_names = {"DN3DSW13.SHR", "FILE_ID.DIZ", "INSTALL.EXE", "LICENSE.TXT"}
nested_names = {
    "LICENSE.TXT",
    "COMMIT.EXE",
    "DEFS.CON",
    "DEMO1.DMO",
    "DEMO2.DMO",
    "DEMO3.DMO",
    "DN3DHELP.EXE",
    "DUKE.RTS",
    "DUKE3D.EXE",
    "DUKE3D.GRP",
    "GAME.CON",
    "MODEM.PCK",
    "README.DOC",
    "SETMAIN.EXE",
    "SETUP.EXE",
    "ULTRAMID.INI",
    "USER.CON",
}

if not os.path.exists(archive_path):
    os.makedirs(os.path.dirname(archive_path), exist_ok=True)
    descriptor, temporary = tempfile.mkstemp(
        prefix=".3dduke13.", suffix=".zip", dir=os.path.dirname(archive_path)
    )
    os.close(descriptor)
    try:
        urllib.request.urlretrieve(url, temporary)
        os.replace(temporary, archive_path)
    finally:
        if os.path.exists(temporary):
            os.unlink(temporary)

with open(archive_path, "rb") as handle:
    archive_bytes = handle.read()
actual_sha256 = hashlib.sha256(archive_bytes).hexdigest()
if actual_sha256 != archive_sha256 or len(archive_bytes) != archive_size:
    raise SystemExit(
        "Duke shareware archive identity mismatch: "
        f"expected {archive_sha256}/{archive_size}, got {actual_sha256}/{len(archive_bytes)}"
    )

with zipfile.ZipFile(io.BytesIO(archive_bytes)) as outer:
    if set(outer.namelist()) != outer_names:
        raise SystemExit(f"Duke shareware outer archive entries changed: {outer.namelist()}")
    license_bytes = outer.read("LICENSE.TXT")
    nested_bytes = outer.read("DN3DSW13.SHR")
with zipfile.ZipFile(io.BytesIO(nested_bytes)) as nested:
    if set(nested.namelist()) != nested_names:
        raise SystemExit(f"Duke shareware nested archive entries changed: {nested.namelist()}")
    if nested.read("LICENSE.TXT") != license_bytes:
        raise SystemExit("Duke shareware license copies differ")
    grp = nested.read("DUKE3D.GRP")
if len(grp) != grp_size or hashlib.sha256(grp).hexdigest() != grp_sha256:
    raise SystemExit("Duke shareware GRP identity mismatch")
if not license_bytes.startswith(b"Episode One of Duke Nukem 3D"):
    raise SystemExit("Duke shareware license is missing")

with open(manifest_path, "rb") as handle:
    manifest_bytes = handle.read()
manifest = json.loads(manifest_bytes)
item = manifest["items"][0]
if item["entry"] != "3dduke13.zip" or item["integrity"]["sha256"] != archive_sha256:
    raise SystemExit("Duke shareware manifest does not pin the original archive")
with open(sources_path, "rb") as handle:
    sources_bytes = handle.read()
sources = json.loads(sources_bytes)
if (
    sources["distribution"]["sha256"] != archive_sha256
    or sources["distribution"]["size"] != archive_size
    or sources["validation"]["grp"]["sha256"] != grp_sha256
):
    raise SystemExit("Duke shareware source record is stale")
with open(index_path, "rb") as handle:
    index_bytes = handle.read()

parent = os.path.dirname(output)
os.makedirs(parent, exist_ok=True)
staging = tempfile.mkdtemp(prefix=".duke3d-shareware-bundle.", dir=parent)
try:
    package_files = {
        "3dduke13.zip": archive_bytes,
        "LICENSE.TXT": license_bytes,
        "SOURCES.json": sources_bytes,
        "index.html": index_bytes,
        "manifest.json": manifest_bytes,
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

print(f"Created {output} from the unchanged licensed shareware archive")
PY
