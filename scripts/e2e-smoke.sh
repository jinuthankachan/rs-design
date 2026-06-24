#!/usr/bin/env bash
# e2e-smoke.sh — CP7 minimal e2e through the real axum seam (the exact path the
# webview uses), offline and headless-friendly.
#
#   1. Catalog   — GET /api/skills through the seam → 200 with a non-empty list.
#   2. SSE + BYOK — POST /api/proxy/openai/stream with a loopback mock provider
#                   baseUrl → a `text/event-stream` response that carries the
#                   provider's streamed token back through the seam. This single
#                   round-trip covers the CP7 "one SSE event + mock BYOK provider"
#                   e2e requirement: the daemon fetches the mock's OpenAI SSE and
#                   re-emits start/delta/end frames to the client.
#
# This is the seam-level e2e (no GUI); the in-WebKitGTK launch e2e lives in
# scripts/ci-install-smoke.sh, which drives the same assertions against the real
# packaged app under Xvfb.
#
# Read-only w.r.t. the repo: boots throwaway daemons against temp data dirs.
# Requires: node 24 + a built daemon (`pnpm --filter @open-design/daemon build`),
# cargo, curl.
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
CONTENT_ROOT="${OD_CONTENT_ROOT:-$REPO_ROOT/vendor/open-design}"
CLI_JS="$CONTENT_ROOT/apps/daemon/dist/cli.js"
NODE_BIN="${OD_NODE_BIN:-$(command -v node || true)}"

[ -n "$NODE_BIN" ] || { echo "error: node not found (set OD_NODE_BIN)" >&2; exit 1; }
[ -f "$CLI_JS" ] || { echo "error: daemon not built at $CLI_JS — run: pnpm --filter @open-design/daemon build" >&2; exit 1; }

PASS=0; FAIL=0
ok()   { echo "  ✓ $1"; PASS=$((PASS+1)); }
bad()  { echo "  ✗ $1"; FAIL=$((FAIL+1)); }
note() { echo "  • $1"; }

PIDS=(); DIRS=()
cleanup() {
  for p in "${PIDS[@]:-}"; do [ -n "$p" ] && kill "$p" 2>/dev/null || true; done
  for d in "${DIRS[@]:-}"; do [ -n "$d" ] && rm -rf "$d" 2>/dev/null || true; done
  return 0
}
trap cleanup EXIT

free_port() { "$NODE_BIN" -e 'const s=require("net").createServer();s.listen(0,"127.0.0.1",()=>{console.log(s.address().port);s.close();});'; }
wait_url() { for _ in $(seq 1 120); do curl -fsS "$1" >/dev/null 2>&1 && return 0; sleep 0.5; done; return 1; }

# ── Mock BYOK provider (loopback, OpenAI-compatible SSE) ────────────────────
echo "[e2e] starting mock BYOK provider"
MOCK_DIR="$(mktemp -d)"; DIRS+=("$MOCK_DIR")
"$NODE_BIN" "$REPO_ROOT/scripts/mock-byok-openai.mjs" >"$MOCK_DIR/mock.log" 2>&1 &
PIDS+=($!)
MOCK_URL=""
for _ in $(seq 1 40); do
  MOCK_URL="$(sed -n 's/^MOCK_URL=//p' "$MOCK_DIR/mock.log" 2>/dev/null | head -1)"
  [ -n "$MOCK_URL" ] && break
  sleep 0.25
done
[ -n "$MOCK_URL" ] || { echo "error: mock provider did not start" >&2; cat "$MOCK_DIR/mock.log" >&2; exit 1; }
note "mock provider: $MOCK_URL"

# ── Daemon ──────────────────────────────────────────────────────────────────
echo "[e2e] booting daemon"
D_PORT="$(free_port)"; D_DIR="$(mktemp -d)"; DIRS+=("$D_DIR")
env -i HOME="${HOME}" PATH="$PATH" \
  OD_PORT="$D_PORT" OD_BIND_HOST=127.0.0.1 OD_DATA_DIR="$D_DIR" \
  OD_INSTALLATION_DIR="$CONTENT_ROOT" OD_RESOURCE_ROOT="$CONTENT_ROOT" \
  "$NODE_BIN" "$CLI_JS" daemon start --headless --port "$D_PORT" --host 127.0.0.1 \
  >"$D_DIR/daemon.log" 2>&1 &
