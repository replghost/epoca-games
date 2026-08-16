# epoca-games

Game ports and interactive demos running on [PolkaVM](https://github.com/nickvdp/polkavm) inside the [Epoca](https://github.com/replghost/epoca) workbench.

Each game is a sandboxed guest program that renders via the framebuffer API — the host presents pixel buffers and routes keyboard/mouse input.

## Games

| Game | Source | License |
|------|--------|---------|
| `doom` | [doomgeneric](https://github.com/ozkl/doomgeneric) port | GPL-2.0 |

## Building

The Doom product requires Rust `nightly-2025-05-10` with `rust-src`,
`polkatool` 0.31, Python 3, and a Clang/LLVM toolchain with the RISC-V target.
On macOS, `package.sh` uses Homebrew LLVM automatically; elsewhere set
`PVM_CLANG` if the RISC-V-capable Clang is not on `PATH`:

```bash
./doom/package.sh
```

This builds and links `app.polkavm`, then creates an engine-only
`doom/doom.prod` containing `manifest.json` and `app.polkavm`. For a local
development override, set `DOOM_IWAD` to a lawfully obtained IWAD. The
override is mounted at the same `game/doom.wad` path used by Epoca's Content
resolver, but is never required or included in the network deployment.

Freedoom Phase 1 is packaged independently from its official 0.13.0 release:

```bash
FREEDOOM_ARCHIVE=/absolute/path/to/freedoom-0.13.0.zip ./freedoom/package.sh
```

Without `FREEDOOM_ARCHIVE`, the script downloads the official release. It
verifies the release SHA-256, IWAD structure and item SHA-256 before creating
`freedoom/bundle`.

## Architecture

Each game port follows the same pattern:

1. Rust `no_std` shim (`src/main.rs`) — exports `init()` and `update()` via `polkavm_derive`
2. C/Rust game code linked in via `build.rs`
3. Host functions: `host_present_frame`, `host_poll_input`, `host_time_ms`, `host_asset_read`, `host_log`
4. Packaged with an experimental `manifest.json` declaring the PolkaVM runtime, framebuffer ABI, General Input handlers, and immutable Content slots

## License

The DOOM port source is derived from
[doomgeneric](https://github.com/ozkl/doomgeneric) and remains licensed under
GPL-2.0; see `doom/LICENSE`. Freedoom content is distributed separately under
BSD-3-Clause with its copyright notice, license conditions, warranty
disclaimer, and contributor credits preserved in the generated package.
Commercial Doom IWADs are not distributed and may only be supplied by the
user as a local Content override.
