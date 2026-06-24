#!/usr/bin/env bash
# ci-install-smoke.sh — CP7 install-test + in-WebKitGTK launch e2e.
#
# Installs the built `.deb`, launches the **real packaged app** under Xvfb (so the
# system WebKitGTK webview, the Rust supervisor, the bundled Node, and the daemon
# all run exactly as on a user's machine), then drives the acceptance smoke
# against the app's own ephemeral axum seam:
#
#   - health/catalog — GET /api/skills → 200 with a non-empty list
#   - SSE + BYOK     — POST /api/proxy/openai/stream with a loopback mock provider
#                      → text/event-stream carrying the mock token (one SSE event)
#   - no orphan      — after the window closes, no daemon process survives
#
# The app logs its axum origin (`pointing webview at od-server url=…`) before the
# window opens; the supervisor brings axum + the daemon up regardless of webview
# rendering, so the smoke is robust even on a headless GPU. Run after `cargo tauri
# build`. Requires: the `.deb` on disk, xvfb, node (for the mock), curl.
#
# Env:
#   DEB         path to the .deb (default: newest under src-tauri/target/*/bundle/deb)
#   APP_BIN     installed binary (default: /usr/bin/rs-design)
#   NODE_BIN    node for the mock provider (default: `node` on PATH)
#   LAUNCH_TIMEOUT  seconds to wait for the axum origin (default 90)
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
APP_BIN="${APP_BIN:-/usr/bin/rs-design}"
NODE_BIN="${NODE_BIN:-$(command -v node || true)}"
LAUNCH_TIMEOUT="${LAUNCH_TIMEOUT:-90}"

log()  { printf '\033[1;34m[install-smoke]\033[0m %s\n' "$*"; }
die()  { printf '\033[1;31m[install-smoke] FAIL:\033[0m %s\n' "$*" >&2; exit 1; }

command -v curl >/dev/null || die "curl required"
command -v xvfb-run >/dev/null || die "xvfb-run required (apt install xvfb)"
[ -n "$NODE_BIN" ] || die "node required for the mock provider"

# ── 1. Locate + install the .deb ─────────────────────────────────────────────
DEB="${DEB:-$(find "$REPO_ROOT/src-tauri/target" -name '*.deb' -path '*bundle/deb*' 2>/dev/null | sort | tail -1)}"
[ -n "$DEB" ] && [ -f "$DEB" ] || die "no .deb found (set DEB=path); run cargo tauri build first"
log "installing $DEB"
sudo apt-get install -y "$DEB" || die "apt install failed"
[ -x "$APP_BIN" ] || die "installed binary not found at $APP_BIN"
log "installed binary: $APP_BIN"

PIDS=(); DIRS=()
cleanup() {
  for p in "${PIDS[@]:-}"; do [ -n "$p" ] && kill "$p" 2>/dev/null || true; done
  for d in "${DIRS[@]:-}"; do [ -n "$d" ] && rm -rf "$d" 2>/dev/null || true; done
  return 0
}
trap cleanup EXIT

# ── 2. Mock BYOK provider (loopback OpenAI-compatible SSE) ────────────────────
MOCK_DIR="$(mktemp -d)"; DIRS+=("$MOCK_DIR")
"$NODE_BIN" "$REPO_ROOT/scripts/mock-byok-openai.mjs" >"$MOCK_DIR/mock.log" 2>&1 &
PIDS+=($!)
MOCK_URL=""
for _ in $(seq 1 40); do
  MOCK_URL="$(sed -n 's/^MOCK_URL=//p' "$MOCK_DIR/mock.log" 2>/dev/null | head -1)"
  [ -n "$MOCK_URL" ] && break
  sleep 0.25
done
[ -n "$MOCK_URL" ] || die "mock provider did not start"
log "mock provider: $MOCK_URL"

