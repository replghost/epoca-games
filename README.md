# epoca-games

Game ports and interactive demos running on [PolkaVM](https://github.com/nickvdp/polkavm) inside the [Epoca](https://github.com/replghost/epoca) workbench.

Each game is a sandboxed guest program that renders via the framebuffer API — the host presents pixel buffers and routes keyboard/mouse input.

## Games

| Game | Source | License |
|------|--------|---------|
| `doom` | [doomgeneric](https://github.com/ozkl/doomgeneric) port | GPL-2.0 |

## Building

The DOOM product requires Rust `nightly-2025-05-10` with `rust-src`,
`polkatool` 0.31, Python 3, and a Clang/LLVM toolchain with the RISC-V target.
On macOS, `package.sh` uses Homebrew LLVM automatically; elsewhere set
`PVM_CLANG` if the RISC-V-capable Clang is not on `PATH`. A lawfully obtained
DOOM IWAD is also required. Game data is not distributed with this
repository. From the repository root, supply the IWAD explicitly:

```bash
DOOM_IWAD=/absolute/path/to/doom1.wad ./doom/package.sh
```

This builds and links `app.polkavm`, then creates `doom/doom.prod` containing
`pvm.json`, `app.polkavm`, and the supplied IWAD as `assets/doom1.wad`.

## Architecture

Each game port follows the same pattern:

1. Rust `no_std` shim (`src/main.rs`) — exports `init()` and `update()` via `polkavm_derive`
2. C/Rust game code linked in via `build.rs`
3. Host functions: `host_present_frame`, `host_poll_input`, `host_time_ms`, `host_asset_read`, `host_log`
4. Packaged as a `.prod` ZIP bundle with a verified `pvm.json` descriptor, `app.polkavm`, and assets

## License

The DOOM port source is derived from
[doomgeneric](https://github.com/ozkl/doomgeneric) and remains licensed under
GPL-2.0; see `doom/LICENSE`. IWAD game data is separate from the GPL-licensed
port source, is not distributed here, and must be supplied by the user from a
lawful source.
