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
- Daemon bundles via **esbuild** (`apps/packaged/esbuild.config.mjs`), not `pkg`.
- **Two** native addons: `better-sqlite3` and `node-pty`.
- Daemon env: `OD_PORT` / `OD_BIND_HOST` / `OD_DATA_DIR`; readiness on stdout `[od] listening on <url>`
  (no `/health` route); CORS via `OD_ALLOWED_ORIGINS`.
- CLI discovery honors explicit `*_BIN` vars (`CLAUDE_BIN`, `CODEX_BIN`, …).
- Telemetry is config-file driven (`app-config.json`) → ship disabled.
- Prior art to port: `apps/desktop/src/main/runtime.ts` (Electron supervisor).

## See also
- [CONTRACT.md](./CONTRACT.md) — the API seam (do not break during V2)
- [PACKAGING.md](./PACKAGING.md) — build prerequisites + bundle shape
- [V2-MIGRATION.md](./V2-MIGRATION.md) — the route-flip playbook
- [../TODO.md](../TODO.md) — the CP0–CP7 task list