PIDS+=($!)
wait_url "http://127.0.0.1:$D_PORT/api/skills" || { echo "error: daemon not ready" >&2; tail -20 "$D_DIR/daemon.log" >&2; exit 1; }

# ── axum seam (the real router_with_catalog the webview talks to) ────────────
echo "[e2e] building + starting axum seam"
( cd "$REPO_ROOT" && cargo build -q -p od-server --example cp5_seam_verify )
SEAM_BIN="$REPO_ROOT/target/debug/examples/cp5_seam_verify"
[ -x "$SEAM_BIN" ] || { echo "error: seam binary missing at $SEAM_BIN" >&2; exit 1; }
SEAM_DIR="$(mktemp -d)"; DIRS+=("$SEAM_DIR")
OD_DAEMON_URL="http://127.0.0.1:$D_PORT" OD_CONTENT_ROOT="$CONTENT_ROOT" OD_DATA_DIR="$D_DIR" \
  "$SEAM_BIN" >"$SEAM_DIR/seam.log" 2>&1 &
PIDS+=($!)
AXUM=""
for _ in $(seq 1 60); do
  AXUM="$(sed -n 's/^AXUM_URL=//p' "$SEAM_DIR/seam.log" 2>/dev/null | head -1)"
  [ -n "$AXUM" ] && break
  sleep 0.25
done
[ -n "$AXUM" ] || { echo "error: seam did not print AXUM_URL" >&2; cat "$SEAM_DIR/seam.log" >&2; exit 1; }
wait_url "$AXUM/api/skills" || { echo "error: seam not serving" >&2; exit 1; }
note "seam: $AXUM  (daemon :$D_PORT)"

# ── 1. Catalog ───────────────────────────────────────────────────────────────
echo
echo "[e2e] 1. catalog through the seam"
SKILLS="$(curl -fsS "$AXUM/api/skills")"
COUNT="$("$NODE_BIN" -e 'let s="";process.stdin.on("data",d=>s+=d).on("end",()=>{try{const j=JSON.parse(s);console.log((j.skills||j).length)}catch{console.log(0)}})' <<<"$SKILLS")"
if [ "${COUNT:-0}" -gt 0 ]; then ok "GET /api/skills → 200, $COUNT skills"; else bad "GET /api/skills returned no skills"; fi

# ── 2. SSE + mock BYOK round-trip ────────────────────────────────────────────
echo
echo "[e2e] 2. BYOK SSE round-trip via mock provider"
BYOK_BODY="$("$NODE_BIN" -e 'process.stdout.write(JSON.stringify({baseUrl:process.argv[1]+"/v1",apiKey:"mock-key",model:"mock-model",messages:[{role:"user",content:"hi"}]}))' "$MOCK_URL")"
RESP_HEADERS="$(mktemp)"; DIRS+=("$RESP_HEADERS")
BODY="$(curl -fsS -D "$RESP_HEADERS" -X POST -H 'content-type: application/json' -d "$BYOK_BODY" "$AXUM/api/proxy/openai/stream")"
CT="$(grep -i '^content-type:' "$RESP_HEADERS" | head -1 | tr -d '\r')"
if echo "$CT" | grep -qi 'text/event-stream'; then ok "BYOK response is SSE ($CT)"; else bad "BYOK response not SSE: $CT"; fi
if echo "$BODY" | grep -q 'mock-token'; then
  ok "mock provider token streamed through the seam (delta delivered)"
elif echo "$BODY" | grep -qiE 'event: *(delta|end|start)'; then
  ok "SSE delta/end frames delivered through the seam"
else
  bad "no provider content/SSE frames in BYOK response"
  echo "$BODY" | head -5 | sed 's/^/      /'
fi

echo
echo "[e2e] result: $PASS passed, $FAIL failed"
[ "$FAIL" = "0" ]
