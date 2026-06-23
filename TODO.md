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
- [x] `src-tauri` supervisor: two ephemeral loopback ports (axum + daemon); point webview at axum — [src-tauri/src/supervisor.rs](./src-tauri/src/supervisor.rs): `start()` binds both ports via `bind(127.0.0.1:0)` (axum bound synchronously so the port is known before the window is built; daemon port reserved bind-then-drop for CP3), serves `od_server::router(daemon_url)` on `tauri::async_runtime`, returns a `Supervisor` handle. [src-tauri/src/main.rs](./src-tauri/src/main.rs): creates the main window at runtime via `WebviewUrl::External(axum_url)` (loads the **axum http origin** so the frontend's relative `/api` is same-origin per the CP1 api-base spike). Topology rationale in [docs/spikes/cp2-seam.md](./docs/spikes/cp2-seam.md).
- [x] **(spike→impl)** SSE through the webview's real `EventSource` via axum (not just curl) — impl = the SSE-safe proxy above. Real-`EventSource` harness [crates/od-server/examples/sse_spike.rs](./crates/od-server/examples/sse_spike.rs) (mock SSE upstream → od-server proxy → HTML page with a live `EventSource`): `cargo run -p od-server --example sse_spike`. Curl-verified end-to-end (native page served, `/sse` proxied with `text/event-stream` preserved, `transfer-encoding: chunked`, **no** `content-encoding`, frames ~500ms apart). Live in-engine observation inside the **packaged app** (real daemon SSE) is the CP3 `(verify)` item (needs the embedded daemon, not spawned until CP3).
- [x] Tauri asset-protocol scope + CSP `connect-src` to axum origin — webview loads the axum http origin (not the asset protocol), so app content is **not** asset-protocol-served and the default capability stays minimal (`core:default`). `app.security.csp` in [src-tauri/tauri.conf.json](./src-tauri/tauri.conf.json) allows `'self'` + `http://127.0.0.1:*`/`http://localhost:*` on `default-src`/`connect-src`/`img-src` (+ `ws://…`) — `connect-src` is what permits `fetch`/`EventSource` to axum. Caveat (in cp2-seam.md): config CSP only governs Tauri-origin content; the authoritative response CSP for the external origin is axum's (a CP6 concern).
- [x] `tracing` route-resolution logging; graceful shutdown on window close — `TraceLayer::new_for_http()` on the route table (per-request method/path/status/latency; never buffers SSE bodies) + build-time `debug!` per route (`Native`/`Proxy`/catch-all) in [crates/od-server/src/route_table.rs](./crates/od-server/src/route_table.rs); proxy logs each forward+status. Graceful shutdown: `axum::serve(...).with_graceful_shutdown(notify)`; `WindowEvent::CloseRequested` → `Supervisor::shutdown()` ([src-tauri/src/main.rs](./src-tauri/src/main.rs)).
- [x] Wire `od-server` into `src-tauri` (`od-server = { workspace = true }`) — done in [src-tauri/Cargo.toml](./src-tauri/Cargo.toml) (plus `axum` for `axum::serve`). `cargo check -p rs-design` + `cargo clippy --workspace --all-targets` green; `cargo fmt --all --check` clean.

## CP3 — Embed the real daemon (dev mode) end-to-end
Exit: `cargo tauri dev` → daemon up + health-check → UI lists real skills/design-systems.
Status: implemented; axum→daemon seam verified headlessly against the real daemon (catalog, R1, R2, SSE). Full in-window `cargo tauri dev` + real-`EventSource` observation is the GUI-gated manual step (needs a clean desktop session per the CP1 spikes). See [docs/spikes/cp3-embed.md](./docs/spikes/cp3-embed.md).
- [x] Read `vendor/open-design/apps/desktop/src/main/runtime.ts`; port spawn/health/env to Rust (`tokio::process`) — launch contract sourced from the CP1 daemon-packaging spike (`daemon start --headless`), spawn via `tokio::process` in [src-tauri/src/supervisor.rs](./src-tauri/src/supervisor.rs)
- [x] Supervisor sets daemon env explicitly: `OD_PORT` = the `Supervisor::daemon_port` reserved in CP2, `OD_BIND_HOST=127.0.0.1`, `OD_DATA_DIR` (no GUI-env inheritance) — `DevNodeLauncher::command` `env_clear`s then sets only HOME/PATH + the `OD_*` vars (incl. `OD_INSTALLATION_DIR`+`OD_RESOURCE_ROOT` for content; safe-base check). Daemon spawned promptly after `reserve_ephemeral_port()` to minimize the bind-then-drop TOCTOU window.
- [x] Health-check: wait for stdout `[od] listening` AND poll `GET /api/skills`; timeout + surfaced error — `readiness()` awaits the listening line (oneshot fired by the stdout pipe) then polls `/api/skills` until 200; `READY_TIMEOUT` (30s) + `SKILLS_POLL_TIMEOUT` (10s); failure published as `DaemonStatus::Failed` and logged. `main.rs` `wait_ready(30s)` surfaces it.
- [x] Lifecycle: capture stdout/stderr → `tracing`; bounded restart-on-crash; kill on exit — `pipe_stdout`/`pipe_stderr` forward every line (target `daemon`); `supervise()` restarts on exit with backoff up to `MAX_RESTARTS=5`; `run_once` kills the child on the shutdown `watch` signal (+ `kill_on_drop`).
- [x] First-run: write default `app-config.json` with telemetry disabled — `ensure_first_run_config()` writes `{telemetry:{metrics:false,content:false,artifactManifest:false}}` if absent (daemon defaults telemetry to opt-in only when the key is missing); never clobbers an existing file.
- [x] `DaemonLauncher` abstraction: `DevNodeLauncher` now, `BundledLauncher` slot for CP6 — trait in [src-tauri/src/launcher.rs](./src-tauri/src/launcher.rs); supervisor depends only on `Arc<dyn DaemonLauncher>`.
- [x] **od-server SPA fallback (R1) — deferred from CP2** ([static-export spike](./docs/spikes/static-export.md) calls R1 "not optional"): **satisfied by proxy→daemon** — the daemon serves `out/` via `express.static` and registers its own `registerStaticSpaFallback`, so the catch-all proxy forwards deep-link HTML reloads and the daemon returns `out/index.html`. No axum-side SPA logic needed in V1 (would only be required once `/` flips native). **R2** likewise satisfied by the catch-all proxy → daemon `express.static`; **R3 (SSE)** done in CP2. Verified below.
- [x] **(verify)** app launches, catalog populates through axum→daemon — headless: real `od_server::router` over the real daemon serves `/api/skills` (155) + `/api/design-systems` (150) through axum ([cp3_proxy_verify](./crates/od-server/examples/cp3_proxy_verify.rs)). In-window launch is the GUI-gated manual step.
- [x] **(verify)** R2 origin-root assets (`/_next/*`, the 4 on-disk routes) load through axum→daemon; deep-link hard-reload resolves via R1 SPA fallback (no 404) — `GET /` 200 `text/html`, `/_next/*.css` 200 `text/css`, `/projects` `/settings` `/onboarding` (Accept html) → 200 `text/html`; `/nope.json` correctly 404s (fallback stays html-only). Evidence in [docs/spikes/cp3-embed.md](./docs/spikes/cp3-embed.md).
- [x] **(verify)** SSE-safe proxy against the real daemon SSE source (closes the CP2-Task2 / line-58 deferral): `GET /api/projects/:id/events` through axum → 200, `content-type: text/event-stream`, **no** `content-encoding`, `transfer-encoding: chunked`, first `event: ready` frame immediate. The remaining real-`EventSource`-in-WebKitGTK observation is GUI-gated (clean desktop session); full chat+todo-card SSE is CP5.

## CP4 — First native route + golden-test harness (V2 dress rehearsal)
Exit: `od-catalog` routes served by Rust; golden tests byte-identical; `--force-proxy` retained.
Status: met. The three file-walk catalog routes (`/api/skills`, `/api/design-systems`,
`/api/design-templates`) are served natively by `od-catalog`, **byte-identical** to the pinned
daemon (155 + 150 + 120 entries, 0 mismatches), gated by golden tests; `OD_FORCE_PROXY` reverts.
The contract's third name `/api/templates` is the SQLite-backed user template store, not a
file-walk catalog — it stays proxied and migrates with `od-store` (V2 step 2).
- [x] Read `skills/` + `design-templates/` + `design-systems/` from the submodule (read-only); per-folder LICENSE files are untouched (the crate only reads via `std::fs`)
- [x] `od-catalog`: faithful port of the daemon's parsers — bespoke YAML-subset frontmatter ([frontmatter.rs](./crates/od-catalog/src/frontmatter.rs)), `listSkills` + all normalizers ([skills.rs](./crates/od-catalog/src/skills.rs)), `listDesignSystems` + manifest/metadata + swatch Forms A–D ([design_systems.rs](./crates/od-catalog/src/design_systems.rs), [swift_colors.rs](./crates/od-catalog/src/swift_colors.rs)). `gray_matter`/`serde_yaml` would diverge, so they are *not* used; serde field order mirrors the daemon's object construction → `JSON.stringify` == `serde_json` (verified byte-present per element)
- [x] Golden harness in `od-contract` ([golden.rs](./crates/od-contract/src/golden.rs) — `GoldenFixture` + `.meta.json` sidecar + documented `normalize_by_id`) + runner ([tests/golden.rs](./crates/od-contract/tests/golden.rs)) driving the real `od-server` handlers over the vendored content; asserts status + header-subset + byte-identical body, plus a `--force-proxy` revert test (4 tests green)
- [x] [`scripts/capture-golden.sh`](./scripts/capture-golden.sh) — read-only GET capture from the pinned daemon (boots a throwaway daemon, or `OD_GOLDEN_BASE` against a running one); fixtures pinned at submodule `6afe7ea`
- [x] axum handlers → `Target::NativeExact` + `RouteTable::native_exact` + `od_server::router_with_catalog` ([catalog.rs](./crates/od-server/src/catalog.rs)); exact-path so siblings (`/api/skills/:id`) and non-`GET` methods (`POST /api/design-systems`) fall through to the proxy; `OD_FORCE_PROXY` keeps the `--force-proxy` rollback lever; supervisor wired via `DaemonLauncher::content_root()`
- [x] **(verify)** per-route DoD: golden pass (byte-identical) · error parity (live seam: `POST /api/design-systems` 201==201 daemon; sibling/version proxied) · routes flipped to native (`content-type: application/json; charset=utf-8`) · no regression (catch-all proxy unchanged; everything non-catalog still proxies). Live harness: [cp4_seam_verify](./crates/od-server/examples/cp4_seam_verify.rs)

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
- [ ] **Authoritative response CSP — deferred from CP2** ([cp2-seam spike](./docs/spikes/cp2-seam.md)): the webview loads the external axum http origin, so the `tauri.conf.json` config CSP only governs Tauri-origin content. Have axum emit a hardened `Content-Security-Policy` response header for served HTML (frame/connect/script scoped for the artifact iframes + loopback `/api`); reconcile with the dev-loose config CSP.
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
