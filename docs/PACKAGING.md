# Packaging & Build Prerequisites

Target: **Ubuntu 24.04 LTS, linux-x64** (arm64 deferred past V1). Output: `.deb` + `.AppImage`
that launch with **zero Node/pnpm** on the user's machine.

## Build prerequisites (developer / CI machine)

These are **not** installed by default on this box and must be installed before building the
Tauri app or running the CP1 spikes:

```bash
# System libraries for Tauri v2 + WebKitGTK on Ubuntu 24.04, plus AppImage tooling
sudo apt install -y \
  libwebkit2gtk-4.1-dev \
  libjavascriptcoregtk-4.1-dev \
  libsoup-3.0-dev \
  patchelf \
  libssl-dev \
  build-essential curl file

# Tauri CLI (or add @tauri-apps/cli as a pnpm dev-dependency)
cargo install tauri-cli --version '^2'
```

Already present on this machine: Rust 1.96, Node 24, pnpm 11, the WebKitGTK 4.1 **runtime**.
The non-Tauri crates (`crates/od-*`) build without the above; only `src-tauri` and the spikes need them.

> The placeholder app icon at `src-tauri/icons/icon.png` is a 1×1 PNG. Before bundling (CP6),
> generate the real icon set with `cargo tauri icon <source.png>`.

## Bundle shape (decided from upstream; confirm at CP1)

The Node daemon ships as a Tauri resource bundle, **not** a single-file executable:

```
bundle/
├── node            # pinned Node 24 linux-x64 runtime binary
├── daemon/         # esbuild output of apps/daemon (reuses upstream apps/packaged config)
│   └── node_modules/   # pruned prod deps incl. prebuilt better-sqlite3 + node-pty (.node)
├── web/            # Next static export (out/) served via Tauri asset protocol
└── content/        # skills/ + design-systems/ (with per-folder LICENSE files preserved)
```

The supervisor's `BundledLauncher` (CP6) resolves these via Tauri `resolve_resource` and spawns
`node daemon/...` with an explicit env (`OD_PORT`, `OD_BIND_HOST`, `OD_DATA_DIR`, PATH, `*_BIN`).

## Native modules (the main reason V1 isn't single-file)

Two `.node` addons must be shipped prebuilt for linux-x64 and load under the bundled Node:
`better-sqlite3` and `node-pty`. CP1 confirms RPATH/glibc compatibility on Ubuntu 24.04.

## Licensing guardrail
When vendoring/bundling content, **copy per-folder `LICENSE`/attribution files too** and keep
attribution intact (e.g. `guizang-ppt`). See `NOTICE`.

## Targets
- `.deb` — declares webkit2gtk runtime + patchelf-related deps.
- `.AppImage` — self-contained; needs `patchelf` at build time.
