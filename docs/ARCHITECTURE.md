# Architecture

> Status: CP0 scaffold. This is a living document; sections marked _(to confirm)_
> are verified during CP1 against the pinned upstream.

## What rs-design is

A native Ubuntu desktop build of **Open Design** (Apache-2.0) on **Tauri v2 / Rust**.
Two checkpoints of delivery:

- **V1** — a Tauri native shell that embeds the upstream **Node daemon as a sidecar**.
- **V2** — a strangler-fig migration replacing the daemon route-by-route with Rust crates
  until the sidecar is deleted. See [V2-MIGRATION.md](./V2-MIGRATION.md).

## V1 topology — "Rust proxy from day one" (locked decision)

The webview never talks to the Node daemon directly. The Rust supervisor runs an **axum**
server (`od-server`) that binds the webview-facing loopback port and forwards every request
through a **route table** to the Node daemon on a second, internal loopback port.

```
WebKitGTK webview  ──HTTP/SSE──▶  od-server (axum, route table)  ──HTTP/SSE──▶  Node daemon
  (Tauri window)                   :<ephemeral A>  every prefix = Proxy           :<ephemeral B>
                                          │                                       spawns agent CLIs
                                          └─ CP4: /api/skills,/design-systems,/templates = Native
```

Why this shape (vs. webview→daemon directly):
- **CORS becomes internal** — the daemon only ever sees loopback requests from axum.
- The real route surface is large, so the proxy must be a **catch-all** anyway _(to confirm)_.
- Every V2 migration becomes a single `Proxy → Native` route flip with **zero architecture change**.
- SSE-through-Rust is proven once, up front (CP2), instead of mid-migration.

## Components

| Component | Crate / location | Role |
|---|---|---|
| Native shell + supervisor | `src-tauri/` | window, webview, port selection, daemon lifecycle, env injection |
| HTTP/SSE server + route table | `crates/od-server` | webview-facing endpoint; Proxy/Native dispatch |
| Catalog (first native route) | `crates/od-catalog` | `/api/skills`, `/api/design-systems`, `/api/templates` (CP4) |
| Persistence / proxy / artifacts / agents / prompt | `crates/od-*` | stubs in V1; filled during V2 |
| Shared contract + golden fixtures | `crates/od-contract` | the "sacred" API types + parity fixtures |
| Upstream (sidecar source + content) | `vendor/open-design` | Node daemon + Next frontend + skills/design-systems |

## Pinned upstream

- Repo: `https://github.com/nexu-io/open-design`
- Default branch: `main`
- **Pinned commit: `6afe7eae156bfa29251a51fd0636649c257f7444`** (recorded by the submodule
  gitlink at `vendor/open-design`; bump deliberately, re-capture golden fixtures on bump).

## Verified upstream facts _(working assumptions; confirm at CP1)_

- Frontend **static-exports** by default (`apps/web/next.config.ts` → `output: 'export'`).
  **Verified in the real engine (CP1, [static-export.md](./spikes/static-export.md)):** renders,
  hydrates, and client-routes in WebKitGTK 2.52.3. It is a **pure client SPA** (only `/`, `/404`,
  `/_not-found`, `/desktop-pet` exist as HTML) — so axum must provide **SPA fallback →
  `index.html`** for deep links and preserve SSE on `/api`. Static chosen over `standalone`; no
  second Next sidecar.
- Frontend API origin is **pure relative `/api/*`** (+ `/artifacts/*`, `/frames/*`) — **no
  build-time base** (no `NEXT_PUBLIC_*` API var / `basePath` / `assetPrefix`). **Audited at CP1
  ([api-base-wiring.md](./spikes/api-base-wiring.md)), source + built bundle:** the origin is only
  ever read at runtime as `window.location.origin` = axum, so the route table receives every call
  with **zero frontend changes / no per-environment `out/` rebuild**. axum must therefore serve the
  static app **and** `/api`·`/artifacts`·`/frames` from the **same origin** (the catch-all covers
  all three). BYOK provider `baseUrl` is request-body data to relative `/api/proxy/*` (SSRF stays
  server-side). The `next.config` dev rewrites are **dev-only** — axum, not `next dev`, owns this
  proxying.
- **Daemon packaging (CP1, [daemon-packaging.md](./spikes/daemon-packaging.md), Open Q#2 →
  resolved):** ship `tsc` output (`dist/cli.js`) **+ a pruned prod `node_modules` + a bundled,
  stripped Node 24** — **not `pkg`**, and not a single esbuild file (`apps/packaged`'s esbuild uses
  `packages:external`, bundling only the first-party Electron entry). Reproduced with
  `pnpm --filter @open-design/daemon deploy --legacy --prod`. **Boots and serves `/api/skills`
  (155) under a stripped env (`env -i`, empty `PATH`)** — the daemon core needs **zero `PATH`**;
  only agent-CLI spawn does (CP5). **Size:** ~71 M pruned daemon + ~101 M stripped Node ≈ **172 M**
  sidecar (content shipped separately). CP3 env gotcha: `OD_RESOURCE_ROOT` is safe-base-checked, so
  the supervisor must also set `OD_INSTALLATION_DIR` when pointing at external content.
- **Two** native addons — **both verified to load + run under the bundled stripped Node on Ubuntu
  24.04** (CP1-Task4, [native-addons.md](./spikes/native-addons.md)): `better-sqlite3` (real SQL,
  static SQLite 3.53.1, glibc floor 2.29) and `node-pty` (real PTY **after compiling for
  linux-x64** — no prebuild ships; glibc floor 2.34). **No RPATH on either → no `patchelf`.** ABI
  137 (any Node 24.x). CP6 must compile node-pty (or skip the terminal).
- Daemon env: `OD_PORT` / `OD_BIND_HOST` / `OD_DATA_DIR`; readiness on stdout `[od] listening on <url>`
  (no `/health` route); CORS via `OD_ALLOWED_ORIGINS`.
- CLI discovery honors explicit `*_BIN` vars (`CLAUDE_BIN`, `CODEX_BIN`, …).
- Telemetry is config-file driven (`app-config.json`) → ship disabled.
- Prior art to port: `apps/desktop/src/main/runtime.ts` (Electron supervisor).
- **Artifacts render faithfully in WebKitGTK** (CP1-Task5, [webkitgtk-render.md](./spikes/webkitgtk-render.md)):
  6/6 showcase artifacts render correctly in WebKitGTK 2.52.3 (online); every modern CSS feature is
  supported and WebGL2/canvas-2D work — the "modern CSS differs" caution is downgraded. The real
  risk is **network, not engine**: offline, externally-linked Google Fonts (142 files) fall back to
  system fonts and Three.js/Tailwind CDN scenes break (KI-1). CP6: keep webview network + ship
  GStreamer plugins.

## See also
- [CONTRACT.md](./CONTRACT.md) — the API seam (do not break during V2)
- [PACKAGING.md](./PACKAGING.md) — build prerequisites + bundle shape
- [V2-MIGRATION.md](./V2-MIGRATION.md) — the route-flip playbook
- [../TODO.md](../TODO.md) — the CP0–CP7 task list
