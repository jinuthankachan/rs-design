# Spike: Frontend API-base wiring (CP1-Task2)

**Question:** how does the exported frontend decide *which origin* its API/SSE calls go to —
a **relative `/api`** that follows whatever serves the document, or a **build-time base URL**
(`NEXT_PUBLIC_*` / `assetPrefix` / a baked daemon host)? The answer dictates the CP2/CP3 wiring:
a relative base means the frontend reaches whatever origin serves it (zero rebuild per
environment); a build-time base would force us to inject our axum origin at build time and
rebuild the submodule's `out/` for our packaging.

**Verdict: pure relative `/api`. No build-time API base anywhere.** Every app request and every
SSE stream is an **origin-relative path** (`/api/*`, plus `/artifacts/*` and `/frames/*`). The
frontend's effective API origin is *whatever origin served the document* — at runtime, only ever
read as `window.location.origin`, never baked at build time. Under the locked **"axum from day
one"** topology that origin **is** axum, so the proxied/native route table receives every call
with **zero frontend changes and no per-environment rebuild**. This confirms (and hardens with a
built-bundle audit) the lighter-weight conclusion already noted in CP1-Task1
([static-export.md](./static-export.md), Finding #2).

**Environment:** audited against the CP0 submodule pin (`6afe7ea`), Next 16.2.6,
`output: 'export'`. Source tree `vendor/open-design/apps/web/src`; built artifact
`vendor/open-design/apps/web/out` (the same `out/` exercised in WebKitGTK at CP1-Task1).

## What "build-time base" would have looked like (and doesn't exist here)

| Mechanism we checked for | Found? | Evidence |
|---|---|---|
| `NEXT_PUBLIC_*` API/daemon base | ❌ | Only `NEXT_PUBLIC_*` in the tree is `NEXT_PUBLIC_NEWSLETTER_URL` (marketing signup), not an API base |
| `basePath` / `assetPrefix` in `next.config.ts` | ❌ | neither key present; assets are absolute-from-root `/_next/*` (matches CP1-Task1 R2) |
| Baked daemon host in a fetch base | ❌ | no `fetch()` / `EventSource` uses an absolute app-API origin (see audit below) |
| `<base href>` / service worker rewriting requests | ❌ | no `<base>` tag in any `out/*.html`; no `sw.js`/`service-worker.js` |
| Server runtime that could inject an origin | ❌ | static export — no SSR, no server actions, no runtime config |

## Audit — every API/SSE/WS call resolves origin-relative

**Source sweep** (`apps/web/src`):

- **171 `fetch()` call sites.** Every one that targets the app backend is an origin-relative
  string literal or a helper that returns one. Helpers verified:
  `projectRawUrl`/`projectFileUrl` → `/api/projects/{id}/raw/…`,
  `liveArtifactDetailUrl`/`liveArtifactPreviewUrl` → `/api/live-artifacts/…`,
  `terminalStreamUrl` → `/api/projects/{id}/terminals/{tid}/stream`,
  `DIAGNOSTICS_EXPORT_PATH` = `/api/diagnostics/export`. None prepend a host.
- **3 `EventSource` (SSE) constructions**, all relative:
  `/api/memory/events` (×2) and `terminalStreamUrl(…)`. No SSE URL carries an origin.
- **0 `WebSocket` / `ws://` / `wss://`** — the realtime transport is SSE only (reinforces the
  CP2 SSE-passthrough requirement; there is no WS seam to worry about).

**BYOK is not an exception.** Each provider helper calls
`streamProxyEndpoint('/api/proxy/{provider}/stream', …)` — anthropic/openai/google/senseaudio
are all **origin-relative**. The user-configured provider `baseUrl` (`https://api.anthropic.com`,
`https://api.openai.com/v1`, ollama `localhost`, …) is **request-body data** POSTed to that
relative endpoint; the browser never contacts the LLM directly. The SSRF guard therefore stays
entirely server-side (CP2 `od-proxy`), exactly as planned — the BYOK `baseUrl` values surfaced by
a `grep` are provider targets, **not** the daemon's own base.

**Even "external-looking" reads are proxied through the daemon, not called cross-origin:**
GitHub stars → `/api/github/open-design`, Discord presence → `/api/community/discord`.

**The only genuinely cross-origin frontend fetches** (neither is part of the API contract):
1. Newsletter signup → `NEXT_PUBLIC_NEWSLETTER_URL` (default `open-design.ai/subscribe`) — opt-in
   marketing, fire-and-forget.
2. PostHog telemetry → `${analyticsConfig.host}/i/v0/e/` — host comes from server-provided
   analytics config; **telemetry is off by default** (CP3 first-run writes telemetry-disabled
   config). Not a base for any app API call.

**Built-bundle sweep** (`apps/web/out`, the artifact we actually ship — strongest proof):

- **Zero hardcoded app-API base.** No chunk uses `http://127.0.0.1:<port>` (or any absolute
  origin) as a `fetch`/`EventSource` base.
- The `127.0.0.1:7456` strings that *do* appear are **user-facing help content**, not request
  targets: the **"Use Everywhere"** panel's i18n strings (18 locales) and copy-paste
  `curl`/`od`/MCP snippets that teach the user to drive the daemon. They pass through
  `substituteDaemonUrl(body, daemonUrl)` =
  `body.replace(/http:\/\/127\.0\.0\.1:7456/g, daemonUrl)`, where
  `daemonUrl = liveDaemonUrl = window.location.origin` (`IntegrationsView.tsx`). The `7456`
  literal is a **placeholder swapped at render time** with the live document origin — confirming
  again that the origin is read from `window.location`, never baked.

## The one nuance: dev rewrites are dev-only — axum owns proxying, not `next dev`

`next.config.ts` defines `rewrites()` mapping `/api`, `/artifacts`, `/frames` →
`http://127.0.0.1:${OD_PORT||7456}`. **These apply only under `next dev`** (the
`!isProd && !isServerOutput` branch); they are **absent from the static export**. Two
consequences for us:

- We never run `next dev`. CP3 "dev mode" serves the **prebuilt `out/`** through axum, so the
  `/api` · `/artifacts` · `/frames` proxying is **axum's job (CP2)**, not Next's. The catch-all
  `/*` proxy already covers all three prefixes — just don't assume Next is doing it.
- `/artifacts/*` and `/frames/*` are **first-class proxied prefixes alongside `/api/*`**. They
  are origin-relative in the frontend too, so the same-origin requirement below covers them.

## What this dictates for CP2/CP3 wiring

- **W1 — Same-origin serving is mandatory.** axum must serve **both** the static `out/` **and**
  `/api/*` (+ `/artifacts/*`, `/frames/*`) from the **same origin**. The frontend has no knob to
  point at a different API host; it only ever reaches `window.location.origin`. (Reinforces
  CP1-Task1 **R2**.)
- **W2 — No build-time injection, ever.** Do **not** add `NEXT_PUBLIC_*`, `basePath`,
  `assetPrefix`, or any baked origin to the submodule build. The submodule's stock `out/` is
  consumed as-is; nothing about our axum port (ephemeral, per-launch) needs to be known at build
  time. This removes "rebuild `out/` per environment" from the packaging story entirely.
- **W3 — Submodule-bump guard.** On an upstream bump, re-run the audit's invariant: no new
  `NEXT_PUBLIC_*` API base, and no `fetch()`/`EventSource` constructed with an absolute origin.
  Cheap CI grep over `apps/web` (see "How to reproduce"); fail the build if either appears.
- **W4 — "Use Everywhere" panel UX (out of contract, minor).** The panel renders copy-paste
  `curl`/`od`/MCP snippets pointed at `window.location.origin` = the **ephemeral axum port**.
  They work *live* (axum proxies `/api/*` → daemon), but the port changes each launch and is dead
  when the app is closed, so a copied snippet/MCP config goes stale. Not a V1 blocker and not a
  contract issue; flag for CP5/CP6 (e.g. surface a stable documented port or a note in that
  panel). Tracked here so it isn't rediscovered as a "bug" later.

## How to reproduce (audit + bump guard)

```bash
# from repo root, over the submodule frontend
cd vendor/open-design/apps/web

# 1. No build-time API base in config or env
grep -REn "NEXT_PUBLIC_|basePath|assetPrefix" next.config.ts src | grep -iv NEWSLETTER

# 2. No fetch()/EventSource constructed with an absolute origin (app API must be relative)
grep -REn "fetch\(|new EventSource|new WebSocket" src \
  | grep -E "https?://|wss?://" | grep -v "127.0.0.1:7456"   # 7456 hits are help-panel display text

# 3. Built bundle carries no hardcoded app-API base (only help-text 7456 placeholders)
grep -rn "127.0.0.1" out/_next/static/chunks/*.js   # all are useEverywhere snippets/i18n
```

Expect (1) and (2) to print nothing (the `-iv` filters drop the lone newsletter `NEXT_PUBLIC_`
and the help-panel `:7456` placeholders), and (3)'s hits to be exclusively `Use Everywhere` help
strings. Any other match means an origin leaked in on the bump — investigate before flipping
CP2/CP3 wiring assumptions.

## Implications for the roadmap

- **CP1-Task2:** answered — **relative `/api`, no build-time var.** Closes the open question; no
  per-environment rebuild of `out/`.
- **CP2:** W1 (same-origin static + `/api`·`/artifacts`·`/frames` from axum) and the catch-all
  proxy already cover this; W2/W3 are guardrails, not new work.
- **CP3:** serve prebuilt `out/` via axum; do **not** rely on `next dev` rewrites.
- **CP5/CP6:** revisit W4 (Use-Everywhere panel shows the ephemeral axum origin).
- **ARCHITECTURE.md:** "frontend reaches whatever origin serves it (relative `/api`)" upgraded from
  _assumption_ to _audited fact (source + built bundle)_.
</content>
</invoke>
