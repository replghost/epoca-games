# Product Runtime Examples

Reproducible sample Products using the runtime-aware App manifest v2 contract from `paritytech/host-rust-core` PR #507.

The repository is host-neutral. Artifacts are packaged once and are consumed unchanged by Epoca, Dotli, and other compatible Hosts. PolkaVM is the runtime; framebuffer, Tri2D, and WebGPU Raster are declared graphics profiles rather than executable kinds.

## Applications

| App                 | Profile         | Source license   | Packaged content                                   |
| ------------------- | --------------- | ---------------- | -------------------------------------------------- |
| `doom`              | `framebuffer`   | GPL-2.0          | Freedoom Phase 1, BSD-3-Clause                     |
| `quake`             | `framebuffer`   | GPL-2.0-or-later | LibreQuake Lite, BSD-3-Clause and GPL-2.0-or-later |
| `duke3d`            | `framebuffer`   | GPL-2.0-or-later | LibreSector, CC0-1.0                               |
| `nes`               | `framebuffer`   | GPL-2.0          | Reproducible controller-test ROM, MIT              |
| `egui-kitchen-sink` | `tri2d`         | MPL-2.0          | None                                               |
| `gpu-cube`          | `webgpu-raster` | MPL-2.0          | None                                               |
| `scene-lab`         | `webgpu-raster` | MPL-2.0          | Reproducible glTF-derived mesh                     |

A sample is included only when its executable source and every default asset are reproducible and legally redistributable. Commercial Doom IWADs and Duke Nukem 3D GRP files are never committed. Quake uses the same PolkaPorts engine revision deployed at `epocaquake.paseo`, rebuilt from source and packaged with the pinned LibreQuake Lite release.

## Layout

```text
apps/       App executables, manifests, icons, and per-app package commands
content/    Reproducible redistributable game-data packages
crates/     Shared no_std graphics and mesh crates used by sample Apps
scripts/    Deterministic build, CAR creation, and byte-equality verification
tests/      Source-level App manifest v2 contract tests
dist/       Generated CARs and release identities; never committed
```

Each app has one `manifest.json`. Its `bulletin-deploy.config.mjs` imports that object directly; it does not repeat `appVersion`, runtime, or capabilities. Artifact preparation serializes the object once and writes those exact bytes to `bundle/manifest.json`. The same bytes become the DotNS `executable` text record during publication.

## Prerequisites

- Node.js 22 or newer
- Python 3
- Rust `nightly-2025-05-10` and `nightly-2025-10-09`, both with `rust-src`
- `polkatool` 0.31 for the Rust/C samples
- Clang/LLVM with the RISC-V target for the C-based game ports; Quake bootstraps the pinned PolkaPorts `polkatool` 0.36 toolchain

```bash
rustup toolchain install nightly-2025-05-10 nightly-2025-10-09 --component rust-src
npm install
```

## Build

Build every app and produce deterministic CARs:

```bash
npm run build
npm run verify
```

Build a subset:

```bash
APP=doom,egui-kitchen-sink npm run build
```

Generated release metadata under `dist/<app>.release.json` records the CID, App version, manifest SHA-256, and CAR SHA-256.

## Verification contracts

`npm test` enforces source-level invariants:

- `$v: 2`, `kind: "app"`, and explicit PolkaVM ABI 1
- exactly one supported graphics profile
- no legacy `$schema`, `modalities`, or `contentSlots`
- deployment config imports the manifest as its only executable version source
- commercial game data is not a committed dependency

`npm run verify` reopens every generated CAR and enforces:

- one CAR root
- safe UnixFS paths
- embedded `manifest.json` byte equality with the external executable record
- executable presence
- release digest freshness
- required asset licenses and attribution
- Host compatibility expectations

The Doom, Quake, and Duke framebuffer samples launch in both Epoca and Dotli. NES launches in Dotli; Epoca's current native compiler traps during NES initialization, and `dist/compatibility.json` records that observed host boundary. Tri2D and WebGPU Raster samples launch in Epoca; Dotli must skip them as unsupported profiles until it implements those contracts, never substitute framebuffer.

Run the native Epoca process-host smoke matrix against every supported profile:

```bash
EPOCA_PVM_HOST=/absolute/path/to/epoca-pvm-host npm run smoke:epoca
```

The smoke runner validates non-empty framebuffer output, a checked Tri2D stream, and a negotiated WebGPU Raster batch from the generated application binaries.

Run the framebuffer artifacts through Dotli's browser-host integration:

```bash
DOTLI_REPO=/absolute/path/to/dotli npm run smoke:dotli
```

## Publishing

Publishing is manual. Build and verify first, then use the app's `bulletin-deploy.config.mjs` with a Bulletin Deploy release that supports App manifest v2. CI does not hold deployment credentials or mutate DotNS.

## Licensing

See `LICENSE`, each app or content directory, and the notices embedded into generated bundles. User-supplied retail data may be used for local testing through the documented environment overrides but must not be published by these sample workflows.
