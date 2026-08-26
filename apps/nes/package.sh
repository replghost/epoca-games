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

NES_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd "$NES_DIR/../.." && pwd)"
BUNDLE_DIR="$NES_DIR/bundle"
TOOLCHAIN="nightly-2025-10-09"
RUSTC="$(rustup which --toolchain "$TOOLCHAIN" rustc)" ||
  fail "Rust toolchain $TOOLCHAIN with rust-src is required"
TARGET_JSON="$(RUSTC="$RUSTC" polkatool get-target-json-path --bitness 32)" ||
  fail "polkatool could not provide its 32-bit target specification"
[[ -f "$TARGET_JSON" ]] || fail "PolkaVM target specification was not found: $TARGET_JSON"
TARGET_NAME="$(basename "$TARGET_JSON" .json)"

cargo +"$TOOLCHAIN" build \
  -Z build-std=core,alloc \
  --manifest-path "$NES_DIR/Cargo.toml" \
  --target-dir "$ROOT/.tmp/target/nes" \
  --target "$TARGET_JSON" \
  --release || fail "Rust build failed"

GUEST_ELF="$ROOT/.tmp/target/nes/$TARGET_NAME/release/nes-guest"
[[ -f "$GUEST_ELF" ]] || fail "guest executable was not produced: $GUEST_ELF"
rm -rf "$BUNDLE_DIR"
mkdir -p "$BUNDLE_DIR/game" "$BUNDLE_DIR/LICENSES"
polkatool link "$GUEST_ELF" -o "$BUNDLE_DIR/app.polkavm" ||
  fail "polkatool failed to link app.polkavm"
cp "$NES_DIR/manifest.json" "$BUNDLE_DIR/manifest.json"
cp "$NES_DIR/icon.png" "$BUNDLE_DIR/icon.png"
cp "$NES_DIR/vendor/fceumm/Copying" "$BUNDLE_DIR/LICENSES/FCEUmm-GPL-2.0.txt"
cp "$NES_DIR/vendor/fceumm/EPOCA_VENDOR_REVISION" "$BUNDLE_DIR/LICENSES/FCEUmm-SOURCE.txt"

"$ROOT/content/nes-controller-test/package.sh"
cp "$ROOT/content/nes-controller-test/bundle/controller-test.nes" "$BUNDLE_DIR/game/cartridge.nes"
cp "$ROOT/content/nes-controller-test/bundle/LICENSE" "$BUNDLE_DIR/LICENSES/controller-test-MIT.txt"
cp "$ROOT/content/nes-controller-test/bundle/build_rom.py" "$BUNDLE_DIR/LICENSES/controller-test-source.py"

printf 'Built %s\n' "$BUNDLE_DIR"
