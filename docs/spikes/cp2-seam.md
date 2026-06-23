# CP2 — the stable seam: supervisor + axum route table + SSE

> Status: **implemented** (CP2). This note records the topology decisions taken
> while wiring the supervisor and the asset-protocol/CSP choices, and how to run
> the real-`EventSource` SSE spike.

## Topology decision: the webview loads the **axum http origin**

The CP1 api-base spike ([api-base-wiring.md](./api-base-wiring.md)) found the
frontend reads its API base as `window.location.origin` and calls **relative
`/api/*`** (+ `/artifacts/*`, `/frames/*`) with no build-time base. For those
relative calls to reach the route table, the webview's origin must **be** axum.

So the supervisor points the main window at `http://127.0.0.1:<axum-port>` via
`WebviewUrl::External` — **not** the Tauri asset protocol (`tauri://localhost`).
Consequences:

- `window.location.origin` = the axum origin → relative `/api` hits the route
  table with **zero frontend changes**, exactly as the spike requires (W1
  same-origin serving).
- axum serves the app **and** proxies `/api` from one origin; SSE flows straight
  through the same origin (no cross-origin `EventSource`).
- The **Tauri asset protocol is not used for app content**, so its scope is not
  load-bearing for V1. The default capability stays minimal (`core:default`).

> Static frontend: the **daemon** serves `apps/web/out` via `express.static`
> (`server.ts:4996`) plus `/artifacts` and `/frames`, so in V1 the catch-all
> proxy already delivers the frontend by passthrough — axum needs **no** static
> file server. The one gap is **R1 SPA-fallback** (the daemon has no
> catch-all → `index.html` route, so deep-link reloads 404); that lands in CP3
> once the daemon is embedded (see TODO CP3). In V2, when the sidecar is deleted,
> `od-server` must serve `out/` + `/artifacts` + `/frames` natively (see
> [V2-MIGRATION.md](../V2-MIGRATION.md)). In CP2 axum is pure proxy, so an empty
> daemon yields a 502 — that still demonstrates "the webview loads through axum".

## Two ephemeral loopback ports

`supervisor::start()` binds **both** ports with `bind(127.0.0.1:0)` (OS-assigned):

- **axum port** — bound synchronously up front so it's known the instant we build
  the window; connections queue in the listen backlog until the async `serve`
  accepts them. `from_std` adopts the listener inside `tauri::async_runtime`.
- **daemon port** — reserved by bind-then-drop; CP3's daemon spawn binds it for
  real. (Brief TOCTOU window, acceptable on loopback for dev.)

Nothing binds a non-loopback interface, so nothing is reachable off-host.

## CSP

`tauri.conf.json` sets `app.security.csp` with `default-src`/`connect-src`/
`img-src` allowing `'self'` **plus** `http://127.0.0.1:*` / `http://localhost:*`
(and `ws://…` for HMR/websockets). `connect-src` is what permits `fetch` +
`EventSource` to the axum origin.

Caveat: a config CSP is injected by Tauri only for **Tauri-origin** content.
Because we load an **external http origin**, the *authoritative* response CSP is
whatever axum returns (none, for proxied daemon bytes, in V1). The config CSP is
kept as defense-in-depth and documents intent; setting a hardened response CSP
from axum is a CP6 packaging concern.

## SSE safety — proven two ways

1. **Automated** (`crates/od-server/tests/sse.rs`): a streaming HTTP client
   asserts the first frame arrives *before* the upstream's 300ms gap (i.e. not
   buffered), `text/event-stream` + `cache-control` are preserved, and
   `accept-encoding` never reaches the upstream (no compression). See CP2-Task2.

2. **Real `EventSource`** (`crates/od-server/examples/sse_spike.rs`): a runnable
   harness — mock SSE upstream → od-server proxy → an HTML page whose
   `EventSource` reconnects through the proxy. Open it in the actual WebKitGTK
   engine (or any browser) and watch frames tick live.

   ```sh
   cargo run -p od-server --example sse_spike
   # → prints  http://127.0.0.1:<port>/app  — open it
   ```

   **Pass:** the log fills with `tick 1, tick 2, …` one line per ~500ms (live,
   not all-at-once after a delay). Verified at the HTTP layer with `curl`:
   `content-type: text/event-stream` preserved, `transfer-encoding: chunked`
   (streamed), **no** `content-encoding` (uncompressed), frames ~500ms apart.

The live-webview observation inside the **packaged app** (real daemon SSE behind
axum) is the CP3 `(verify)` item — it needs the embedded daemon, which CP2 does
not yet spawn.

## What landed in CP2

- `od-server`: catch-all SSE-safe reverse proxy (`reqwest`, hop-by-hop stripping,
  streamed bodies, no compression) + a typed `Proxy | Native` route table with a
  dispatcher and `force_proxy` lever + `TraceLayer` route-resolution logging.
- `src-tauri`: `od-server` wired in; `supervisor` starts axum on an ephemeral
  loopback port, reserves the daemon port, points the webview at axum, and shuts
  axum down gracefully on window close.
