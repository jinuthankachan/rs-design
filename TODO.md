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

## CP5 — Acceptance integrations (CLI/PATH, SSE chat, BYOK) — ✅ COMPLETE
Exit: the three behavioral done-criteria pass. Status: met headlessly through the
real axum seam (`scripts/cp5-acceptance.sh` → 4/4 pass: `claude 2.1.137` +
`antigravity 1.0.10` detected, no-CLI graceful, SSRF 403, BYOK reachable). The
daemon owns detection/chat/BYOK in V1; the Rust port's job is **env injection** so
the GUI-launched daemon sees the user's CLIs (V1 gotcha #1) — never reimplement
detection. Full write-up: [docs/spikes/cp5-acceptance.md](./docs/spikes/cp5-acceptance.md).
- [x] **(spike→impl)** login-shell PATH reconstruction → inject into daemon PATH — `$SHELL -lic` marker probe (5 s timeout, graceful fallback, `OD_SKIP_LOGIN_PATH` opt-out), merged login-first/de-duped with the GUI PATH and injected as the daemon child's `PATH` ([env_inject.rs](./src-tauri/src/env_inject.rs) + [launcher.rs](./src-tauri/src/launcher.rs)); `resolve_node_bin` scans the same merged PATH. The daemon resolves CLIs from `process.env.PATH` (+ well-known toolchain dirs), so this is the fix.
- [x] Explicit CLI-path settings → `CLAUDE_BIN` + ≥1 other (second independent lever) — `env_inject` collects `*_BIN` (key list mirrors the daemon's `AGENT_BIN_ENV_KEYS`) and **seeds** them into `app-config.json` `agentCliEnv`, the daemon's PATH-*independent* override (`configuredExecutableOverride`); never clobbers a user-set value. Process-env `*_BIN` alone is **not** honored by the daemon, so seeding the file is the faithful mechanism. 7 unit tests (`cargo test -p rs-design --bins env_inject`).
- [x] **(verify)** Claude Code + 1 other CLI detected; no-CLI path degrades gracefully — Mode 1: `GET /api/agents` via axum → `claude` + `antigravity` available. Mode 2: deterministically CLI-free env (`OD_AGENT_HOME=<empty>` + minimal PATH) → `/api/agents` 200, **0 available**, daemon still serves. [cp5_seam_verify](./crates/od-server/examples/cp5_seam_verify.rs).
- [x] **(verify)** BYOK works with no CLI present; SSRF guard intact — Mode 3 (CLI-free): `POST /api/proxy/anthropic/stream` link-local `baseUrl` → **403** (SSRF), incomplete body → **400** (route present + validates, not 404) → BYOK independent of agent detection. (SSRF guard allows loopback for local Ollama by design.)
- [x] Surface CLI-detection + BYOK status in settings/diagnostics — startup probe of `/api/agents` through axum logs the acceptance summary (per-CLI name/version/auth, Claude+other check, no-CLI note, BYOK-always note) in [diagnostics.rs](./src-tauri/src/diagnostics.rs); user-facing surface stays the proxied daemon Settings UI.
- [ ] **(verify)** real chat streams tokens + todo card live in WebKitGTK — **GUI-gated** (clean desktop session). SSE *framing* is verified end-to-end in CP2/CP3; what remains is observing a live chat token stream + the pinned TodoCard render in-window via `cargo tauri dev`. Tracked as the manual confirmation step in [docs/spikes/cp5-acceptance.md](./docs/spikes/cp5-acceptance.md) (alongside the BYOK live-key token stream, which can't use loopback offline).

## CP6 — Package: bundled daemon + `.deb` + `.AppImage`, clean-machine launch
Exit: both artifacts install on a clean Ubuntu 24.04 (zero Node/pnpm) and pass all behaviors.
Status: implemented + headlessly verified. The three build scripts assemble a
~363 M `runtime/` resource tree that **boots under `env -i`/empty `PATH` and
serves 155 skills + 150 design-systems + `index.html` (200) + 457 plugins**;
`verify-bundle.sh` passes both `.node` addons under the bundled Node. The
`BundledLauncher` resolves it from the Tauri resource dir and materializes node
(AppImage read-only safe). The actual `.deb`/`.AppImage` install on a **clean**
machine remains the umbrella manual acceptance (final box + CP7 CI). Build path
written up in [docs/PACKAGING.md](./docs/PACKAGING.md).
- [x] `scripts/build-daemon-bundle.sh`: **`pnpm deploy` + pruned prod deps** (not esbuild — the spike showed esbuild only bundles the Electron entry; model is `tsc` JS + pruned `node_modules`). Applies the daemon-packaging prune rules, **compiles `node-pty` for linux-x64** (best-effort; terminal out of V1 smoke scope), keeps the prebuilt `better-sqlite3` `.node`, lays out `runtime/` so the daemon's `PROJECT_ROOT`/`STATIC_DIR`/`OD_RESOURCE_ROOT` all resolve on one root. cp -a preserves per-folder LICENSEs.
- [x] Pin + place a Node 24 linux-x64 runtime as a resource (record license) — [`scripts/fetch-node-runtime.sh`](./scripts/fetch-node-runtime.sh) pins **v24.16.0** (ABI 137), sha256-verifies the published tarball, strips it (118→101 M), records `NODE_LICENSE` + `NODE_VERSION`.
- [x] Verify bundled Node loads both `.node` addons (RPATH/dlopen/glibc) — [`scripts/verify-bundle.sh`](./scripts/verify-bundle.sh) `require()`s both under `env -i`/empty PATH and runs one real op each (better-sqlite3 SQL roundtrip + sqlite_version 3.53.1; node-pty real PTY). **PASS**, glibc floor GLIBC_2.34 ≤ 24.04's 2.39. CP6/CP7 regression gate.
- [x] Tauri `bundle.resources` ship `out/`, daemon tree, Node binary, content (with LICENSEs); sidecar perms — `bundle.resources: { "bundle-resources/runtime": "runtime" }` in [tauri.conf.json](./src-tauri/tauri.conf.json). **Resources (not `externalBin`)**: we spawn node ourselves and AppImage mounts resources read-only, so `BundledLauncher` materializes node to the writable data dir + `chmod +x` (`.node` addons are dlopen'd → no exec bit needed).
- [x] `BundledLauncher` (resolve resource paths; inject env) — [launcher.rs](./src-tauri/src/launcher.rs): resolves `<resource>/runtime`, shares `configure_daemon_command` with `DevNodeLauncher` (identical launch contract). Supervisor picks it by probing `runtime/apps/daemon/dist/cli.js`; `cargo tauri dev` (no bundle) → `DevNodeLauncher`.
- [x] Packaged `OD_DATA_DIR` → XDG; first-run creates it + telemetry-off config — `launcher::data_dir(packaged)`: packaged → `$XDG_DATA_HOME/rs-design`, dev → `rs-design/dev` (never share a SQLite store); `OD_DATA_DIR` still overrides. First-run config + telemetry-off seeding (CP3/CP5) run against the per-mode dir.
- [x] **Authoritative response CSP — deferred from CP2** ([cp2-seam spike](./docs/spikes/cp2-seam.md)): axum `set_security_headers` ([security.rs](./crates/od-server/src/security.rs)) stamps an authoritative `Content-Security-Policy` (+ nosniff + referrer-policy) on `text/html` only — JSON/assets/**SSE** untouched — and **overwrites** any upstream CSP. Hardened (`object-src 'none'`, `base-uri`/`frame-ancestors 'self'`) but allows `blob:`/`data:`/`https:` for the sandboxed `srcdoc` artifact iframes (which inherit the shell CSP). 3 tests; full od-server (18) + golden (4) green.
- [x] Bundler config for `deb` + `appimage`; generate real icons (`cargo tauri icon`); deb depends — real 1024² icon set (replacing the 1×1 placeholder); `deb.depends: [libwebkit2gtk-4.1-0, libgtk-3-0]`; `appimage.bundleMediaFramework: true` (GStreamer for `<video>` — addresses KI-4 on the AppImage).
- [ ] **(verify)** clean Ubuntu 24.04, no Node/pnpm: install `.deb` + `.AppImage`; health-check, native catalog, ≥5 skills render, SSE chat, BYOK; no orphan daemon — **clean-machine gated** (see manual backlog). Bundle boot + addon load + catalog/web serving verified headlessly above; what remains is the in-engine install acceptance on a Node-free machine.

## CP7 — CI/CD: build, install-test, golden + e2e, release
Exit: green CI producing installable `.deb` + `.AppImage`, gated by golden tests.
Status: implemented. Three `ubuntu-24.04` workflows in `.github/workflows/`:
`ci.yml` (fmt·clippy·`cargo test --workspace` incl. golden·seam e2e),
`package.yml` (build daemon bundle → `verify-bundle.sh` → `cargo tauri build` →
install `.deb` + launch the real app in WebKitGTK under Xvfb → upload installers),
`release.yml` (tag `v*` → reuse `package.yml`, publish release stamped with app
version + upstream SHA). The offline e2e drives the real axum seam with a loopback
mock BYOK provider. CI path is locally green for the fast gate (fmt/clippy/test+
golden/seam-e2e); the install-test + in-engine launch are exercised on the CI
runner (display-gated locally). See [docs/PACKAGING.md](./docs/PACKAGING.md) "CP7
CI/CD".
- [x] GH Actions on `ubuntu-24.04`: prereqs, cache cargo (`Swatinem/rust-cache`)+pnpm (corepack), build frontend+daemon+workspace, `tauri build` — `ci.yml` (check job) + `package.yml`
- [x] CI: install `.deb` + headless launch smoke under Xvfb (health-check + `/api/skills` 200) — `package.yml` → [`scripts/ci-install-smoke.sh`](./scripts/ci-install-smoke.sh): `apt install ./*.deb`, launch `/usr/bin/rs-design` under `xvfb-run`, parse the app's axum origin from its logs, assert catalog 200, assert no orphan daemon on exit
- [x] CI: golden suite + `cargo test --workspace` + clippy/fmt — `ci.yml` check job (`cargo fmt --check`, `cargo clippy -D warnings`, `cargo test --workspace` runs the od-contract golden tests)
- [x] **(spike→impl)** minimal WebKitGTK e2e under Xvfb (catalog + one SSE event; mock BYOK provider) — loopback OpenAI mock ([`scripts/mock-byok-openai.mjs`](./scripts/mock-byok-openai.mjs)) + [`scripts/e2e-smoke.sh`](./scripts/e2e-smoke.sh) (seam, in `ci.yml`) and the in-engine variant in `ci-install-smoke.sh` (`package.yml`): a `POST /api/proxy/openai/stream` round-trip streams the mock token back as SSE — covers catalog + one SSE event + mock BYOK in one round-trip
- [x] Release workflow: attach artifacts; version = our version + upstream submodule SHA — `release.yml` + [`scripts/release-version.sh`](./scripts/release-version.sh) (`<app>+od.<upstream-sha>`, reads the pinned gitlink SHA)
- [x] Document the contributor build path in `docs/PACKAGING.md` — added "CP7 CI/CD" + a local "Contributor build path" mirroring the three workflows

---

## Manual / GUI-gated verification backlog

Cross-cutting checks that **can't be verified headlessly** in this dev
environment (need a real display / clean desktop session — the VS Code snap shell
crashes GTK on `GLIBC_PRIVATE` per the CP1 spikes — or a live secret / clean
machine). The underlying HTTP/SSE behaviors each depends on are already verified
headlessly in the cited checkpoint; what remains is the **in-engine / live
observation**. Collected here so they don't stay buried in checkpoint prose.

Run all GUI items together once on a clean desktop session via `cargo tauri dev`.

- [ ] **(CP3, GUI)** Full in-window launch: `cargo tauri dev` → daemon up →
  health-check passes → UI lists real skills/design-systems; deep-link
  hard-reload (R1 SPA fallback) resolves in-window; live `EventSource` shows
  **incremental** delivery (first frame before the upstream gap). HTTP seam
  (catalog, R1, R2, SSE framing) verified headlessly in CP3 — see
  [docs/spikes/cp3-embed.md](./docs/spikes/cp3-embed.md).
- [ ] **(CP5, GUI)** Real chat **streams tokens + the pinned TodoCard renders
  live** in WebKitGTK (done-criterion (d)). Needs an agent CLI installed or a
  BYOK key. SSE framing + CLI detection + BYOK/SSRF all verified headlessly —
  see [docs/spikes/cp5-acceptance.md](./docs/spikes/cp5-acceptance.md).
- [ ] **(CP5, live key)** BYOK **happy-path token stream** against a real public
  provider (a live API key is required; the SSRF guard allows loopback for local
  Ollama, but a public endpoint can't be reached offline). Route presence, SSRF
  block, and CLI-independence are verified headlessly; only the live upstream
  round-trip is manual.
- [ ] **(CP6/CP1, GUI offline — KI-1)** Showcase artifacts that pull **Google
  Fonts / Three.js / Tailwind from the network** render degraded **offline**
  (system-font drift, broken WebGL/CDN). Decide + verify the bundling/offline
  strategy at CP6 — see [docs/spikes/webkitgtk-render.md](./docs/spikes/webkitgtk-render.md).
- [ ] **(CP6/CP1, GUI — KI-4)** Ensure **GStreamer plugins** are present so
  `<video>` artifacts play in WebKitGTK; verify on the packaged app. **Addressed
  in config:** `appimage.bundleMediaFramework: true` bundles GStreamer into the
  AppImage; the `.deb` relies on the host's webkit2gtk pulling GStreamer. What
  remains is the in-engine `<video>` playback check on both packaged artifacts.
- [ ] **(CP6, clean machine)** Clean Ubuntu 24.04, **zero Node/pnpm**: install
  `.deb` + `.AppImage`, then run the whole acceptance set in-window (health-check,
  native catalog, ≥5 skills render, SSE chat, BYOK) and confirm **no orphan
  daemon** on exit. This is the umbrella manual acceptance (also the last box in
  CP6).

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
