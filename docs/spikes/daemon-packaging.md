# Spike: Daemon packaging — bundle + pruned deps + Node 24, PATH-stripped boot (CP1-Task3)

**Question (Open Q#2):** how do we turn the upstream Node daemon into a shippable V1 sidecar —
what is the bundling model (esbuild single-file vs. `tsc` + node_modules), how big is the
**pruned production** payload, and does it **boot and serve under a PATH-stripped env** (the GUI
launch reality, V1 gotcha #1)? Records the numbers that set the CP6 bundle shape and resolves
`pkg` vs. bundled-runtime.

**Verdict: `tsc` JS + a *pruned* prod `node_modules` + a bundled Node 24, launched by absolute
path. Not `pkg`, not a single esbuild file.** The daemon **boots and serves `/api/skills` (155),
`/api/design-systems` (150), and `/api/version` under a fully stripped env (`env -i`, empty
`PATH`)** with `better_sqlite3.node` loading cleanly — no SEA/`pkg` gymnastics. A realistic prune
takes the daemon payload from **145 M → 71 M**; with a stripped Node 24 binary (**118 M → 101 M**)
the total sidecar payload is **≈172 M** (content shipped separately as resources). This answers
**CLAUDE.md Open Question #2: bundled runtime, not `pkg`** — the `better-sqlite3` native-addon
story is trivial under a plain bundled Node.

**Environment:** Ubuntu 24.04, **Node v24.16.0** (`~/.config/nvm`), pnpm **10.33.2**, submodule pin
`6afe7ea`. Daemon `@open-design/daemon@0.11.0` (`better-sqlite3@12.10.0`, `node-pty@1.1.0`).

## Bundling model — confirmed

Upstream does **not** esbuild the daemon into one file. The daemon builds with **`tsc`** →
`apps/daemon/dist/cli.js` (entry `bin/od.mjs` dynamically imports it). Third-party deps ship as a
**pruned prod `node_modules`**. The `apps/packaged/esbuild.config.mjs` uses
`packages: "external"` — it bundles only the first-party **Electron** entry and leaves every
npm package external; it never bundles the daemon's own JS and is irrelevant to our Tauri/axum
shell. (Upstream's Linux path assembles the same shape: tarball-pack the workspace packages,
write a synthetic `package.json`, then `pnpm install --prod --config.node-linker=hoisted` —
`tools/pack/src/linux.ts`.)

For the spike I reproduced the pruned bundle with one idiomatic command:

```bash
pnpm --filter @open-design/daemon deploy --legacy --prod <out>
```

`--legacy` is required (pnpm v10 refuses non-injected workspace deploys by default). It produces a
self-contained `<out>/{dist,bin,node_modules,package.json}`. Caveat: it re-triggers workspace
`postinstall` builds (noisy/slow) — a dedicated `scripts/build-daemon-bundle.sh` (CP6) should use
the tarball + `--node-linker=hoisted` route and apply the prune below instead.

## PATH-stripped boot — the headline check ✅

Launched the **deploy bundle** with `env -i` (no inherited environment, **empty `PATH`**), Node
invoked by **absolute path** (exactly what the Rust supervisor will do), only the daemon's own env
vars set:

```bash
env -i HOME=… PATH="" \
    OD_PORT=$PORT OD_BIND_HOST=127.0.0.1 OD_DATA_DIR=$DATA \
    OD_INSTALLATION_DIR=$VENDOR OD_RESOURCE_ROOT=$VENDOR \
    /abs/path/to/node  <bundle>/dist/cli.js daemon start --headless --port $PORT --host 127.0.0.1
```

Result — ready in ~2 s, stdout `[od] listening on http://127.0.0.1:<port> (headless)`:

| Probe | Result |
|---|---|
| Boot under empty `PATH` (DB init, plugin registry) | ✅ `registered 457 bundled plugin(s)` |
| `better_sqlite3.node` load | ✅ (SQLite opened; no error) |
| `GET /api/skills` | ✅ **155** skills |
| `GET /api/design-systems` | ✅ **150** systems |
| `GET /api/version` | ✅ `0.11.0 · linux · x64` |
| Graceful `SIGTERM` shutdown | ✅ |

**The daemon core (HTTP/SSE/SQLite/catalog) needs zero `PATH`.** `PATH` only matters for *spawning
agent CLIs* (`claude`/`codex`/…) — that's the separate login-shell PATH-reconstruction lever at
**CP5**, not a boot blocker. This de-risks V1 gotcha #1: the supervisor can launch the daemon from
a bare desktop env and the app's catalog/BYOK surface works immediately.

### CP3 supervisor wiring this dictates
- **Launch:** `node dist/cli.js daemon start --headless --port <ephemeral> --host 127.0.0.1`
  (node by absolute path).
- **Env:** `OD_PORT`, `OD_BIND_HOST=127.0.0.1`, `OD_DATA_DIR`, `OD_RESOURCE_ROOT` for content.
  **Gotcha:** `OD_RESOURCE_ROOT` is rejected unless it sits under `PROJECT_ROOT`,
  `process.resourcesPath`, or **`OD_INSTALLATION_DIR`** (safe-base check in
  `apps/daemon/src/server.ts:1300`). When pointing at content outside the daemon dir, the
  supervisor **must also set `OD_INSTALLATION_DIR`** (or place `skills/` etc. under the bundle
  root). Found the hard way in this spike.
- **Readiness:** wait for stdout `[od] listening on` (then poll `GET /api/skills`, per CP3 plan).

## Size — measured (linux-x64)

| Component | Raw deploy | Pruned | What pruned |
|---|---:|---:|---|
| daemon `dist/` (tsc output) | 12 M | **5.8 M** | 346 `*.d.ts` + 346 `*.map` (runtime needs neither) |
| prod `node_modules/` | 133 M | **65 M** | see below |
| **daemon bundle subtotal** | **145 M** | **71 M** | **−51 %** |
| Node 24 runtime binary | 118 M | **101 M** | `strip` |
| **sidecar payload total** | ~263 M | **≈172 M** | |
| content (`skills/`, `design-systems/`, `frames/`) | — | — | shipped as separate Tauri resources |

**Where the node_modules bulk is — and what's dead weight:**
- **`node-pty@1.1.0` = 63 M, of which ~58 M is `prebuilds/win32-*` (30 M + 28 M) + `darwin-*`** —
  binaries the Linux daemon can **never** load. Pure dead weight; dropped.
- **`better-sqlite3@12.10.0` = 13 M**, but `deps/` (9.8 M SQLite amalgamation source) + `src/` +
  `build/Release/obj` are **build-time only**. Keep `build/Release/better_sqlite3.node` (2.1 M);
  drop the rest. (Upstream's electron-builder excludes the same `deps`/`obj`.)
- Misc transitive: two `zod` majors (v4 5 M + v3 4.7 M, hard to dedupe), `@modelcontextprotocol/sdk`
  4.7 M, `hono` 3.6 M, `undici` 2.2 M, etc.

The pruned bundle was **re-booted under the same stripped env and still served 155 skills** — the
prune removes nothing the runtime touches.

### CP6 prune rules to encode in `scripts/build-daemon-bundle.sh`
```
rm -rf node-pty/prebuilds/{win32-x64,win32-arm64,darwin-arm64,darwin-x64}
rm -rf better-sqlite3/{deps,src,build/Release/obj,build/Release/.deps}
find dist -name '*.d.ts' -delete; find dist -name '*.map' -delete
strip bin/node            # 118 M → 101 M
```

## Native addons — boot-relevant findings (full story → CP1-Task4)
- **`better-sqlite3`:** ✅ loads under bundled Node; `better_sqlite3.node` links only standard libs
  (`libstdc++`, `libm`, `libgcc_s`, `libc`) — all present on Ubuntu 24.04.
- **`node-pty`:** ⚠️ **no Linux binary exists** — pnpm **ignored its build script** at install
  (`Ignored build scripts: node-pty@1.1.0`; matches the CP0 note). The daemon **boots fine
  anyway** because node-pty is lazy-loaded only for terminal features. Consistent with the V1
  decision to keep **`/api/terminal` out of smoke scope**. **CP6 must** either compile node-pty
  for linux-x64 (`pnpm approve-builds` / `node-gyp rebuild`) to enable the terminal, or ship
  without it and confirm terminal degrades gracefully.
- **Node 24 glibc floor:** `GLIBC_2.28` (Ubuntu 24.04 ships 2.39 ✓). RPATH/glibc rigor is
  CP1-Task4's job.

## Open Question #2 — resolved
**Bundled runtime, not `pkg`.** The whole reason to consider `pkg`/SEA was the `better-sqlite3`
native-addon packaging fight. Under a plain bundled Node 24 + pruned `node_modules`, the addon
loads with zero special handling and the daemon serves under a stripped env. `pkg` buys nothing
and reintroduces the addon problem. Ship `tsc` JS + pruned deps + a bundled, stripped Node 24.

## How to reproduce
```bash
# 1. pruned prod bundle of the daemon
pnpm --filter @open-design/daemon deploy --legacy --prod /tmp/daemon-deploy

# 2. apply the CP6 prune rules (see box above) → ~71 M

# 3. boot under a stripped env, node by absolute path
env -i HOME=$HOME PATH="" \
    OD_PORT=7790 OD_BIND_HOST=127.0.0.1 OD_DATA_DIR=/tmp/od-data \
    OD_INSTALLATION_DIR=$PWD/vendor/open-design OD_RESOURCE_ROOT=$PWD/vendor/open-design \
    "$(readlink -f "$(which node)")" /tmp/daemon-deploy/dist/cli.js \
    daemon start --headless --port 7790 --host 127.0.0.1
# expect: "[od] listening on http://127.0.0.1:7790 (headless)"
# then: curl -s 127.0.0.1:7790/api/skills | jq '.skills | length'   # → 155
```

## Implications for the roadmap
- **Open Q#2:** answered — bundled runtime + pruned deps; `pkg` dropped.
- **CP3:** supervisor launch/env/readiness contract above (note the `OD_INSTALLATION_DIR`
  safe-base requirement for `OD_RESOURCE_ROOT`).
- **CP6:** `scripts/build-daemon-bundle.sh` = `pnpm deploy` (or tarball + `--node-linker=hoisted`)
  → prune rules → ship ~71 M daemon + ~101 M stripped Node 24 as Tauri resources; **compile
  node-pty for linux-x64** (or accept no terminal). `pnpm approve-builds` is mandatory (build
  scripts are skipped by default).
- **CP1-Task4:** inherits the native-addon load story (RPATH/glibc rigor for both `.node` files
  under the *bundled* Node, not the dev Node).
- **PACKAGING.md / ARCHITECTURE.md:** record the bundled-runtime decision and the ≈172 M payload.
