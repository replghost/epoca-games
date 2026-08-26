#!/usr/bin/env bash
set -euo pipefail

fail() {
  printf 'package.sh: %s\n' "$*" >&2
  exit 1
}

for tool in autoreconf cargo clang clang++ curl git llvm-ar llvm-ranlib make patch python3 sha256sum tar; do
  command -v "$tool" >/dev/null 2>&1 || fail "required tool '$tool' was not found in PATH"
done

QUAKE_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd "$QUAKE_DIR/../.." && pwd)"
COMMIT="5d7b30ee88ced16491110c79118423ab3913efb0"
ARCHIVE="$ROOT/.tmp/sources/polkaports-$COMMIT.tar.gz"
SOURCE_DIR="$ROOT/.tmp/build/quake/polkaports-$COMMIT"
URL="${POLKAPORTS_URL:-https://codeload.github.com/paritytech/polkaports/tar.gz/$COMMIT}"
ARCHIVE_SHA256="f669242e42218adf6dbcb2cc42a5bbb26a518d1ff2964e28851aaa13c811c753"
BUNDLE_DIR="$QUAKE_DIR/bundle"

PVM_CLANG="${PVM_CLANG:-$(command -v clang)}"
PVM_CLANGXX="${PVM_CLANGXX:-$(command -v clang++)}"
PVM_LLVM_AR="${PVM_LLVM_AR:-$(command -v llvm-ar)}"
PVM_LLVM_RANLIB="${PVM_LLVM_RANLIB:-$(command -v llvm-ranlib)}"
if [[ -z "${PVM_LLD:-}" ]]; then
  LLD_ARCHIVE="$ROOT/.tmp/sources/lld-22.1.8-1-x86_64.pkg.tar.zst"
  LLD_ROOT="$ROOT/.tmp/tools/lld-22.1.8"
  LLD_URL="https://archive.archlinux.org/packages/l/lld/lld-22.1.8-1-x86_64.pkg.tar.zst"
  mkdir -p "$(dirname "$LLD_ARCHIVE")" "$LLD_ROOT"
  if [[ ! -f "$LLD_ARCHIVE" ]]; then
    curl -L --fail --retry 3 -o "$LLD_ARCHIVE" "$LLD_URL"
  fi
  printf '%s  %s\n' "3032282365f09110922c8df0ef6afc56be2427b9396044334e22dced1aadc31c" "$LLD_ARCHIVE" |
    sha256sum --check --status -
  if [[ ! -x "$LLD_ROOT/usr/bin/ld.lld" ]]; then
    tar -xf "$LLD_ARCHIVE" -C "$LLD_ROOT"
  fi
  PVM_LLD="$LLD_ROOT/usr/bin/ld.lld"
fi
for tool in "$PVM_CLANG" "$PVM_CLANGXX" "$PVM_LLD" "$PVM_LLVM_AR" "$PVM_LLVM_RANLIB"; do
  [[ -n "$tool" && -x "$tool" ]] || fail "required LLVM tool was not found: $tool"
done
LLVM_BIN="$(dirname "$PVM_LLD")"
LLD_LIBRARY_DIR="$(dirname "$(dirname "$PVM_LLD")")/lib"
if [[ -d "$LLD_LIBRARY_DIR" ]]; then
  export LD_LIBRARY_PATH="$LLD_LIBRARY_DIR${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
fi

python3 - "$ARCHIVE" "$URL" "$ARCHIVE_SHA256" "$SOURCE_DIR" <<'PY'
import hashlib
import os
import shutil
import sys
import tarfile
import tempfile
import urllib.request

archive_path, url, expected_sha256, source_dir = sys.argv[1:]
os.makedirs(os.path.dirname(archive_path), exist_ok=True)
if not os.path.exists(archive_path):
    descriptor, temporary = tempfile.mkstemp(
        prefix=".polkaports.", suffix=".tar.gz", dir=os.path.dirname(archive_path)
    )
    os.close(descriptor)
    try:
        urllib.request.urlretrieve(url, temporary)
        os.replace(temporary, archive_path)
    finally:
        if os.path.exists(temporary):
            os.unlink(temporary)
with open(archive_path, "rb") as handle:
    actual_sha256 = hashlib.sha256(handle.read()).hexdigest()
if actual_sha256 != expected_sha256:
    raise SystemExit(
        f"PolkaPorts archive SHA-256 mismatch: expected {expected_sha256}, got {actual_sha256}"
    )

