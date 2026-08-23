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
BUNDLE_DIR="$NES_DIR/bundle"
TOOLCHAIN="nightly-2025-05-10"
RUSTC="$(rustup which --toolchain "$TOOLCHAIN" rustc)" ||
  fail "Rust toolchain $TOOLCHAIN with rust-src is required"
TARGET_JSON="$(RUSTC="$RUSTC" polkatool get-target-json-path --bitness 32)" ||
  fail "polkatool could not provide its 32-bit target specification"
[[ -f "$TARGET_JSON" ]] || fail "PolkaVM target specification was not found: $TARGET_JSON"
TARGET_NAME="$(basename "$TARGET_JSON" .json)"

cargo +"$TOOLCHAIN" build \
  -Z build-std=core,alloc \
  --manifest-path "$NES_DIR/Cargo.toml" \
  --target-dir "$NES_DIR/target" \
  --target "$TARGET_JSON" \
  --release || fail "Rust build failed"

GUEST_ELF="$NES_DIR/target/$TARGET_NAME/release/nes-guest"
[[ -f "$GUEST_ELF" ]] || fail "guest executable was not produced: $GUEST_ELF"
polkatool link "$GUEST_ELF" -o "$BUNDLE_DIR/app.polkavm" ||
  fail "polkatool failed to link app.polkavm"

python3 - \
  "$BUNDLE_DIR/manifest.json" \
  "$BUNDLE_DIR/app.polkavm" \
  "$NES_DIR/vendor/fceumm/Copying" \
  "$NES_DIR/vendor/fceumm/EPOCA_VENDOR_REVISION" \
  "$NES_DIR/index.html" \
  "$NES_DIR/nes.prod" <<'PY'
import os
import sys
import tempfile
import zipfile

sources = [
    (sys.argv[1], "manifest.json", 0o644),
    (sys.argv[2], "app.polkavm", 0o644),
    (sys.argv[3], "LICENSE", 0o644),
    (sys.argv[4], "SOURCE.txt", 0o644),
    (sys.argv[5], "index.html", 0o644),
]
output = sys.argv[6]
fd, temporary = tempfile.mkstemp(prefix=".nes.prod.", dir=os.path.dirname(output))
os.close(fd)
try:
    with zipfile.ZipFile(temporary, "w", zipfile.ZIP_DEFLATED, compresslevel=9) as archive:
        for source, name, mode in sources:
            info = zipfile.ZipInfo(name, (1980, 1, 1, 0, 0, 0))
            info.compress_type = zipfile.ZIP_DEFLATED
            info.create_system = 3
            info.external_attr = mode << 16
            with open(source, "rb") as handle:
                archive.writestr(info, handle.read())
    os.replace(temporary, output)
finally:
    if os.path.exists(temporary):
        os.unlink(temporary)
PY

printf 'Created %s\n' "$NES_DIR/nes.prod"
