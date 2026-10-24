# RustRouter

RustRouter is a Rust port of the [Freerouting](https://github.com/freerouting/freerouting)
PCB autorouter. The routing core (board model, search trees, shove/pull-tight, maze search
autorouter, fanout, batch pipeline with fallbacks and the optimizer) is a faithful port of
the Java code, so the results match the Java version. See `PORTING.md` for the conventions.

## Usage

```
rustrouter -de board.dsn -do board.ses [-mp <max passes>] [-mt <threads>]
           [-oit <optimizer improvement %>] [-jt <HH:MM:SS>]
           [--no-optimizer] [--no-fanout] [--no-router]
```

The input is a Specctra DSN design (as exported by KiCad and other CAD tools), the output a
Specctra SES session that can be imported back into the CAD tool. Set `RUST_LOG=debug` for
more output.

## Building

```
cargo build --release              # native
./build-all.sh                     # Linux x86_64/aarch64, Windows x86_64, macOS x86_64/arm64
```

`build-all.sh` uses [cargo-zigbuild](https://github.com/rust-cross/cargo-zigbuild) with zig as
cross linker; install the targets with
`rustup target add x86_64-unknown-linux-gnu aarch64-unknown-linux-gnu x86_64-pc-windows-gnu x86_64-apple-darwin aarch64-apple-darwin`.
The macOS binaries are not code signed.

### Release packages

```
./package.sh
```

writes to `dist/`: a Windows x86_64 `.zip`, Linux x86_64 `.deb`, `.rpm` and `.tar.gz`
(glibc >= 2.17) and a macOS `.dmg` with a universal binary (x86_64 + arm64, unsigned), plus
`SHA256SUMS`. Besides zig and cargo-zigbuild it needs `cargo install cargo-deb
cargo-generate-rpm`, `zip`, `mkfs.hfsplus` (hfsplus-tools) and the `hfsplus` and `dmg` tools of
[libdmg-hfsplus](https://github.com/mozilla/libdmg-hfsplus) to build the DMG on Linux.

## Status

* Headless DSN → SES routing (fanout, autorouter, optimizer) — done.
* Specctra DSN/SES/rules I/O — done, verified against Java on all fixtures.
* GUI — not yet ported.
