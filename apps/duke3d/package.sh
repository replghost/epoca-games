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

if [[ -n "${DUKE3D_GRP:-}" && (! -f "$DUKE3D_GRP" || ! -r "$DUKE3D_GRP") ]]; then
  fail "DUKE3D_GRP is not a readable file: $DUKE3D_GRP"
fi

DUKE_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd "$DUKE_DIR/../.." && pwd)"
BUNDLE_DIR="$DUKE_DIR/bundle"
TOOLCHAIN="nightly-2025-05-10"
RUSTC="$(rustup which --toolchain "$TOOLCHAIN" rustc)" ||
  fail "Rust toolchain $TOOLCHAIN with rust-src is required"
TARGET_JSON="$(RUSTC="$RUSTC" polkatool get-target-json-path --bitness 32)" ||
  fail "polkatool could not provide its 32-bit target specification"
[[ -f "$TARGET_JSON" ]] || fail "PolkaVM target specification was not found: $TARGET_JSON"
TARGET_NAME="$(basename "$TARGET_JSON" .json)"

if ! cargo +"$TOOLCHAIN" build \
  -Z build-std=core,alloc \
  --manifest-path "$DUKE_DIR/Cargo.toml" \
  --target-dir "$ROOT/.tmp/target/duke3d" \
  --target "$TARGET_JSON" \
  --release; then
  fail "Rust build failed (install the nightly toolchain and its rust-src component)"
fi

GUEST_ELF="$ROOT/.tmp/target/duke3d/$TARGET_NAME/release/duke3d-guest"
[[ -f "$GUEST_ELF" ]] || fail "Rust build did not produce the expected guest executable: $GUEST_ELF"
rm -rf "$BUNDLE_DIR"
mkdir -p "$BUNDLE_DIR/game" "$BUNDLE_DIR/LICENSES"
polkatool link --min-stack-size 1048576 "$GUEST_ELF" -o "$BUNDLE_DIR/app.polkavm" ||
  fail "polkatool failed to link app.polkavm"
cp "$DUKE_DIR/manifest.json" "$BUNDLE_DIR/manifest.json"
cp "$DUKE_DIR/icon.png" "$BUNDLE_DIR/icon.png"
cp "$DUKE_DIR/LICENSE" "$BUNDLE_DIR/LICENSES/Duke3D-GPL-2.0-or-later.txt"

if [[ -n "${DUKE3D_GRP:-}" ]]; then
  cp "$DUKE3D_GRP" "$BUNDLE_DIR/game/duke3d.grp"
else
  "$ROOT/content/libresector/package.sh"
  cp "$ROOT/content/libresector/bundle/duke3d.grp" "$BUNDLE_DIR/game/duke3d.grp"
  cp "$ROOT/content/libresector/bundle/LICENSE" "$BUNDLE_DIR/LICENSES/LibreSector-CC0-1.0.txt"
  cp "$ROOT/content/libresector/bundle/generate.py" "$BUNDLE_DIR/LICENSES/LibreSector-source.py"
fi

printf 'Built %s\n' "$BUNDLE_DIR"
