#!/usr/bin/env bash
set -euo pipefail

CARTRIDGE_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROM="$CARTRIDGE_DIR/bundle/controller-test.nes"
EXPECTED_SHA256="76ed7c02c5137d9d0f478f71e0200a60b2e469b778755b78b9a3f1865918a150"

python3 "$CARTRIDGE_DIR/build_rom.py" "$ROM"
printf '%s  %s\n' "$EXPECTED_SHA256" "$ROM" | sha256sum --check --status || {
  printf 'package.sh: generated ROM SHA-256 mismatch\n' >&2
  exit 1
}

python3 - "$CARTRIDGE_DIR" <<'PY'
import json
import os
import shutil
import sys
import tempfile

root = sys.argv[1]
bundle = os.path.join(root, "bundle")
with open(os.path.join(root, "manifest.json"), "rb") as handle:
    manifest_bytes = handle.read()
manifest = json.loads(manifest_bytes)
with open(os.path.join(bundle, "controller-test.nes"), "rb") as handle:
    rom = handle.read()

staging = tempfile.mkdtemp(prefix=".nes-controller-test.", dir=root)
try:
    files = {
        "manifest.json": manifest_bytes,
        "controller-test.nes": rom,
    }
    for name in ("LICENSE", "build_rom.py"):
        with open(os.path.join(root, name), "rb") as handle:
            files[name] = handle.read()
    for name, data in files.items():
        path = os.path.join(staging, name)
        with open(path, "wb") as handle:
            handle.write(data)
        os.chmod(path, 0o444)
    shutil.rmtree(bundle)
    os.replace(staging, bundle)
finally:
    shutil.rmtree(staging, ignore_errors=True)

print(f"Created {bundle}")
PY