marker = os.path.join(source_dir, ".epoca-patched")
if not os.path.exists(marker):
    shutil.rmtree(source_dir, ignore_errors=True)
    parent = os.path.dirname(source_dir)
    os.makedirs(parent, exist_ok=True)
    staging = tempfile.mkdtemp(prefix=".polkaports-source.", dir=parent)
    try:
        with tarfile.open(archive_path, "r:gz") as archive:
            roots = {member.name.split("/", 1)[0] for member in archive.getmembers()}
            if len(roots) != 1:
                raise SystemExit(f"PolkaPorts archive has unexpected roots: {sorted(roots)}")
            archive.extractall(staging, filter="data")
        extracted = os.path.join(staging, roots.pop())
        os.replace(extracted, source_dir)
    finally:
        shutil.rmtree(staging, ignore_errors=True)
PY

if [[ ! -f "$SOURCE_DIR/.epoca-patched" ]]; then
  (
    cd "$SOURCE_DIR"
    patch --dry-run --silent -p1 -i "$QUAKE_DIR/polkaports.patch"
    patch --silent -p1 -i "$QUAKE_DIR/polkaports.patch"
  )
  touch "$SOURCE_DIR/.epoca-patched"
fi

if [[ ! -f "$SOURCE_DIR/sysroot-polkavm/.epoca-ready" ]]; then
  (
    cd "$SOURCE_DIR"
    CC="$PVM_CLANG" \
      CXX="$PVM_CLANGXX" \
      LLD="$PVM_LLD" \
      AR="$PVM_LLVM_AR" \
      RANLIB="$PVM_LLVM_RANLIB" \
      ./setup.sh
    touch "$SOURCE_DIR/sysroot-polkavm/.epoca-ready"
  )
fi

export PATH="$SOURCE_DIR/sysroot-polkavm/bin:$LLVM_BIN:$PATH"
export POLKAPORTS_SYSROOT="$SOURCE_DIR/sysroot-polkavm"
export POLKAPORTS_SUFFIX="polkavm"
make -C "$SOURCE_DIR/apps/quake" clean
make -C "$SOURCE_DIR/apps/quake" -j "${JOBS:-$(nproc)}" LDFLAGS=-fuse-ld=lld

GUEST="$SOURCE_DIR/apps/quake/output/quake.polkavm"
[[ -f "$GUEST" ]] || fail "Quake build did not produce $GUEST"
"$ROOT/content/librequake/package.sh"

rm -rf "$BUNDLE_DIR"
mkdir -p "$BUNDLE_DIR/LICENSES" "$BUNDLE_DIR/SOURCES"
cp "$GUEST" "$BUNDLE_DIR/app.polkavm"
cp "$QUAKE_DIR/manifest.json" "$BUNDLE_DIR/manifest.json"
cp "$QUAKE_DIR/icon.png" "$BUNDLE_DIR/icon.png"
cp "$QUAKE_DIR/autoexec.cfg" "$BUNDLE_DIR/autoexec.cfg"
cp "$QUAKE_DIR/LICENSE" "$BUNDLE_DIR/LICENSES/Quake-GPL-2.0-or-later.txt"
cp "$QUAKE_DIR/SOURCES.json" "$BUNDLE_DIR/SOURCES/Quake.json"
cp "$QUAKE_DIR/polkaports.patch" "$BUNDLE_DIR/SOURCES/polkaports.patch"
cp "$ROOT/content/librequake/bundle/pak0.pak" "$BUNDLE_DIR/pak0.pak"
cp "$ROOT/content/librequake/bundle/pak1.pak" "$BUNDLE_DIR/pak1.pak"
cp "$ROOT/content/librequake/bundle/COPYING-BSD-3-Clause.txt" "$BUNDLE_DIR/LICENSES/LibreQuake-BSD-3-Clause.txt"
cp "$ROOT/content/librequake/bundle/COPYING-GPL-2.0.txt" "$BUNDLE_DIR/LICENSES/LibreQuake-GPL-2.0.txt"
cp "$ROOT/content/librequake/bundle/CREDITS.txt" "$BUNDLE_DIR/LICENSES/LibreQuake-CREDITS.txt"
cp "$ROOT/content/librequake/bundle/LICENSE-INFO.txt" "$BUNDLE_DIR/LICENSES/LibreQuake-LICENSE-INFO.txt"
cp "$ROOT/content/librequake/bundle/SOURCES.json" "$BUNDLE_DIR/SOURCES/LibreQuake.json"

printf 'Built %s\n' "$BUNDLE_DIR"
