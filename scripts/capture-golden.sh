#!/usr/bin/env bash
# capture-golden.sh — capture golden fixtures for the CP4 catalog routes from the
# **pinned** Node daemon, the byte-for-byte oracle the Rust handlers must match.
#
# Read-only: it boots the daemon against a throwaway data dir, issues GET requests
# to the three catalog routes, and writes their (status, header subset, body) to
# crates/od-contract/tests/golden/fixtures/. It never mutates the daemon or the
# vendored content. Re-run it whenever the upstream submodule is bumped (the V2
# CONTRACT's W3 submodule-bump guard), then re-run the golden tests.
#
# Usage: scripts/capture-golden.sh
# Requires: node 24 + a built daemon (`pnpm --filter @open-design/daemon build`).
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
CONTENT_ROOT="${OD_CONTENT_ROOT:-$REPO_ROOT/vendor/open-design}"
CLI_JS="$CONTENT_ROOT/apps/daemon/dist/cli.js"
FIXTURES_DIR="$REPO_ROOT/crates/od-contract/tests/golden/fixtures"

NODE_BIN="${OD_NODE_BIN:-$(command -v node || true)}"
[ -n "$NODE_BIN" ] || { echo "error: node not found (set OD_NODE_BIN)" >&2; exit 1; }

DAEMON_PID=""
DATA_DIR=""
cleanup() {
  [ -n "$DAEMON_PID" ] && kill "$DAEMON_PID" 2>/dev/null || true
  [ -n "$DATA_DIR" ] && rm -rf "$DATA_DIR"
  return 0
}
trap cleanup EXIT

# Capture against an already-running daemon (OD_GOLDEN_BASE), else boot the
# pinned daemon ourselves against a throwaway data dir.
if [ -n "${OD_GOLDEN_BASE:-}" ]; then
  BASE="$OD_GOLDEN_BASE"
  echo "[capture] using existing daemon at $BASE"
else
  [ -f "$CLI_JS" ] || { echo "error: daemon not built at $CLI_JS — run: pnpm --filter @open-design/daemon build" >&2; exit 1; }
  PORT="$("$NODE_BIN" -e 'const s=require("net").createServer();s.listen(0,"127.0.0.1",()=>{console.log(s.address().port);s.close();});')"
  DATA_DIR="$(mktemp -d)"
  echo "[capture] booting pinned daemon on 127.0.0.1:$PORT (data: $DATA_DIR)"
  env -i HOME="$HOME" PATH="$PATH" \
    OD_PORT="$PORT" OD_BIND_HOST=127.0.0.1 OD_DATA_DIR="$DATA_DIR" \
    OD_INSTALLATION_DIR="$CONTENT_ROOT" OD_RESOURCE_ROOT="$CONTENT_ROOT" \
    "$NODE_BIN" "$CLI_JS" daemon start --headless --port "$PORT" --host 127.0.0.1 \
    >"$DATA_DIR/daemon.log" 2>&1 &
  DAEMON_PID=$!
  BASE="http://127.0.0.1:$PORT"
fi

for i in $(seq 1 60); do
  if curl -fsS "$BASE/api/skills" >/dev/null 2>&1; then break; fi
  sleep 0.5
  if [ "$i" = 60 ]; then
    echo "error: daemon did not become ready" >&2
    [ -n "$DATA_DIR" ] && tail -20 "$DATA_DIR/daemon.log" >&2
    exit 1
  fi
done

mkdir -p "$FIXTURES_DIR"

# name|path|arrayKey
ROUTES=(
  "skills|/api/skills|skills"
  "design-systems|/api/design-systems|designSystems"
  "design-templates|/api/design-templates|designTemplates"
)

for spec in "${ROUTES[@]}"; do
  IFS='|' read -r name path key <<<"$spec"
  hdr="$(mktemp)"
  curl -fsS -D "$hdr" -o "$FIXTURES_DIR/$name.json" "$BASE$path"
  status="$(awk 'NR==1{print $2}' "$hdr")"
  ctype="$(awk -F': ' 'tolower($1)=="content-type"{sub(/\r$/,"",$2);print $2}' "$hdr")"
  rm -f "$hdr"
  # Meta sidecar: the expected status + header subset + the array key the runner
  # normalizes on (readdir order is non-deterministic; the runner sorts by id).
  cat >"$FIXTURES_DIR/$name.meta.json" <<JSON
{
  "method": "GET",
  "path": "$path",
  "status": $status,
  "headers": { "content-type": "$ctype" },
  "arrayKey": "$key"
}
JSON
  count="$("$NODE_BIN" -e "console.log(require('$FIXTURES_DIR/$name.json')['$key'].length)")"
  echo "[capture] $name → status $status, $count entries"
done

echo "[capture] wrote fixtures to $FIXTURES_DIR"
