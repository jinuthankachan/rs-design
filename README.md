# rs-design

A **native Ubuntu desktop application** that delivers the
[Open Design](https://github.com/nexu-io/open-design) experience — the open-source,
local-first design agent — as a self-contained installable package (`.deb` + `.AppImage`),
with **no user-side Node/pnpm install required**.

Built on **Tauri v2 / Rust**. Open Design is Apache-2.0 (`nexu-io/open-design`); rs-design is
a derivative work — see [`NOTICE`](./NOTICE) and [`LICENSE`](./LICENSE).

> **Status:** CP0 complete — repo + Cargo workspace skeleton + vendored upstream are in place
> and the non-tauri crates compile. The app does not run yet. See [`TODO.md`](./TODO.md) for the
> CP0–CP7 roadmap.

## The plan in one paragraph

We ship in two checkpoints. **V1** is a Tauri native shell that embeds the existing upstream
**Node daemon as a sidecar** — maximum reuse, a working app fast. **V2** is a strangler-fig
migration that replaces the daemon one route at a time with Rust crates until the sidecar is
deleted. The **HTTP/SSE API contract** between the webview and the daemon is the stable seam that
makes this safe: a Rust **axum** reverse proxy (`od-server`) sits in front from day one, routing
each path prefix to either `Proxy` (the Node sidecar) or `Native` (Rust). Each V2 migration is a
single `Proxy → Native` flip, gated by **byte-identical golden tests**.

The bulk of Open Design's value — 132 Skills, 150 Design Systems, prompt templates, device frames —
is **portable data, reused as-is**. Rust only rebuilds the orchestration around it.

## Repo layout

```
rs-design/
├── Cargo.toml              # [workspace] — ALL crate members declared up front
├── rust-toolchain.toml     # pinned Rust 1.96
├── src-tauri/              # Tauri v2 app: window, webview, supervisor, route table, sidecar mgmt
├── crates/
│   ├── od-contract/        # shared API types + golden-test fixtures (the "sacred" seam)
│   ├── od-server/          # axum app + route table (Proxy|Native) → becomes the daemon in V2
│   ├── od-catalog/         # /api/skills, /api/design-systems, /api/templates  (V2 step 1, first native route)
│   ├── od-store/           # rusqlite over the upstream .od/app.sqlite schema   (V2 step 2)
│   ├── od-proxy/           # BYOK proxy + ipnet SSRF guard                       (V2 step 3)
│   ├── od-artifacts/       # <artifact> parser + export (zip/pdf/pptx)           (V2 steps 4, 7)
│   ├── od-agents/          # coding-agent CLI transport + adapters (long pole)   (V2 step 5)
│   └── od-prompt/          # prompt-stack assembly                               (V2 step 6)
├── vendor/open-design/     # upstream as a pinned git submodule (sidecar source + reused content)
└── docs/                   # ARCHITECTURE · CONTRACT · PACKAGING · V2-MIGRATION · spikes/
```

In V1 every crate except `od-server`/`od-catalog` is a documented stub. By the end of V2,
`od-server` **is** the daemon and the sidecar is deleted.

## Building

The eight library crates build with no system dependencies (the Tauri app package in
`src-tauri/` is named `rs-design`):

```bash
cargo check --workspace --exclude rs-design
```

Building and running the **Tauri app** (`src-tauri`) additionally needs WebKitGTK dev headers,
`patchelf`, and the Tauri CLI — see [`docs/PACKAGING.md`](./docs/PACKAGING.md) for the exact
`apt`/`cargo install` commands.

### Upstream submodule

`vendor/open-design` is pinned to a specific commit (recorded in
[`docs/ARCHITECTURE.md`](./docs/ARCHITECTURE.md)). After a fresh clone, initialize it:

```bash
git submodule update --init vendor/open-design
```

If it was fetched as a `blob:none` partial clone, the working tree may be empty (every path shows
as `D`/deleted on disk). Force-restore it — this lazily fetches the file blobs (large):

```bash
git -C vendor/open-design checkout -f HEAD
```

## Documentation

| Doc | What it covers |
|---|---|
| [`docs/ARCHITECTURE.md`](./docs/ARCHITECTURE.md) | V1 topology, components, pinned upstream SHA, verified upstream facts |
| [`docs/CONTRACT.md`](./docs/CONTRACT.md) | the HTTP/SSE API seam — never broken during V2 migration |
| [`docs/PACKAGING.md`](./docs/PACKAGING.md) | build prerequisites + bundle shape (`.deb` / `.AppImage`) |
| [`docs/V2-MIGRATION.md`](./docs/V2-MIGRATION.md) | the route-by-route `Proxy → Native` flip playbook |
| [`TODO.md`](./TODO.md) | the CP0–CP7 checkpoint task list |

## Licensing

Apache-2.0, derived from Open Design. Attribution is preserved in [`NOTICE`](./NOTICE). Some bundled
Skills carry their **own** preserved licenses (e.g. `guizang-ppt`); those per-folder `LICENSE` files
are retained verbatim and **must not be stripped** when content is vendored or bundled.

This project is not affiliated with or endorsed by the upstream authors.
