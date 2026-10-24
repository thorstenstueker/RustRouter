#!/bin/bash
# Builds RustRouter release binaries for Linux, Windows and macOS with cargo-zigbuild.
# Requires: rustup targets (see below), zig and cargo-zigbuild in PATH.
# The macOS binaries are not signed.
set -euo pipefail
cd "$(dirname "$0")"
TARGETS=(
  x86_64-unknown-linux-gnu
  aarch64-unknown-linux-gnu
  x86_64-pc-windows-gnu
  x86_64-apple-darwin
  aarch64-apple-darwin
)
mkdir -p dist
for t in "${TARGETS[@]}"; do
  echo "== $t"
  cargo zigbuild --release --target "$t"
  ext=""
  [[ "$t" == *windows* ]] && ext=".exe"
  cp "target/$t/release/rustrouter$ext" "dist/rustrouter-$t$ext"
done
# Universal macOS binary (optional, needs llvm-lipo or lipo)
if command -v lipo >/dev/null 2>&1; then
  lipo -create -output dist/rustrouter-universal-apple-darwin \
    dist/rustrouter-x86_64-apple-darwin dist/rustrouter-aarch64-apple-darwin
fi
ls -la dist
