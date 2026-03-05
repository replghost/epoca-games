# epoca-games

Game ports and interactive demos running on [PolkaVM](https://github.com/nickvdp/polkavm) inside the [Epoca](https://github.com/replghost/epoca) workbench.

Each game is a sandboxed guest program that renders via the framebuffer API — the host presents pixel buffers and routes keyboard/mouse input.

## Games

| Game | Source | License |
|------|--------|---------|
| `doom` | [doomgeneric](https://github.com/ozkl/doomgeneric) port | GPL-2.0 |

## Building

DOOM example:

```bash
cd doom
cargo +nightly build -Z build-std=core,alloc \
  --target $(polkatool get-target-json-path --bitness 32) \
  --release
polkatool link target/riscv32-polkavm-fixed/release/doom-guest -o bundle/app.polkavm
cd bundle && zip -r ../doom.prod manifest.toml app.polkavm assets/
```

Requires `doom1.wad` (shareware) in `bundle/assets/`.

## Architecture

Each game port follows the same pattern:

1. Rust `no_std` shim (`src/main.rs`) — exports `init()` and `update()` via `polkavm_derive`
2. C/Rust game code linked in via `build.rs`
3. Host functions: `host_present_frame`, `host_poll_input`, `host_time_ms`, `host_asset_read`, `host_log`
4. Packaged as a `.prod` bundle (ZIP with `manifest.toml` + `app.polkavm` + `assets/`)

## License

Each game directory carries its own license matching the upstream source.
See individual directories for details.
