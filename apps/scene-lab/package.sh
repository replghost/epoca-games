#!/usr/bin/env bash
set -euo pipefail
APP_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd "$APP_DIR/../.." && pwd)"
mkdir -p "$APP_DIR/assets"
PYTHONPATH="$ROOT/scripts" python3 - "$APP_DIR" <<'PY'
import sys
from pathlib import Path
from epoca_gltf import convert

app = Path(sys.argv[1])
(app / "assets" / "showcase.epm").write_bytes(
    convert(app / "assets-src" / "showcase.gltf")
)
PY
"$ROOT/scripts/build-rust-app.sh" "$APP_DIR"
