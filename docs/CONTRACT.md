# The API Contract (the stable seam)

The HTTP/SSE contract between the frontend and the backend is **sacred**. It is the seam that
lets Rust replace Node one route at a time without the frontend ever knowing which answered.

## Rules

1. **Never change request/response shapes during V2 migration.** Match upstream byte-for-byte —
   status codes, JSON shapes, header semantics, and **SSE framing**. Contract changes are a
   separate, deliberate workstream *after* parity is reached.
2. **One route prefix per PR** during V2. Keep the proxy fallback (`--force-proxy`) until the
   native implementation passes golden tests.
3. **Golden tests gate every flip.** A route may move `Proxy → Native` only when its responses
   are byte-identical to the pinned Node daemon's (see below).

## Route surface (high level — enumerate precisely at CP1 from the pinned daemon)

- Catalog (file-walk reads): `GET /api/skills`, `GET /api/design-systems`,
  `GET /api/design-templates` — **V2 step 1 (CP4, done)**. (The early plan said
  `/api/templates`, but in the pinned daemon that path is the **SQLite-backed**
  user template store, not a file walk — it stays proxied and migrates with
  `od-store` in V2 step 2. The renderable-template catalogue is
  `/api/design-templates`.)
- Persistence: `/api/projects`, conversations, messages, tabs — V2 step 2
- BYOK proxy: `/api/proxy/{provider}/stream` (SSE) — V2 step 3
- Artifacts: `/api/artifacts/*` (save/lint) — V2 step 4
- Chat: `/api/chat` (SSE) + agent CLI spawn — V2 step 5 (long pole)
- Export, import, terminal, media, memory, automation, … — proxied; migrate later / as needed

> The proxy is a **catch-all** (`/*` → daemon), never an enumerated allowlist — new upstream
> routes keep working without code changes.

## SSE framing notes (the highest-risk part)

- Preserve `Content-Type: text/event-stream`; stream the body through axum with **no buffering**.
- Do **not** apply response compression to SSE.
- Preserve event/data/id/retry line framing exactly.
- Verify with the webview's real `EventSource`, not just `curl` (CP2).

## Golden-test harness (CP4; CI-gated at CP7)

- Fixtures are captured from the **pinned** Node daemon (`scripts/capture-golden.sh`, read-only).
- A fixture stores `(request, expected status, expected header subset, expected body)`.
- The runner hits the Rust handler and asserts equality, with a documented normalization step
  for legitimately non-deterministic fields.
- Lives in `crates/od-contract` + `tests/golden/`.

## Per-route definition of done (V2)
- [ ] Golden tests pass (byte-identical, incl. SSE framing)
- [ ] Error cases match upstream status codes/shapes
- [ ] Route moved from the proxy table to the native table
- [ ] No regression in the running app (manual smoke + e2e)
