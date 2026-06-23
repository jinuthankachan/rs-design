# rs-design — V1 Roadmap (TODO)

Checkpoint task list for **V1** (Tauri shell embedding the Node daemon), structured so the
future **V2** strangler-fig migration is friction-free. Full rationale lives in the approved
plan and in [docs/](./docs/): [ARCHITECTURE](./docs/ARCHITECTURE.md) ·
[CONTRACT](./docs/CONTRACT.md) · [PACKAGING](./docs/PACKAGING.md) ·
[V2-MIGRATION](./docs/V2-MIGRATION.md).

**Locked decisions:** Rust axum proxy from day one · V1 includes the first native route
(`od-catalog`) · upstream as a pinned git submodule.

Legend: `[ ]` todo · `[x]` done · **(spike)** investigation · **(user)** needs the user (sudo/secrets) ·
**(verify)** manual/automated check. Dependency order: `CP0→CP1→CP2→CP3→CP4→CP6→CP7`, with
`CP5` after CP2+CP3.

---

## CP0 — Foundation: repo + workspace skeleton + vendored upstream — ✅ COMPLETE
Exit: `cargo build` green on the non-tauri crates; submodule present; licensing in place.
Status: met. All 8 non-tauri crates pass offline `cargo check` (exit 0); the full
dependency graph (incl. `src-tauri`/`tauri`) now resolves and downloads (439 crates, no
network errors — only the `cairo`/WebKitGTK *system* libs are missing, which is the sudo
prereq below, not a dep issue). Submodule working tree is materialized and clean.
- [x] `git init`; `.gitignore`; `rust-toolchain.toml` (1.96)
- [x] Root `Cargo.toml` workspace declaring all members incl. `od-contract`; `[workspace.dependencies]`
- [x] All 8 crates as compiling stubs (doc comment + `// TODO(V2 step N)`)
- [x] `src-tauri` Tauri v2 scaffold (placeholder window, `supervisor`/`launcher`/`env_inject` stubs, default capability, placeholder UI + icon)
- [x] `NOTICE` (Apache-2.0 attribution) + licensing guardrail note
- [x] `docs/` placeholders (ARCHITECTURE, CONTRACT, PACKAGING, V2-MIGRATION) + `docs/spikes/`
- [x] Add `vendor/open-design` submodule pinned to `6afe7ea`; SHA recorded in `docs/ARCHITECTURE.md`
- [x] Copy upstream `LICENSE` (Apache-2.0) to repo root
- [x] **(verify)** non-tauri crates compile — offline `cargo check -p od-contract -p od-server -p od-catalog -p od-store -p od-proxy -p od-artifacts -p od-agents -p od-prompt` is green (exit 0)
- [x] **(verify)** full-graph build incl. `src-tauri`/`tauri` — after the system `-dev` libs were installed, `cargo check --workspace` compiles the entire stack (gtk/webkit2gtk/wry/tauri + `rs-design` app), exit 0. Required bumping transitive `time` 0.3.50→0.3.51 (0.3.50 fails to build on the current toolchain: `unresolved import time_macros::timestamp`)
- [x] Materialize submodule working tree — `vendor/open-design` was a **blob:none promisor
  partial clone** (only `LICENSE` on disk; every other path showed as `D`). `git checkout -f HEAD`
  lazily fetched all blobs; tree now materialized (157 skills, 152 design-systems, `apps/daemon`,
  `apps/web`) and the gitlink is clean (no longer `-dirty`)

- [x] **(user)** install build prereqs — see [docs/PACKAGING.md](./docs/PACKAGING.md): `libwebkit2gtk-4.1-dev`, `libjavascriptcoregtk-4.1-dev`, `libsoup-3.0-dev`, `patchelf`, `libssl-dev` all installed; `cargo install tauri-cli` → `tauri-cli 2.11.3`. (`librsvg2-dev`/`rsvg2` still missing — needed only for CP6 icon/SVG bundling, not for build/dev.)

