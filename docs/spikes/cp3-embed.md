# CP3 — embed the real daemon (dev mode) end-to-end

> Status: **implemented** (CP3). The supervisor now spawns + health-checks the
> vendored Node daemon and the webview loads a populated catalog through
> axum→daemon. This note records the launch contract, the lifecycle design, and
> the headless verification evidence for the axum→daemon seam.

## What landed

- **`DaemonLauncher` trait + `DevNodeLauncher`** ([src-tauri/src/launcher.rs](../../src-tauri/src/launcher.rs)).
  Resolves the system `node` (absolute path; `OD_NODE_BIN` override) + the
  `tsc`-built daemon entry in the submodule (`apps/daemon/dist/cli.js`;
  `OD_CONTENT_ROOT` override). Builds the spawn command with the CP1
  daemon-packaging-spike contract and an **explicit child env** (env-hygiene
  rule). The `BundledLauncher` (CP6) slots in behind the same trait.
- **Supervisor lifecycle** ([src-tauri/src/supervisor.rs](../../src-tauri/src/supervisor.rs)):
  spawn promptly after reserving the daemon port; health-check; pipe
  stdout/stderr to `tracing` (target `daemon`); bounded restart-on-crash; kill on
  shutdown; `DaemonStatus` published over a `watch` channel.
- **First-run config**: seeds `<OD_DATA_DIR>/app-config.json` with telemetry
  **disabled**. The daemon defaults telemetry to opt-in (`metrics: true`) only
  when the `telemetry` key is *absent*, so writing it explicitly makes the
  embedded app private by default. Never clobbers an existing file.
- **Window gating** ([src-tauri/src/main.rs](../../src-tauri/src/main.rs)):
  `wait_ready(30s)` blocks the window on a reachable backend, but on failure the
  window still opens (dev keeps logs/devtools; the proxy surfaces 502s).

## Launch contract (locked by the CP1 daemon-packaging spike)

```
node <vendor>/apps/daemon/dist/cli.js daemon start --headless --port <p> --host 127.0.0.1
```

Child env (set explicitly; everything else cleared):

| Var | Value | Why |
|---|---|---|
| `HOME`, `PATH` | inherited (dev) | CP5 replaces `PATH` with login-shell reconstruction + `*_BIN` for agent-CLI spawn |
| `OD_PORT` | reserved ephemeral port | daemon binds it |
| `OD_BIND_HOST` | `127.0.0.1` | loopback only |
| `OD_DATA_DIR` | `$XDG_DATA_HOME/rs-design/dev` | out of the repo + submodule |
| `OD_INSTALLATION_DIR` + `OD_RESOURCE_ROOT` | submodule root | content (skills/, design-systems/, web `out/`); `OD_RESOURCE_ROOT` is rejected unless under `OD_INSTALLATION_DIR` (server.ts safe-base check) |

**Readiness** = stdout `[od] listening on …` line **AND** `GET /api/skills`
returns 200. Sub-bounds: 10s skills poll after the listening line; 30s overall.

## R1 SPA fallback (deferred from CP2) — satisfied by proxy→daemon

The daemon serves the web `out/` via `express.static` **and** registers its own
SPA fallback (`apps/daemon/src/static-spa.ts` → `registerStaticSpaFallback`): a
GET with `Accept: text/html` for a non-`/api`/`/artifacts`/`/frames`/`/_next`
path that isn't on disk returns `out/index.html`. Because V1 proxies `/` to the
daemon, **R1 is satisfied without any axum-side SPA logic** — the catch-all
proxy forwards the request and the daemon's fallback answers. No reimplementation
in axum was needed; an axum-side fallback would only be required once `/` flips
to a native route (post-V1).

## Verification (headless axum→daemon seam)

Reproduce with [crates/od-server/examples/cp3_proxy_verify.rs](../../crates/od-server/examples/cp3_proxy_verify.rs):
spawn the daemon with the launch contract above, then
`OD_DAEMON_URL=http://127.0.0.1:<p> cargo run -p od-server --example cp3_proxy_verify`
serves the **real `od_server::router`** so curl exercises the exact seam the
webview uses.

| Check | Path (through axum) | Result |
|---|---|---|
| Catalog — skills | `GET /api/skills` | ✅ 155 |
| Catalog — design systems | `GET /api/design-systems` | ✅ 150 |
| Health | `GET /api/health` | ✅ `{ok:true, version:0.11.0}` |
| **R2** origin-root index | `GET /` | ✅ 200 `text/html` |
| **R2** `/_next/*` asset | `GET /_next/static/chunks/*.css` | ✅ 200 `text/css` |
| **R1** SPA fallback | `GET /projects`, `/settings`, `/onboarding` (`Accept: text/html`) | ✅ 200 `text/html` |
| R1 is html-only | `GET /nope.json` (`Accept: application/json`) | ✅ 404 (not masked) |
| **SSE** framing | `GET /api/projects/<id>/events` | ✅ 200, `content-type: text/event-stream`, **no** `content-encoding`, `transfer-encoding: chunked`, first `event: ready` frame arrives immediately |

The SSE check closes the **CP2-Task2 deferral** (line 58/74 in TODO): CP2 verified
framing against a *mock* SSE upstream; here it's the *real daemon* SSE source
(`/api/projects/:id/events`) behind axum, with `cache-control: no-transform`
preserved and no buffering/compression.

## GUI-gated remainder (manual)

The one item these headless checks can't cover is the real **`EventSource` inside
WebKitGTK** observing incremental delivery in the running window. It needs a real
display + the sanitized-env launch the CP1 spikes documented (the VS Code snap
shell crashes GTK on `GLIBC_PRIVATE`). The full `cargo tauri dev` launch — daemon
up → health-check → UI lists real skills/design-systems, deep-link hard-reload,
live `EventSource` — is the manual confirmation step on a clean desktop session;
every underlying HTTP behavior it depends on is verified above.
