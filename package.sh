#!/bin/bash
# Builds the RustRouter release packages into dist/ (command line tool `rustrouter` and
# graphical user interface `rustrouter-gui`):
#   Windows x86_64  .zip  (rustrouter.exe, rustrouter-gui.exe)
#   Linux x86_64    .deb and .rpm (with desktop entry) and .tar.gz, glibc >= 2.17
#   macOS           .dmg  (RustRouter.app and the command line tool, universal binaries for
#                   x86_64 and arm64, not code signed)
#
# Requires in PATH: zig, cargo-zigbuild, cargo-deb, cargo-generate-rpm, zip, mkfs.hfsplus and
# the `hfsplus` and `dmg` tools of libdmg-hfsplus (https://github.com/mozilla/libdmg-hfsplus).
# Rust targets: rustup target add x86_64-unknown-linux-gnu x86_64-pc-windows-gnu \
#   x86_64-apple-darwin aarch64-apple-darwin
# The macOS build needs a macOS SDK (frameworks for the GUI): SDKROOT, or the newest
# ~/.local/share/macos-sdk/MacOSX*.sdk.
set -euo pipefail
cd "$(dirname "$0")"

VERSION=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)
export CARGO_TARGET_DIR=target-cross
DIST=dist
STAGE=$CARGO_TARGET_DIR/stage
rm -rf "$DIST" "$STAGE"
mkdir -p "$DIST" "$STAGE"

if [[ -z "${SDKROOT:-}" ]]; then
  SDKROOT=$(ls -d "$HOME"/.local/share/macos-sdk/MacOSX*.sdk 2>/dev/null | sort -V | tail -1 || true)
fi
if [[ -z "$SDKROOT" || ! -d "$SDKROOT" ]]; then
  echo "macOS SDK not found: set SDKROOT or extract one to ~/.local/share/macos-sdk/" >&2
  exit 1
fi

echo "== build"
cargo zigbuild --release --target x86_64-unknown-linux-gnu.2.17
cargo zigbuild --release --target x86_64-pc-windows-gnu
SDKROOT="$SDKROOT" cargo zigbuild --release --target universal2-apple-darwin

LINUX_DIR=$CARGO_TARGET_DIR/x86_64-unknown-linux-gnu/release
WIN_DIR=$CARGO_TARGET_DIR/x86_64-pc-windows-gnu/release
MAC_DIR=$CARGO_TARGET_DIR/universal2-apple-darwin/release

echo "== linux deb/rpm/tar.gz"
cargo deb --no-build --target x86_64-unknown-linux-gnu -o "$DIST/"
cargo generate-rpm --target x86_64-unknown-linux-gnu --target-dir "$CARGO_TARGET_DIR" -o "$DIST/"
L=$STAGE/rustrouter-$VERSION-linux-x86_64
mkdir -p "$L"
cp "$LINUX_DIR/rustrouter" "$LINUX_DIR/rustrouter-gui" README.md LICENSE \
  assets/rustrouter.desktop assets/rustrouter.png "$L/"
tar -C "$STAGE" -czf "$DIST/rustrouter-$VERSION-linux-x86_64.tar.gz" "$(basename "$L")"

echo "== windows zip"
W=$STAGE/rustrouter-$VERSION-windows-x86_64
mkdir -p "$W"
cp "$WIN_DIR/rustrouter.exe" "$WIN_DIR/rustrouter-gui.exe" README.md LICENSE "$W/"
(cd "$STAGE" && zip -qr "../../$DIST/$(basename "$W").zip" "$(basename "$W")")

echo "== macos dmg"
M=$STAGE/mac
APP=$M/RustRouter.app/Contents
mkdir -p "$APP/MacOS" "$APP/Resources"
cp "$MAC_DIR/rustrouter-gui" "$APP/MacOS/rustrouter-gui"
cp assets/rustrouter.icns "$APP/Resources/rustrouter.icns"
cat > "$APP/Info.plist" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleName</key><string>RustRouter</string>
  <key>CFBundleDisplayName</key><string>RustRouter</string>
  <key>CFBundleIdentifier</key><string>org.rustrouter.RustRouter</string>
  <key>CFBundleVersion</key><string>$VERSION</string>
  <key>CFBundleShortVersionString</key><string>$VERSION</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleExecutable</key><string>rustrouter-gui</string>
  <key>CFBundleIconFile</key><string>rustrouter.icns</string>
  <key>LSMinimumSystemVersion</key><string>10.13</string>
  <key>NSHighResolutionCapable</key><true/>
  <key>CFBundleDocumentTypes</key>
  <array>
    <dict>
      <key>CFBundleTypeName</key><string>Specctra DSN design</string>
      <key>CFBundleTypeExtensions</key><array><string>dsn</string></array>
      <key>CFBundleTypeRole</key><string>Viewer</string>
    </dict>
  </array>
</dict>
</plist>
EOF
cp "$MAC_DIR/rustrouter" README.md LICENSE "$M/"
ln -s /Applications "$M/Applications"
cat > "$M/INSTALL.txt" <<'EOF'
RustRouter for macOS (universal binaries: Intel and Apple Silicon)

GUI: drag RustRouter.app to Applications.
Command line tool:
  sudo cp /Volumes/RustRouter/rustrouter /usr/local/bin/

The app and the tool are not code signed. If macOS blocks them ("damaged" or "unidentified
developer"), remove the quarantine flag:
  xattr -dr com.apple.quarantine /Applications/RustRouter.app
  xattr -d com.apple.quarantine /usr/local/bin/rustrouter

Usage of the command line tool:
  rustrouter -de board.dsn -do board.ses
EOF
IMG=$STAGE/rustrouter.hfs
SIZE_KB=$(( $(du -sk "$M" | cut -f1) + 4096 ))
dd if=/dev/zero of="$IMG" bs=1k count="$SIZE_KB" status=none
mkfs.hfsplus -v RustRouter "$IMG" >/dev/null
hfsplus "$IMG" -s clone_link addall "$M" >/dev/null
hfsplus "$IMG" chmod 755 /rustrouter >/dev/null
hfsplus "$IMG" chmod 755 /RustRouter.app/Contents/MacOS/rustrouter-gui >/dev/null
dmg build "$IMG" "$DIST/rustrouter-$VERSION-macos-universal.dmg" >/dev/null

(cd "$DIST" && sha256sum -- * > SHA256SUMS)
ls -la "$DIST"
