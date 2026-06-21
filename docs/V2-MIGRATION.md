# V2 — Strangler-Fig Migration Playbook

V2 replaces the Node daemon route-by-route with Rust crates until the sidecar is deleted. The
mechanism is already built in V1 (CP2 route table + CP4 golden harness), so V2 is a sequence of
low-risk **route flips**, not an architectural change.

## The mechanism

`od-server` holds a route table mapping each path prefix to:
- `Proxy { upstream }` — forward to the Node sidecar (V1 default for everything), or
- `Native { service }` — handled by a Rust crate.

To migrate a route: implement the crate, capture golden fixtures from the pinned daemon, make
the fixtures pass, then flip that prefix `Proxy → Native`. Keep `--force-proxy` as the rollback
until it's proven in the running app.

## Migration order (easiest / highest-leverage first)

1. **Catalog reads** — `od-catalog`: `/api/skills`, `/api/design-systems`, `/api/templates`.
   File walks + frontmatter (`walkdir` + `gray_matter`). **Done early in V1 (CP4).**
2. **Persistence** — `od-store`: `/api/projects`, conversations, messages, tabs. `rusqlite`
   over the *same* `.od/app.sqlite` schema.
3. **BYOK proxy** — `od-proxy`: `/api/proxy/{provider}/stream`. `reqwest` + SSE normalization +
   SSRF guard with `ipnet` (block private/link-local/CGNAT/multicast/reserved; no redirects).
4. **Artifact parser + save/lint** — `od-artifacts`: `/api/artifacts/*`. Pure parsing.
5. **Agent transport + adapters** — `od-agents`: `/api/chat` (SSE) + CLI spawn. **The long pole.**
   One adapter end-to-end first (Claude Code `stream-json`), then ACP (one parser → six CLIs),
   then stragglers. `tokio::process` + line-delimited JSON.
6. **Prompt stack** — `od-prompt`: port `apps/daemon/src/prompts/*` assembly. Coupled to step 5.
7. **Export** — `od-artifacts`: ZIP (`zip`) + Markdown trivial; PDF via webview print; PPTX is
   agent-written.

## Why the V1 gotchas disappear in V2
- Rust spawns the CLIs directly with explicit env (PATH/`*_BIN` centralized in V1 CP5) → the
  GUI-PATH problem is already solved.
- `rusqlite` bundles SQLite → no `better-sqlite3` native addon.
- When the last prefix flips, delete the sidecar bundle (Node runtime + daemon + addons) and the
  binary shrinks.

## Per-route definition of done
See [CONTRACT.md](./CONTRACT.md#per-route-definition-of-done-v2). Golden tests are CI-gated (CP7),
so "golden pass before flip" is enforced automatically.
