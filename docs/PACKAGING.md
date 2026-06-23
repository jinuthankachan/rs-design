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

## Bundle shape (measured at CP1 — [daemon-packaging.md](./spikes/daemon-packaging.md))

The Node daemon ships as a Tauri resource bundle, **not** a single-file executable. The daemon is
**`tsc` output + a pruned prod `node_modules`** (not a single esbuild file; not `pkg` — Open Q#2
resolved to **bundled runtime**). Sizes are measured for linux-x64, Node 24.16:

```
bundle/
├── node            # bundled Node 24 linux-x64, stripped — ~101 M (118 M unstripped)
├── daemon/         # pnpm deploy of apps/daemon (tsc dist/ + pruned node_modules) — ~71 M pruned
│   ├── dist/           # tsc JS (~5.8 M; .d.ts + .map stripped)
│   └── node_modules/   # pruned prod deps incl. better-sqlite3 (.node) — ~65 M
├── web/            # Next static export (out/) served via Tauri asset protocol — ~51 M
└── content/        # skills/ + design-systems/ + frames/ (per-folder LICENSE files preserved)
```

Daemon sidecar (node + daemon) ≈ **172 M**. The CP6 `scripts/build-daemon-bundle.sh` prune rules
(measured to take the daemon 145 M → 71 M): drop `node-pty/prebuilds/{win32-*,darwin-*}` (−58 M
dead cross-platform weight), `better-sqlite3/{deps,src,build/Release/obj}` (−10 M build-only),
`dist/**/*.{d.ts,map}` (−6 M), and `strip bin/node` (−17 M).

The supervisor's `BundledLauncher` (CP6) resolves these via Tauri `resolve_resource` and spawns
`node daemon/dist/cli.js daemon start --headless --port <ephemeral> --host 127.0.0.1` with an
explicit env (`OD_PORT`, `OD_BIND_HOST`, `OD_DATA_DIR`, `OD_RESOURCE_ROOT` **+ `OD_INSTALLATION_DIR`**
— the latter is required for `OD_RESOURCE_ROOT`'s safe-base check — plus PATH/`*_BIN` at CP5).
**Verified at CP1:** this boots and serves `/api/skills` under a fully PATH-stripped env.

## Native modules (the main reason V1 isn't single-file)

Two `.node` addons. **`better-sqlite3`** is built and **loads clean under the bundled Node**
(standard libs only, GLIBC_2.28 floor — fine on Ubuntu 24.04's 2.39). **`node-pty` has no Linux
binary by default** — pnpm skips its build script, so CP6 must `pnpm approve-builds` / compile it
for linux-x64 to enable the terminal, or ship without it (the daemon boots fine regardless, and
`/api/terminal` is out of V1 smoke scope). Full RPATH/glibc rigor under the bundled (not dev) Node
is CP1-Task4.

## Licensing guardrail
When vendoring/bundling content, **copy per-folder `LICENSE`/attribution files too** and keep
attribution intact (e.g. `guizang-ppt`). See `NOTICE`.

## Targets
- `.deb` — declares webkit2gtk runtime + patchelf-related deps.
- `.AppImage` — self-contained; needs `patchelf` at build time.
