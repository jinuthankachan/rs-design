# CP1 Spike Notes

One short note per de-risking spike (CP1). Each note records: the question, what was tried, the
measured result, and the decision. These notes feed the packaging shape (CP6) and are reused as
regression checklists in V2.

Notes:
- ✅ [`upstream-build.md`](./upstream-build.md) — upstream `pnpm install` + daemon/web builds (CP0 exit check)
- ✅ [`static-export.md`](./static-export.md) — Next export renders/hydrates/routes in real WebKitGTK 2.52.3; **verdict: static, not `standalone`** (Open Q#1). Harness: [`static-export/probe.py`](./static-export/probe.py)
- ✅ [`api-base-wiring.md`](./api-base-wiring.md) — exported app uses **pure relative `/api`** (+ `/artifacts`, `/frames`); no build-time base. Origin = `window.location.origin` = axum. Source + built-bundle audit; **verdict: zero per-environment rebuild**, dictates CP2 same-origin serving (W1–W4)
- ✅ [`daemon-packaging.md`](./daemon-packaging.md) — daemon = `tsc` JS + **pruned prod `node_modules`** + bundled **Node 24**, launched by absolute path. **Boots + serves `/api/skills` (155) under a stripped env (empty `PATH`)**; `better-sqlite3` loads clean. Pruned **145 M→71 M**; +stripped Node **101 M** ≈ **172 M** sidecar. **Open Q#2: bundled runtime, not `pkg`.** node-pty has no Linux binary (CP6 must compile or skip terminal)

Planned notes:
- `native-addons.md` — `better-sqlite3` + `node-pty` load under bundled Node on Ubuntu 24.04
- `webkitgtk-render.md` — ≥5 showcase skills rendered; CSS/WebGL diffs vs Chromium (the known-issues list)
