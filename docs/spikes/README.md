# CP1 Spike Notes

One short note per de-risking spike (CP1). Each note records: the question, what was tried, the
measured result, and the decision. These notes feed the packaging shape (CP6) and are reused as
regression checklists in V2.

Planned notes:
- `static-export.md` — does the Next frontend export and run in real WebKitGTK? (Open Q#1)
- `api-base-wiring.md` — how the exported app picks its API origin (relative `/api` vs build-time var)
- `daemon-packaging.md` — esbuild bundle + bundled Node + pruned deps under a PATH-stripped env (Open Q#2)
- `native-addons.md` — `better-sqlite3` + `node-pty` load under bundled Node on Ubuntu 24.04
- `webkitgtk-render.md` — ≥5 showcase skills rendered; CSS/WebGL diffs vs Chromium (the known-issues list)
