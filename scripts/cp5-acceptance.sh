#!/usr/bin/env bash
# cp5-acceptance.sh — drive the CP5 behavioral acceptance criteria through the
# real axum seam (`cp5_seam_verify`), the exact path the webview uses:
#
#   1. CLI detection      — boot the daemon with a full PATH and list the agent
#                           CLIs it detects; report Claude Code + ≥1 other.
#   2. No-CLI degradation — boot the daemon in a deterministically CLI-free env
#                           (minimal PATH + empty OD_AGENT_HOME) and assert it
#                           still serves: GET /api/agents → 200, zero available.
#   3. BYOK + SSRF intact — even with no CLI, POST /api/proxy/{provider}/stream
#                           is reachable (not 404) and blocks private/link-local
#                           base URLs with 403 (server-side SSRF guard).
#
# The BYOK *happy path* (real token stream) needs a live API key (loopback mock
# providers are intentionally allowed by the SSRF guard but a real public
# endpoint can't be reached offline) — that's the documented live/manual step.
#
# Read-only w.r.t. the repo: boots throwaway daemons against temp data dirs.
# Usage: scripts/cp5-acceptance.sh
# Requires: node 24 + a built daemon (`pnpm --filter @open-design/daemon build`).
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
CONTENT_ROOT="${OD_CONTENT_ROOT:-$REPO_ROOT/vendor/open-design}"
CLI_JS="$CONTENT_ROOT/apps/daemon/dist/cli.js"
NODE_BIN="${OD_NODE_BIN:-$(command -v node || true)}"

[ -n "$NODE_BIN" ] || { echo "error: node not found (set OD_NODE_BIN)" >&2; exit 1; }
[ -f "$CLI_JS" ] || { echo "error: daemon not built at $CLI_JS — run: pnpm --filter @open-design/daemon build" >&2; exit 1; }
command -v jq >/dev/null 2>&1 || { echo "error: jq required" >&2; exit 1; }

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

wait_url() { # url
  for _ in $(seq 1 120); do
    curl -fsS "$1" >/dev/null 2>&1 && return 0
    sleep 0.5
  done
  return 1
}

# Build the axum seam binary once so per-mode boots are instant.
echo "[cp5] building cp5_seam_verify …"
( cd "$REPO_ROOT" && cargo build -q -p od-server --example cp5_seam_verify )
SEAM_BIN="$REPO_ROOT/target/debug/examples/cp5_seam_verify"
[ -x "$SEAM_BIN" ] || { echo "error: seam binary not built at $SEAM_BIN" >&2; exit 1; }

# boot_daemon <PATH> <extra-env...> ; echoes "PORT DATADIR" and records pid/dir.
boot_daemon() {
  local path_value="$1"; shift
  local port datadir
  port="$(free_port)"
  datadir="$(mktemp -d)"
  DIRS+=("$datadir")
  env -i HOME="${HOME}" PATH="$path_value" "$@" \
    OD_PORT="$port" OD_BIND_HOST=127.0.0.1 OD_DATA_DIR="$datadir" \
    OD_INSTALLATION_DIR="$CONTENT_ROOT" OD_RESOURCE_ROOT="$CONTENT_ROOT" \
    "$NODE_BIN" "$CLI_JS" daemon start --headless --port "$port" --host 127.0.0.1 \
    >"$datadir/daemon.log" 2>&1 &
  PIDS+=($!)
  if ! wait_url "http://127.0.0.1:$port/api/skills"; then
    echo "error: daemon did not become ready" >&2
    tail -20 "$datadir/daemon.log" >&2 || true
    exit 1
  fi
  echo "$port $datadir"
}

# start_seam <daemon-port> <datadir> ; echoes axum base url, records pid.
start_seam() {
  local dport="$1" datadir="$2"
  OD_DAEMON_URL="http://127.0.0.1:$dport" \
  OD_CONTENT_ROOT="$CONTENT_ROOT" \
  OD_DATA_DIR="$datadir" \
    "$SEAM_BIN" >"$datadir/seam.log" 2>&1 &
  PIDS+=($!)
  local axum=""
  for _ in $(seq 1 60); do
    axum="$(sed -n 's/^AXUM_URL=//p' "$datadir/seam.log" 2>/dev/null | head -1)"
    [ -n "$axum" ] && break
    sleep 0.25
  done
  [ -n "$axum" ] || { echo "error: seam did not print AXUM_URL" >&2; cat "$datadir/seam.log" >&2; exit 1; }
  wait_url "$axum/api/skills" || { echo "error: seam not serving" >&2; exit 1; }
  echo "$axum"
}

