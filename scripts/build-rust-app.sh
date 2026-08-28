#!/usr/bin/env bash
set -euo pipefail

fail() {
  printf 'build-rust-app.sh: %s\n' "$*" >&2
  exit 1
}

[[ $# -eq 1 ]] || fail "usage: build-rust-app.sh <app-directory>"
for tool in cargo polkatool python3 rustup; do
  command -v "$tool" >/dev/null 2>&1 || fail "required tool '$tool' was not found in PATH"
done

APP_DIR="$(cd "$1" && pwd)"
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TOOLCHAIN="${PVM_RUST_TOOLCHAIN:-nightly-2025-10-09}"
RUSTC="$(rustup which --toolchain "$TOOLCHAIN" rustc)" ||
  fail "Rust toolchain $TOOLCHAIN with rust-src is required"
TARGET_JSON="$(RUSTC="$RUSTC" polkatool get-target-json-path --bitness 32)" ||
  fail "polkatool could not provide its 32-bit target specification"
TARGET_NAME="$(basename "$TARGET_JSON" .json)"
TARGET_DIR="$ROOT/.tmp/target/$(basename "$APP_DIR")"

cargo +"$TOOLCHAIN" build \
  -Z build-std=core,alloc \
  --manifest-path "$APP_DIR/Cargo.toml" \
  --target-dir "$TARGET_DIR" \
  --target "$TARGET_JSON" \
  --release

GUEST_ELF="$(python3 - "$APP_DIR/Cargo.toml" "$TARGET_DIR/$TARGET_NAME/release" <<'PY'
import pathlib
import sys
import tomllib

manifest, output = sys.argv[1:]
with open(manifest, "rb") as handle:
    name = tomllib.load(handle)["package"]["name"].replace("-", "_")
candidates = [
    pathlib.Path(output) / f"lib{name}.so",
    pathlib.Path(output) / f"{name}.elf",
    pathlib.Path(output) / name,
    pathlib.Path(output) / name.replace("_", "-"),
]
for candidate in candidates:
    if candidate.is_file():
        print(candidate)
        break
else:
    raise SystemExit(f"could not locate guest ELF; tried: {candidates}")
PY
)"

BUNDLE="$APP_DIR/bundle"
rm -rf "$BUNDLE"
mkdir -p "$BUNDLE"
polkatool link "$GUEST_ELF" -o "$BUNDLE/app.polkavm"
cp "$APP_DIR/manifest.json" "$BUNDLE/manifest.json"
cp "$APP_DIR/icon.png" "$BUNDLE/icon.png"
if [[ -d "$APP_DIR/assets" ]]; then
  cp -R "$APP_DIR/assets/." "$BUNDLE/"
fi

mkdir -p "$BUNDLE/LICENSES"
cp "$ROOT/licenses/MPL-2.0.txt" "$BUNDLE/LICENSES/MPL-2.0.txt"
cp "$ROOT/licenses/PolkaVM-MIT.txt" "$BUNDLE/LICENSES/PolkaVM-MIT.txt"
cp "$ROOT/licenses/PolkaVM-Apache-2.0.txt" "$BUNDLE/LICENSES/PolkaVM-Apache-2.0.txt"
printf 'Built %s\n' "$BUNDLE"