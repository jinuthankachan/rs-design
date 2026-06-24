#!/usr/bin/env bash
#
# verify-bundle.sh — post-bundle load-test of the native addons (CP6/CP7 gate).
#
# Catches ABI / glibc / missing-binary regressions on a submodule or Node bump
# (docs/spikes/native-addons.md "verify-after-bundle gate"). Runs under the
# *bundled* Node with `env -i` and an empty PATH — exactly the packaged GUI-launch
# reality — and `require()`s each addon, exercising one real op:
#   - better-sqlite3: open :memory:, CREATE/INSERT/SELECT, read sqlite_version()
#   - node-pty:       spawn /bin/echo in a real PTY, read its output (if compiled)
#
# Exit non-zero if better-sqlite3 fails (it is mandatory). node-pty is best-effort
# (terminal is out of V1 smoke scope); a missing/broken pty.node only warns.
#
# Env: RUNTIME_DIR (default src-tauri/bundle-resources/runtime)
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
RUNTIME_DIR="${RUNTIME_DIR:-$REPO_ROOT/src-tauri/bundle-resources/runtime}"
NODE_BIN="$RUNTIME_DIR/node"
NM="$RUNTIME_DIR/apps/daemon/node_modules"

log()  { printf '\033[1;34m[verify-bundle]\033[0m %s\n' "$*"; }
die()  { printf '\033[1;31m[verify-bundle] FAIL:\033[0m %s\n' "$*" >&2; exit 1; }

[[ -x "$NODE_BIN" ]] || die "bundled node missing/executable at $NODE_BIN (run build-daemon-bundle.sh)"
[[ -d "$NM" ]]       || die "daemon node_modules missing at $NM"

BSQLITE_DIR="$(find "$NM" -type d -name better-sqlite3 2>/dev/null | head -1 || true)"
PTY_DIR="$(find "$NM" -type d -name node-pty 2>/dev/null | head -1 || true)"
[[ -n "$BSQLITE_DIR" ]] || die "better-sqlite3 not found under $NM"

log "node:           $("$NODE_BIN" --version)  (abi $("$NODE_BIN" -p 'process.versions.modules'))"
log "better-sqlite3: $BSQLITE_DIR"
log "node-pty:       ${PTY_DIR:-<absent>}"

# Glibc floor sanity (informational): the highest GLIBC_ symbol any artifact needs.
if command -v objdump >/dev/null; then
  floor="$( { for f in "$NODE_BIN" \
        "$BSQLITE_DIR"/build/Release/*.node \
        "${PTY_DIR:-/nonexistent}"/build/Release/*.node; do
      [[ -f "$f" ]] && objdump -T "$f" 2>/dev/null | grep -oP 'GLIBC_[0-9.]+'
    done; } | sort -V | tail -1 )"
  log "glibc floor:    ${floor:-unknown}  (Ubuntu 24.04 provides GLIBC_2.39)"
fi

log "running addon load-test under bundled node (env -i, empty PATH)…"
set +e
env -i HOME="${HOME:-/tmp}" PATH="" "$NODE_BIN" \
  -e '
    const path = require("path");
    const { createRequire } = require("module");
    const bs = process.argv[1], pty = process.argv[2];
    const req = createRequire(path.join(process.cwd(), "x.js"));
    // better-sqlite3 (mandatory)
    const D = req(bs);
    const db = new D(":memory:");
    db.exec("create table t(x)");
    db.prepare("insert into t values(42)").run();
    const v = db.prepare("select x from t").get().x;
    const ver = db.prepare("select sqlite_version() v").get().v;
    if (v !== 42) { console.error("SQL roundtrip wrong:", v); process.exit(2); }
    console.log("OK better-sqlite3: sqlite " + ver + ", SELECT=" + v);
    // node-pty (best-effort)
    if (!pty) { console.log("SKIP node-pty: not present (terminal out of V1 smoke scope)"); process.exit(0); }
    let p;
    try { p = req(pty); } catch (e) { console.log("WARN node-pty require failed: " + e.message); process.exit(0); }
    let buf = "";
    const proc = p.spawn("/bin/echo", ["pty-works"], { cols: 80, rows: 24 });
    proc.onData((d) => (buf += d));
    proc.onExit(({ exitCode }) => {
      console.log("OK node-pty: read " + JSON.stringify(buf.trim()) + " exit " + exitCode);
      process.exit(0);
    });
    setTimeout(() => { console.log("WARN node-pty: no output within 3s"); process.exit(0); }, 3000);
  ' "$BSQLITE_DIR" "$PTY_DIR"
rc=$?
set -e
[[ $rc -eq 0 ]] || die "addon load-test failed (rc=$rc)"
log "PASS — bundled addons load and run under the pinned Node"
