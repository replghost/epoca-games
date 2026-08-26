#!/usr/bin/env bash
set -euo pipefail

fail() {
  printf 'package.sh: %s\n' "$*" >&2
  exit 1
}

for tool in cargo polkatool python3 rustup; do
  command -v "$tool" >/dev/null 2>&1 || fail "required tool '$tool' was not found in PATH"
done
if [[ -z "${PVM_CLANG:-}" ]] && command -v brew >/dev/null 2>&1; then
  PVM_CLANG="$(brew --prefix llvm)/bin/clang"
fi
PVM_CLANG="${PVM_CLANG:-clang}"
command -v "$PVM_CLANG" >/dev/null 2>&1 ||
  fail "a clang build with the RISC-V target is required (set PVM_CLANG)"
export PVM_CLANG
LLVM_BIN="$(dirname "$(command -v "$PVM_CLANG")")"
PVM_LLVM_AR="${PVM_LLVM_AR:-$LLVM_BIN/llvm-ar}"
PVM_LLVM_RANLIB="${PVM_LLVM_RANLIB:-$LLVM_BIN/llvm-ranlib}"
[[ -x "$PVM_LLVM_AR" && -x "$PVM_LLVM_RANLIB" ]] ||
  fail "llvm-ar and llvm-ranlib are required beside PVM_CLANG"
export PVM_LLVM_AR PVM_LLVM_RANLIB

if [[ -n "${DOOM_IWAD:-}" && (! -f "$DOOM_IWAD" || ! -r "$DOOM_IWAD") ]]; then
  fail "DOOM_IWAD is not a readable file: $DOOM_IWAD"
fi

DOOM_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd "$DOOM_DIR/../.." && pwd)"
BUNDLE_DIR="$DOOM_DIR/bundle"
TOOLCHAIN="nightly-2025-05-10"
RUSTC="$(rustup which --toolchain "$TOOLCHAIN" rustc)" ||
  fail "Rust toolchain $TOOLCHAIN with rust-src is required"
TARGET_JSON="$(RUSTC="$RUSTC" polkatool get-target-json-path --bitness 32)" ||
  fail "polkatool could not provide its 32-bit target specification"
[[ -f "$TARGET_JSON" ]] || fail "PolkaVM target specification was not found: $TARGET_JSON"
TARGET_NAME="$(basename "$TARGET_JSON" .json)"

if ! cargo +"$TOOLCHAIN" build \
  -Z build-std=core,alloc \
  --manifest-path "$DOOM_DIR/Cargo.toml" \
  --target-dir "$ROOT/.tmp/target/doom" \
  --target "$TARGET_JSON" \
  --release; then
  fail "Rust build failed (install the nightly toolchain and its rust-src component)"
fi

GUEST_ELF="$ROOT/.tmp/target/doom/$TARGET_NAME/release/doom-guest"
[[ -f "$GUEST_ELF" ]] || fail "Rust build did not produce the expected guest executable: $GUEST_ELF"
rm -rf "$BUNDLE_DIR"
mkdir -p "$BUNDLE_DIR/game" "$BUNDLE_DIR/LICENSES"
polkatool link "$GUEST_ELF" -o "$BUNDLE_DIR/app.polkavm" ||
  fail "polkatool failed to link app.polkavm"
cp "$DOOM_DIR/manifest.json" "$BUNDLE_DIR/manifest.json"
cp "$DOOM_DIR/icon.png" "$BUNDLE_DIR/icon.png"
cp "$DOOM_DIR/LICENSE" "$BUNDLE_DIR/LICENSES/doomgeneric-GPL-2.0.txt"

if [[ -n "${DOOM_IWAD:-}" ]]; then
  cp "$DOOM_IWAD" "$BUNDLE_DIR/game/doom.wad"
else
  "$ROOT/content/freedoom/package.sh"
  cp "$ROOT/content/freedoom/bundle/freedoom1.wad" "$BUNDLE_DIR/game/doom.wad"
  cp "$ROOT/content/freedoom/bundle/COPYING.txt" "$BUNDLE_DIR/LICENSES/Freedoom-COPYING.txt"
  cp "$ROOT/content/freedoom/bundle/CREDITS.txt" "$BUNDLE_DIR/LICENSES/Freedoom-CREDITS.txt"
  cp "$ROOT/content/freedoom/bundle/CREDITS-MUSIC.txt" "$BUNDLE_DIR/LICENSES/Freedoom-CREDITS-MUSIC.txt"
fi

printf 'Built %s\n' "$BUNDLE_DIR"
