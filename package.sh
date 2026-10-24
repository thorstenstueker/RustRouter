#!/bin/bash
# Builds the RustRouter release packages into dist/:
#   Windows x86_64  .zip  (rustrouter.exe)
#   Linux x86_64    .deb and .rpm (glibc >= 2.17) and .tar.gz
#   macOS           .dmg  (universal binary for x86_64 and arm64, not code signed)
#
# Requires in PATH: zig, cargo-zigbuild, cargo-deb, cargo-generate-rpm, zip, mkfs.hfsplus and
# the `hfsplus` and `dmg` tools of libdmg-hfsplus (https://github.com/mozilla/libdmg-hfsplus).
# Rust targets: rustup target add x86_64-unknown-linux-gnu x86_64-pc-windows-gnu \
#   x86_64-apple-darwin aarch64-apple-darwin
set -euo pipefail
cd "$(dirname "$0")"

VERSION=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)
export CARGO_TARGET_DIR=target-cross
DIST=dist
STAGE=$CARGO_TARGET_DIR/stage
rm -rf "$DIST" "$STAGE"
mkdir -p "$DIST" "$STAGE"

echo "== build"
cargo zigbuild --release --target x86_64-unknown-linux-gnu.2.17
cargo zigbuild --release --target x86_64-pc-windows-gnu
cargo zigbuild --release --target universal2-apple-darwin

LINUX_BIN=$CARGO_TARGET_DIR/x86_64-unknown-linux-gnu/release/rustrouter
WIN_BIN=$CARGO_TARGET_DIR/x86_64-pc-windows-gnu/release/rustrouter.exe
MAC_BIN=$CARGO_TARGET_DIR/universal2-apple-darwin/release/rustrouter

echo "== linux deb/rpm/tar.gz"
cargo deb --no-build --target x86_64-unknown-linux-gnu -o "$DIST/"
cargo generate-rpm --target x86_64-unknown-linux-gnu --target-dir "$CARGO_TARGET_DIR" -o "$DIST/"
mkdir -p "$STAGE/rustrouter-$VERSION-linux-x86_64"
cp "$LINUX_BIN" README.md LICENSE "$STAGE/rustrouter-$VERSION-linux-x86_64/"
tar -C "$STAGE" -czf "$DIST/rustrouter-$VERSION-linux-x86_64.tar.gz" "rustrouter-$VERSION-linux-x86_64"

echo "== windows zip"
mkdir -p "$STAGE/rustrouter-$VERSION-windows-x86_64"
cp "$WIN_BIN" README.md LICENSE "$STAGE/rustrouter-$VERSION-windows-x86_64/"
(cd "$STAGE" && zip -qr "../../$DIST/rustrouter-$VERSION-windows-x86_64.zip" "rustrouter-$VERSION-windows-x86_64")

echo "== macos dmg"
MAC_DIR=$STAGE/mac
mkdir -p "$MAC_DIR"
cp "$MAC_BIN" README.md LICENSE "$MAC_DIR/"
cat > "$MAC_DIR/INSTALL.txt" <<'EOF'
RustRouter for macOS (universal binary: Intel and Apple Silicon)

Install the command line tool, e.g.:
  sudo cp /Volumes/RustRouter/rustrouter /usr/local/bin/

The binary is not code signed. If macOS blocks it, remove the quarantine flag:
  xattr -d com.apple.quarantine /usr/local/bin/rustrouter

Usage:
  rustrouter -de board.dsn -do board.ses
EOF
IMG=$STAGE/rustrouter.hfs
SIZE_KB=$(( $(du -sk "$MAC_DIR" | cut -f1) + 2048 ))
dd if=/dev/zero of="$IMG" bs=1k count="$SIZE_KB" status=none
mkfs.hfsplus -v RustRouter "$IMG" >/dev/null
hfsplus "$IMG" addall "$MAC_DIR" >/dev/null
hfsplus "$IMG" chmod 755 /rustrouter >/dev/null
dmg build "$IMG" "$DIST/rustrouter-$VERSION-macos-universal.dmg" >/dev/null

(cd "$DIST" && sha256sum -- * > SHA256SUMS)
ls -la "$DIST"