http_code() { curl -s -o /dev/null -w '%{http_code}' "$@"; }

# ── Mode 1: full PATH → CLI detection ───────────────────────────────────────
echo
echo "[cp5] Mode 1 — CLI detection (full PATH)"
read -r D1_PORT D1_DIR < <(boot_daemon "$PATH")
A1="$(start_seam "$D1_PORT" "$D1_DIR")"
note "seam: $A1  (daemon :$D1_PORT)"
AGENTS_JSON="$(curl -fsS "$A1/api/agents")"
AVAIL="$(echo "$AGENTS_JSON" | jq -r '.agents[] | select(.available) | "\(.id) \(.version // "?")"')"
AVAIL_COUNT="$(echo "$AGENTS_JSON" | jq '[.agents[] | select(.available)] | length')"
TOTAL="$(echo "$AGENTS_JSON" | jq '.agents | length')"
note "/api/agents → 200, $TOTAL probed, $AVAIL_COUNT available"
if [ -n "$AVAIL" ]; then echo "$AVAIL" | sed 's/^/      - /'; fi
HAS_CLAUDE="$(echo "$AGENTS_JSON" | jq '[.agents[] | select(.available and .id=="claude")] | length')"
HAS_OTHER="$(echo "$AGENTS_JSON" | jq '[.agents[] | select(.available and .id!="claude")] | length')"
if [ "$HAS_CLAUDE" -ge 1 ] && [ "$HAS_OTHER" -ge 1 ]; then
  ok "acceptance: Claude Code + ≥1 other CLI detected"
elif [ "$AVAIL_COUNT" -ge 1 ]; then
  note "partial detection on this machine (claude=$HAS_CLAUDE other=$HAS_OTHER) — install Claude Code + one other to assert the full criterion"
else
  note "no CLIs installed on this machine — detection path exercised; install some to see them listed"
fi

# ── Mode 2 + 3: CLI-free env → graceful degradation + BYOK/SSRF ─────────────
echo
echo "[cp5] Mode 2/3 — no-CLI degradation + BYOK/SSRF (minimal PATH, empty OD_AGENT_HOME)"
EMPTY_HOME="$(mktemp -d)"; DIRS+=("$EMPTY_HOME")
NODE_DIR="$(dirname "$NODE_BIN")"
# OD_AGENT_HOME scopes toolchain search strictly to the (empty) override home and
# skips system bins, so detection is deterministically CLI-free regardless of
# what's installed on this machine.
read -r D2_PORT D2_DIR < <(boot_daemon "$NODE_DIR" OD_AGENT_HOME="$EMPTY_HOME")
A2="$(start_seam "$D2_PORT" "$D2_DIR")"
note "seam: $A2  (daemon :$D2_PORT)"

AGENTS2="$(curl -fsS "$A2/api/agents")"
AVAIL2="$(echo "$AGENTS2" | jq '[.agents[] | select(.available)] | length')"
if [ "$AVAIL2" = "0" ]; then
  ok "no-CLI degradation: /api/agents → 200, 0 available (daemon still serves)"
else
  bad "expected 0 available CLIs in CLI-free env, got $AVAIL2"
fi

# SSRF guard: private/link-local base URL must be blocked with 403.
SSRF_BODY='{"baseUrl":"http://169.254.169.254/v1","apiKey":"x","model":"m","messages":[]}'
CODE_SSRF="$(http_code -X POST -H 'content-type: application/json' -d "$SSRF_BODY" "$A2/api/proxy/anthropic/stream")"
if [ "$CODE_SSRF" = "403" ]; then
  ok "SSRF guard intact: link-local baseUrl → 403 (no CLI required)"
else
  bad "SSRF block expected 403, got $CODE_SSRF"
fi

# BYOK reachability independent of CLI: endpoint exists (not 404); a body missing
# apiKey/model is a 400, proving the route validates without any agent CLI.
BAD_BODY='{"baseUrl":"https://api.anthropic.com"}'
CODE_BYOK="$(http_code -X POST -H 'content-type: application/json' -d "$BAD_BODY" "$A2/api/proxy/anthropic/stream")"
if [ "$CODE_BYOK" = "400" ]; then
  ok "BYOK reachable with no CLI: incomplete body → 400 (route present, validates)"
elif [ "$CODE_BYOK" = "404" ]; then
  bad "BYOK route missing through seam (404)"
else
  note "BYOK endpoint returned $CODE_BYOK for incomplete body (reachable, not 404)"
fi

echo
echo "[cp5] result: $PASS passed, $FAIL failed"
[ "$FAIL" = "0" ]
