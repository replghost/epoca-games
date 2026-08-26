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
mkdir -p "$BUNDLE_DIR/game" "$BUNDLE_DIR/LICENSES" "$BUNDLE_DIR/DISTRIBUTION" "$BUNDLE_DIR/SOURCES"
polkatool link --min-stack-size 1048576 "$GUEST_ELF" -o "$BUNDLE_DIR/app.polkavm" ||
  fail "polkatool failed to link app.polkavm"
cp "$DUKE_DIR/manifest.json" "$BUNDLE_DIR/manifest.json"
cp "$DUKE_DIR/icon.png" "$BUNDLE_DIR/icon.png"
cp "$DUKE_DIR/LICENSE" "$BUNDLE_DIR/LICENSES/Duke3D-GPL-2.0-or-later.txt"

if [[ -n "${DUKE3D_GRP:-}" ]]; then
  cp "$DUKE3D_GRP" "$BUNDLE_DIR/game/duke3d.grp"
else
  "$ROOT/content/duke3d-shareware/package.sh"
  python3 - "$ROOT/content/duke3d-shareware/bundle/3dduke13.zip" "$BUNDLE_DIR/game/duke3d.grp" <<'PY'
import hashlib
import io
import sys
import zipfile

source, output = sys.argv[1:]
with zipfile.ZipFile(source) as outer:
    nested_bytes = outer.read("DN3DSW13.SHR")
with zipfile.ZipFile(io.BytesIO(nested_bytes)) as nested:
    grp = nested.read("DUKE3D.GRP")
expected = "f943d0c2e2a0803a644a2107c81ea897dec87596d9dd1a6a432131ad6f5818d6"
actual = hashlib.sha256(grp).hexdigest()
if actual != expected:
    raise SystemExit(f"Duke shareware GRP SHA-256 mismatch: expected {expected}, got {actual}")
with open(output, "wb") as handle:
    handle.write(grp)
PY
  cp "$ROOT/content/duke3d-shareware/bundle/3dduke13.zip" "$BUNDLE_DIR/DISTRIBUTION/3dduke13.zip"
  cp "$ROOT/content/duke3d-shareware/bundle/LICENSE.TXT" "$BUNDLE_DIR/LICENSES/Duke3D-shareware-LICENSE.txt"
  cp "$ROOT/content/duke3d-shareware/bundle/SOURCES.json" "$BUNDLE_DIR/SOURCES/Duke3D-shareware.json"
fi

printf 'Built %s\n' "$BUNDLE_DIR"
