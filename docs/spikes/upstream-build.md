# Spike: upstream `pnpm install` + daemon/web builds (CP0 exit check)

**Question:** does the vendored upstream (`vendor/open-design`, pinned `6afe7ea`) install and
build its daemon + web with the host toolchain? Record time + size to feed CP1/CP6 packaging.

**Environment:** Ubuntu 24.04, Node 24.16.0, pnpm 10.33.2 (via corepack), `vendor/open-design`
working tree materialized.

## Results — all green

| Step | Command | Wall time | Output | Size |
|---|---|---|---|---|
| Install | `pnpm install` | ~1m52s (clean retry) | `node_modules/` | **1.4 G** |
| Daemon build | `pnpm --filter @open-design/daemon build` (`tsc`) | **10.5 s** | `apps/daemon/dist/` | **12 M** |
| Web build | `pnpm --filter @open-design/web build` (`next build`) | **52.2 s** | `apps/web/out/` | **51 M** |
|  |  |  | (`apps/web/.next/` intermediate) | 66 M |

## Findings

- **Web static-exports by default** — `next build` (Next.js 16.2.6) prerendered all routes as
  static HTML/SSG into `out/` (with `index.html`, `404.html`), no server runtime. Confirms the
  plan's core assumption (`shouldStaticExport = isProd && !isServerOutput`; `OD_WEB_OUTPUT_MODE`
  unset → static). The `next start` second-sidecar fallback is **not** needed for the default path.
  → de-risks Open Q#1 ahead of the CP1 WebKitGTK-load spike (`static-export.md` still owed: load
  `out/` in the real webview).
- **No monorepo build orchestrator** — no `turbo`; builds are per-package via `pnpm --filter`.
  Root `build` script is absent. Daemon = `tsc -p tsconfig.json`; web = `next build`.
- **`node-pty@1.1.0` build script was ignored** by pnpm's default (blocked) build-scripts policy
  (`pnpm approve-builds` to allow). It's one of the two native addons (`better-sqlite3`,
  `node-pty`) — must be explicitly approved/prebuilt when assembling the CP6 daemon bundle.
- **Network flakiness:** first `pnpm install` died at ~5m45s with `ECONNRESET` fetching
  `dompurify`. A retry with `--fetch-retries=5 --fetch-retry-mintimeout=10000` completed in 1m52s.
  CI (CP7) should set generous fetch-retry config.

## Implications for packaging (CP6)

- 1.4 G of dev `node_modules` is **not** what ships — CP6 prunes to prod deps + esbuild bundle
  (`apps/packaged`). This spike only proves the upstream builds; bundle-size measurement is the
  separate `daemon-packaging.md` spike.
- Ship the web `out/` (≈51 M) via the Tauri asset protocol.
