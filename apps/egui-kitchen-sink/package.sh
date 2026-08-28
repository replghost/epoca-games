#!/usr/bin/env bash
set -euo pipefail
APP_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
"$APP_DIR/../../scripts/build-rust-app.sh" "$APP_DIR"