- [x] **(spike)** confirm `pnpm -C vendor/open-design install` + daemon/web builds; record time/size — all green; see [docs/spikes/upstream-build.md](./docs/spikes/upstream-build.md). install 1.4G/~1m52s, daemon `tsc` 12M/10.5s, web `next build` static-exports `out/` 51M/52.2s (confirms Open Q#1 default). `node-pty` build script ignored by pnpm default (CP6 must approve/prebuild).

## CP1 — De-risking spikes (open questions → facts)
Exit: one `docs/spikes/*` note per question; packaging shape decided.
- [x] **(spike)** Static export reality: build `out/`, load in real WebKitGTK, verify routing/hydration → static vs `standalone`. **Verdict: static** (no second sidecar). All checks pass in WebKitGTK 2.52.3 (`webkit2gtk-4.1`, Tauri's engine): render + hydration (`reactFiber`, no mismatch) + client routing + deep-link hard-load. App is a pure client SPA (only 4 HTML routes on disk) with origin-relative `/api/*`. → imposes **R1 SPA-fallback, R2 origin-root serving, R3 SSE passthrough** on CP2 axum. See [docs/spikes/static-export.md](./docs/spikes/static-export.md).
- [x] **(spike)** Frontend API-base wiring: **pure relative `/api`** (+ `/artifacts`, `/frames`), no build-time base anywhere (no `NEXT_PUBLIC_*` API var / `basePath` / `assetPrefix`). Origin is read at runtime as `window.location.origin` = axum under the locked topology → **zero per-environment `out/` rebuild**. BYOK provider `baseUrl` is request-body data to relative `/api/proxy/*` (SSRF stays server-side). Dev `next.config` rewrites are dev-only → **axum owns `/api`·`/artifacts`·`/frames` proxying, not `next dev`**. Source + built-bundle audit. Imposes **W1 same-origin serving · W2 no build-time injection · W3 submodule-bump grep guard · W4 Use-Everywhere panel shows ephemeral axum port (CP5/CP6)**. See [docs/spikes/api-base-wiring.md](./docs/spikes/api-base-wiring.md).
- [x] **(spike)** Daemon packaging: model is **`tsc` JS + pruned prod `node_modules` + bundled Node 24** (not `pkg`, not a single esbuild file — esbuild `packages:external` only bundles the first-party Electron entry). Reproduced via `pnpm --filter @open-design/daemon deploy --legacy --prod`. **Boots + serves `/api/skills` (155) · `/api/design-systems` (150) · `/api/version` under `env -i`/empty `PATH`** (node by absolute path) — core daemon needs **zero PATH**; only agent-CLI spawn does (CP5). `better-sqlite3` loads clean (std libs, GLIBC_2.28 ✓); **node-pty has no Linux binary** (build script skipped — CP6 must compile or skip terminal, matches `/api/terminal` out-of-smoke). **Size:** daemon **145 M→71 M** pruned (drop node-pty win/darwin prebuilds −58 M, better-sqlite3 `deps/src/obj` −10 M, dist `.d.ts`/`.map` −6 M) + Node 24 **118 M→101 M** stripped ≈ **172 M** sidecar. **Resolves Open Q#2: bundled runtime.** CP3 env gotcha: `OD_RESOURCE_ROOT` needs `OD_INSTALLATION_DIR` set (safe-base check). See [docs/spikes/daemon-packaging.md](./docs/spikes/daemon-packaging.md).
- [x] **(spike)** Native addon load: **both `.node` addons load AND run under a relocated, stripped Node 24 (`env -i`, empty PATH)** — `better-sqlite3` executes real SQL (statically-linked SQLite 3.53.1, **no `libsqlite3` dep**), `node-pty` spawns a real PTY (after compiling for linux-x64; `npx node-gyp rebuild` → 75 K `pty.node`, ~1 s). **No RPATH/RUNPATH on either → no `patchelf`/relocation for addons.** ABI `NODE_MODULE_VERSION` **137** (any Node 24.x loads them). glibc/GLIBCXX floors: better-sqlite3 **2.29/3.4.20** (portable prebuild), node-pty **2.34/3.4.22** (= build host → Ubuntu 22.04+); target 24.04 = **2.39/3.4.33** ✓. **node-pty has no shipped Linux prebuild + pnpm skips its build script** → CP6 must compile it (or skip terminal); it sets the bundle's glibc floor. See [docs/spikes/native-addons.md](./docs/spikes/native-addons.md).
- [x] **(spike)** WebKitGTK render: **6/6 showcase artifacts render correctly** in WebKitGTK 2.52.3 (online) — design-system components, slide decks, glassmorphism, pixel-art compositing, canvas charts, WebGL hero. **Every modern CSS feature `CSS.supports()=true`** (color-mix, backdrop-filter, clip-path, mask-image, mix-blend, `:has`, container queries, conic-gradient, nesting, aspect-ratio); **WebGL2 + canvas-2D work**; no JS/console/resource errors. CLAUDE.md "modern CSS may differ" caution **downgraded**. Real risk is **network, not engine** → **KI-1**: offline, Google Fonts (142 files, zero `@font-face`) fall back to system fonts (visible typography drift), Three.js/Tailwind CDNs 404 → broken WebGL scenes + uncaught error (5/6 offline). KI-4: ensure GStreamer plugins for `<video>` artifacts at CP6. 12 screenshots + reusable harness. See [docs/spikes/webkitgtk-render.md](./docs/spikes/webkitgtk-render.md).
- [x] **(decision)** V1 acceptance test scope — **resolved (Open Q#3): Claude Code (`stream-json`) + ≥1 other CLI + BYOK**; `/api/terminal` **out of smoke** (node-pty is the one packaging gap per Task3/Task4). CP5 done-criteria = (a) Claude Code detected via PATH, (b) ≥1 other CLI detected (second independent lever), (c) BYOK works with **no** CLI present (relative `/api/proxy/*`, SSRF server-side), (d) SSE chat + todo card stream live in WebKitGTK. Chosen over "Claude+BYOK only" / "BYOK only" for broadest confidence in the core value.

## CP2 — Stable seam: supervisor + axum catch-all proxy + route table
Exit: webview loads through axum; SSE streams un-buffered; route table is a real `Proxy|Native` type.
- [x] `od-server`: axum fallback `/*path` forwarding via `reqwest`; strip hop-by-hop headers — `ProxyState` (redirects disabled) + `proxy_handler` catch-all in [crates/od-server/src/proxy.rs](./crates/od-server/src/proxy.rs); request/response bodies streamed (unbuffered, SSE-ready), hop-by-hop headers + `Connection`-named tokens stripped both directions (`host`/`content-length` also dropped upstream so `reqwest` re-derives them); upstream failure → `502`. 5 integration tests in [crates/od-server/tests/proxy.rs](./crates/od-server/tests/proxy.rs) (method/path/query, body, both-direction stripping, 502); fmt+clippy clean.
- [x] **SSE-safe streaming proxy** (no buffering; preserve `text/event-stream`; no SSE compression) — highest-risk task. Request/response bodies streamed (Task1 foundation); `accept-encoding` dropped on the forwarded request so the loopback daemon never compresses (compression coalesces SSE frames); `tcp_nodelay(true)` flushes small `data:` frames; `content-type`/`cache-control`/framing pass through untouched. od-server applies **no** response compression — invariant + future-`CompressionLayer` warning recorded on `router` in [crates/od-server/src/lib.rs](./crates/od-server/src/lib.rs). 3 SSE tests in [crates/od-server/tests/sse.rs](./crates/od-server/tests/sse.rs): header preservation, **incremental delivery** (first frame arrives before upstream's 300ms gap → not buffered), `accept-encoding` stripped. NOTE: verified via reqwest stream client; real-`EventSource`-through-webview check is the separate CP2 spike task below.
- [x] Route-table type `(prefix, Proxy{upstream}|Native{service})`; V1 = `("/", Proxy)`; dispatcher — [crates/od-server/src/route_table.rs](./crates/od-server/src/route_table.rs): `Target::Proxy{upstream} | Native{router}` enum + `RouteEntry` + builder `RouteTable` (`proxy_all`/`proxy`/`native`/`force_proxy`). Dispatcher `into_router()` compiles to axum: Native prefixes `nest`ed, `("/", Proxy)` → `fallback_service` (avoids axum's ignored-nested-fallback gotcha), non-root proxy prefixes via wildcard routes. `force_proxy(true)` skips Native registration so those prefixes fall through to proxy — the CP4 `--force-proxy` rollback lever. `router()` is now `RouteTable::proxy_all(upstream).into_router()` (existing proxy/SSE tests unchanged). 5 tests in [crates/od-server/tests/route_table.rs](./crates/od-server/tests/route_table.rs): native-vs-proxy dispatch, `force_proxy` revert, V1 proxy-all, non-root proxy subtree, typed-enum inspection. 13 od-server tests green; fmt+clippy clean.
- [ ] `src-tauri` supervisor: two ephemeral loopback ports (axum + daemon); point webview at axum
- [ ] **(spike→impl)** SSE through the webview's real `EventSource` via axum (not just curl)
- [ ] Tauri asset-protocol scope + CSP `connect-src` to axum origin
- [ ] `tracing` route-resolution logging; graceful shutdown on window close
- [ ] Wire `od-server` into `src-tauri` (`od-server = { workspace = true }`)

## CP3 — Embed the real daemon (dev mode) end-to-end
Exit: `cargo tauri dev` → daemon up + health-check → UI lists real skills/design-systems.
- [ ] Read `vendor/open-design/apps/desktop/src/main/runtime.ts`; port spawn/health/env to Rust (`tokio::process`)
- [ ] Supervisor sets daemon env explicitly: `OD_PORT`, `OD_BIND_HOST=127.0.0.1`, `OD_DATA_DIR` (no GUI-env inheritance)
- [ ] Health-check: wait for stdout `[od] listening` AND poll `GET /api/skills`; timeout + surfaced error
- [ ] Lifecycle: capture stdout/stderr → `tracing`; bounded restart-on-crash; kill on exit
- [ ] First-run: write default `app-config.json` with telemetry disabled
- [ ] `DaemonLauncher` abstraction: `DevNodeLauncher` now, `BundledLauncher` slot for CP6
- [ ] **(verify)** app launches, catalog populates through axum→daemon
- [ ] **(verify)** SSE-safe proxy through the **real webview `EventSource`** (closes the CP2-Task2 / line-58 deferral, now that a live daemon SSE source exists behind axum): open an `EventSource` against a daemon SSE route in WebKitGTK, confirm frames arrive incrementally (not buffered), `text/event-stream` preserved, no compression. Full chat+todo-card SSE is CP5.

## CP4 — First native route + golden-test harness (V2 dress rehearsal)
Exit: `od-catalog` routes served by Rust; golden tests byte-identical; `--force-proxy` retained.
- [ ] Read `skills/` + `design-systems/` from submodule; preserve per-folder LICENSE files
- [ ] `od-catalog`: `walkdir` over `SKILL.md`/`DESIGN.md` + `gray_matter` frontmatter; match daemon JSON shapes
- [ ] Golden harness in `od-contract`/`tests/golden` (fixture format + runner + normalization)
- [ ] `scripts/capture-golden.sh` (read-only) for the three catalog routes
- [ ] axum handlers → register as `Native`; keep `--force-proxy`
- [ ] **(verify)** per-route DoD: golden pass, error parity, route flipped, no UI regression

## CP5 — Acceptance integrations (CLI/PATH, SSE chat, BYOK)
Exit: the three behavioral done-criteria pass manually.
- [ ] **(spike→impl)** login-shell PATH reconstruction → inject into daemon PATH
- [ ] Explicit CLI-path settings → `CLAUDE_BIN` + ≥1 other (second independent lever)
- [ ] **(verify)** Claude Code + 1 other CLI detected; no-CLI path degrades gracefully
- [ ] **(verify)** real chat streams tokens + todo card live in WebKitGTK
- [ ] **(verify)** BYOK works with no CLI present; SSRF guard intact
- [ ] Surface CLI-detection + BYOK status in settings/diagnostics

## CP6 — Package: bundled daemon + `.deb` + `.AppImage`, clean-machine launch
Exit: both artifacts install on a clean Ubuntu 24.04 (zero Node/pnpm) and pass all behaviors.
- [ ] `scripts/build-daemon-bundle.sh`: esbuild daemon + pruned prod deps (prebuilt `better-sqlite3` + `node-pty`)
- [ ] Pin + place a Node 24 linux-x64 runtime as a resource (record license)
- [ ] Verify bundled Node loads both `.node` addons (RPATH/dlopen/glibc)
- [ ] Tauri `bundle.resources`/`externalBin`: ship `out/`, daemon tree, Node binary, content (with LICENSEs); sidecar perms
- [ ] `BundledLauncher` (resolve resource paths; inject env)
- [ ] Packaged `OD_DATA_DIR` → XDG; first-run creates it + telemetry-off config
- [ ] Bundler config for `deb` + `appimage`; generate real icons (`cargo tauri icon`); deb depends
- [ ] **(verify)** clean Ubuntu 24.04, no Node/pnpm: install `.deb` + `.AppImage`; health-check, native catalog, ≥5 skills render, SSE chat, BYOK; no orphan daemon

## CP7 — CI/CD: build, install-test, golden + e2e, release
Exit: green CI producing installable `.deb` + `.AppImage`, gated by golden tests.
- [ ] GH Actions on `ubuntu-24.04`: prereqs, cache cargo+pnpm, build frontend+daemon+workspace, `tauri build`
- [ ] CI: install `.deb` + headless launch smoke under Xvfb (health-check + `/api/skills` 200)
- [ ] CI: golden suite + `cargo test --workspace` + clippy/fmt
- [ ] **(spike→impl)** minimal WebKitGTK e2e under Xvfb (catalog + one SSE event; mock BYOK provider)
- [ ] Release workflow: attach artifacts; version = our version + upstream submodule SHA
- [ ] Document the contributor build path in `docs/PACKAGING.md`

---

## V1 done-criteria → checkpoint
| Done-criterion | Met at |
|---|---|
| `.deb`/`.AppImage` build in CI + install on Ubuntu LTS | CP7 (local CP6) |
| Launches with zero Node/pnpm on a clean machine | CP6 |
| Supervisor spawns daemon + health-check + UI loads | CP3 |
| PATH injection detects Claude Code + 1 other CLI | CP5 |
| BYOK works with no CLI present | CP5 |
| ≥5 showcase skills render in WebKitGTK | CP1 → CP6 |
| SSE streaming (chat + todo card) end to end | CP2 → CP5 |