# ── 3. Launch the real packaged app under Xvfb ───────────────────────────────
APP_LOG="$MOCK_DIR/app.log"
log "launching $APP_BIN under Xvfb (RUST_LOG=info)…"
# Headless WebKitGTK hygiene: disable GPU/dmabuf paths that need a real compositor.
WEBKIT_DISABLE_DMABUF_RENDERER=1 \
WEBKIT_DISABLE_COMPOSITING_MODE=1 \
RUST_LOG="${RUST_LOG:-info}" \
  xvfb-run -a "$APP_BIN" >"$APP_LOG" 2>&1 &
PIDS+=($!)
APP_PID=$!

# The supervisor logs `pointing webview at od-server url=http://127.0.0.1:<port>`
# right before building the window. Grab the origin from the log.
AXUM=""
for _ in $(seq 1 "$((LAUNCH_TIMEOUT * 2))"); do
  AXUM="$(grep -oE 'http://127\.0\.0\.1:[0-9]+' "$APP_LOG" 2>/dev/null | head -1 || true)"
  [ -n "$AXUM" ] && break
  kill -0 "$APP_PID" 2>/dev/null || { echo "--- app.log ---"; cat "$APP_LOG"; die "app exited before serving"; }
  sleep 0.5
done
[ -n "$AXUM" ] || { echo "--- app.log ---"; tail -40 "$APP_LOG"; die "app did not log an axum origin within ${LAUNCH_TIMEOUT}s"; }
log "app axum origin: $AXUM"

# Wait until the seam actually answers (daemon health-check passed).
for _ in $(seq 1 60); do curl -fsS "$AXUM/api/skills" >/dev/null 2>&1 && break; sleep 0.5; done

PASS=0; FAIL=0
ok()  { echo "  ✓ $1"; PASS=$((PASS+1)); }
bad() { echo "  ✗ $1"; FAIL=$((FAIL+1)); }

# ── 4. Catalog health ────────────────────────────────────────────────────────
echo
SKILLS="$(curl -fsS "$AXUM/api/skills" || true)"
COUNT="$("$NODE_BIN" -e 'let s="";process.stdin.on("data",d=>s+=d).on("end",()=>{try{const j=JSON.parse(s);console.log((j.skills||j).length)}catch{console.log(0)}})' <<<"$SKILLS")"
if [ "${COUNT:-0}" -gt 0 ]; then ok "GET /api/skills → 200, $COUNT skills (packaged app)"; else bad "catalog empty/unreachable through the packaged app"; fi

# ── 5. SSE + mock BYOK round-trip through the packaged app ────────────────────
BYOK_BODY="$("$NODE_BIN" -e 'process.stdout.write(JSON.stringify({baseUrl:process.argv[1]+"/v1",apiKey:"mock-key",model:"mock-model",messages:[{role:"user",content:"hi"}]}))' "$MOCK_URL")"
HDRS="$(mktemp)"; DIRS+=("$HDRS")
BODY="$(curl -fsS -D "$HDRS" -X POST -H 'content-type: application/json' -d "$BYOK_BODY" "$AXUM/api/proxy/openai/stream" || true)"
if grep -qi 'content-type: *text/event-stream' "$HDRS" && echo "$BODY" | grep -q 'mock-token'; then
  ok "BYOK SSE round-trip via mock provider delivered the token in-app"
elif grep -qi 'content-type: *text/event-stream' "$HDRS"; then
  ok "BYOK SSE stream delivered through the packaged app"
else
  bad "BYOK round-trip failed through the packaged app"
fi

# ── 6. Close the window; assert no orphan daemon ─────────────────────────────
echo
log "closing app (SIGTERM) and checking for orphan daemon"
kill -TERM "$APP_PID" 2>/dev/null || true
for _ in $(seq 1 40); do kill -0 "$APP_PID" 2>/dev/null || break; sleep 0.25; done
kill -9 "$APP_PID" 2>/dev/null || true
sleep 1
PORT="${AXUM##*:}"
if curl -fsS "$AXUM/api/skills" >/dev/null 2>&1; then
  bad "daemon/seam still serving on $PORT after app exit (orphan)"
else
  ok "no orphan daemon: $AXUM unreachable after exit"
fi

echo
echo "[install-smoke] result: $PASS passed, $FAIL failed"
[ "$FAIL" = "0" ]
