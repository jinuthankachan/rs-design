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

## Native modules (the main reason V1 isn't single-file) — verified at CP1-Task4

Two `.node` addons; **both load AND run under the bundled, stripped Node 24** in a fully
PATH-stripped env ([native-addons.md](./spikes/native-addons.md)). **Neither carries an
RPATH/RUNPATH → no `patchelf`/relocation for the addons** (the `patchelf` prereq above is for
Tauri/AppImage's WebKit bundling, not these `.node` files). ABI `NODE_MODULE_VERSION` **137** — any
Node 24.x loads them, so pin *a* Node 24.x.

| addon | status | glibc / GLIBCXX floor | notes |
|---|---|---|---|
| `better-sqlite3` | ✅ real SQL, SQLite 3.53.1 | 2.29 / 3.4.20 | **SQLite statically linked** (no `libsqlite3` dep); keep prebuilt `.node` as-is |
| `node-pty` | ✅ real PTY (after compile) | 2.34 / 3.4.22 | **no Linux prebuild ships + pnpm skips its build script** |

Ubuntu 24.04 provides glibc **2.39** / GLIBCXX **3.4.33**, clearing both. The bundle's glibc floor
is **2.34**, set by the host that compiles node-pty (→ Ubuntu 22.04+); building on the 24.04 runner
is fine for the V1 target. **CP6 must compile node-pty for linux-x64** (`pnpm approve-builds` then
`node-gyp rebuild` → 75 K `pty.node`, ~1 s) to enable the terminal — or ship without it (the daemon
boots regardless; `/api/terminal` is out of V1 smoke scope). Add a **verify-after-bundle load-test**
(`require()` both addons under the bundled node, one op each) to CP6/CP7 to catch ABI/glibc/missing
-binary regressions.

## Licensing guardrail
When vendoring/bundling content, **copy per-folder `LICENSE`/attribution files too** and keep
attribution intact (e.g. `guizang-ppt`). See `NOTICE`.

## Targets
- `.deb` — declares webkit2gtk runtime + patchelf-related deps.
- `.AppImage` — self-contained; needs `patchelf` at build time.

## CP6 build path (implemented)

The package is built in two stages: assemble the daemon resource tree, then run
the Tauri bundler. Three scripts produce the resource tree (all idempotent;
output staged under `src-tauri/bundle-resources/runtime/`, gitignored):

```bash
# 1. Pin + place the Node 24 runtime (downloads, sha256-verifies, strips, records
#    NODE_LICENSE). Auto-invoked by step 2 if absent.
scripts/fetch-node-runtime.sh

# 2. Assemble the runtime layout: pnpm deploy + prune the daemon, compile node-pty
#    for linux-x64, copy web out/ + content (LICENSEs preserved). Use SKIP_BUILD=1
#    when apps/daemon/dist + apps/web/out are already built in the submodule.
scripts/build-daemon-bundle.sh           # ~363 M runtime/ (node 101M, daemon 72M, web 51M, content 58M)

# 3. Post-bundle gate: load-test both .node addons under the bundled Node
#    (env -i / empty PATH). CP6/CP7 regression catch for ABI/glibc/missing-binary.
scripts/verify-bundle.sh

# 4. Bundle into .deb + .AppImage (ships runtime/ via bundle.resources → runtime).
cargo tauri build
```

**Runtime layout** (one root makes the daemon's own path resolution line up —
`runtime/apps/daemon/dist` ⇒ `PROJECT_ROOT = runtime`, `STATIC_DIR =
runtime/apps/web/out`, with `OD_RESOURCE_ROOT = OD_INSTALLATION_DIR = runtime`):

```
<resource_dir>/runtime/
  node  NODE_LICENSE  NODE_VERSION
  apps/daemon/{dist,node_modules,package.json,bin}    # pnpm deploy, pruned
  apps/web/out/                                        # Next static export
  skills/ design-systems/ design-templates/ craft/ prompt-templates/ plugins/
  frames/  community-pets/                             # remapped from assets/
```

**Resources, not `externalBin`.** We ship the Node binary inside `runtime/` as a
resource and the `BundledLauncher` **materializes it into the writable XDG data
dir + `chmod +x`** at launch — because an AppImage mounts its resources read-only
and Tauri's resource copy can drop the exec bit. We spawn `node` ourselves (not
via Tauri's shell sidecar), so `externalBin` buys nothing. The `.node` addons are
`dlopen`-ed and need no exec bit, so only `node` is materialized.

**Verified at CP6 (headless):** the assembled bundle boots under `env -i` / empty
`PATH` and serves 155 skills + 150 design-systems + `index.html` (200) + 457
plugins; `verify-bundle.sh` passes (better-sqlite3 sqlite 3.53.1 + node-pty,
glibc floor 2.34 ≤ 24.04's 2.39). The full `.deb`/`.AppImage` install on a clean
machine is the GUI/clean-machine verify item (CP6 final box + CP7 CI).
