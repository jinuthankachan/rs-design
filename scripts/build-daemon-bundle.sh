#!/usr/bin/env bash
#
# build-daemon-bundle.sh — assemble the packaged daemon sidecar resource tree
# (CP6).
#
# Produces the runtime layout the BundledLauncher + the Node daemon expect. The
# daemon derives PROJECT_ROOT from its own dist location
# (resolveProjectRoot: <daemon>/dist → ../.. ) and reads its web export from
# PROJECT_ROOT/apps/web/out, while catalog content resolves from
# OD_RESOURCE_ROOT. So the daemon lives at runtime/apps/daemon and we point
# OD_RESOURCE_ROOT=OD_INSTALLATION_DIR=runtime — making PROJECT_ROOT==runtime and
# every path line up with one root:
#
#   runtime/                         <- PROJECT_ROOT = OD_RESOURCE_ROOT = OD_INSTALLATION_DIR
#     node                           <- bundled Node 24 (fetch-node-runtime.sh)
#     apps/daemon/{dist,node_modules,package.json,bin}   <- pnpm deploy, pruned
#     apps/web/out/                  <- Next static export (STATIC_DIR)
#     skills/ design-systems/ design-templates/ craft/ prompt-templates/ plugins/
#     frames/  community-pets/       <- remapped from assets/ (daemon reads <root>/frames)
#
# The model (tsc JS + pruned prod node_modules + bundled Node, not pkg/single-file)
# and the prune rules are locked by docs/spikes/daemon-packaging.md.
#
# Env:
#   VENDOR        upstream submodule (default vendor/open-design)
#   RUNTIME_DIR   output staging dir (default src-tauri/bundle-resources/runtime)
#   SKIP_BUILD=1  assume apps/daemon/dist + apps/web/out already built
#   SKIP_DEPLOY=1 reuse an existing runtime/apps/daemon (only re-copy content)
#   SKIP_NODE=1   don't auto-run fetch-node-runtime.sh
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
VENDOR="${VENDOR:-$REPO_ROOT/vendor/open-design}"
RUNTIME_DIR="${RUNTIME_DIR:-$REPO_ROOT/src-tauri/bundle-resources/runtime}"

log()  { printf '\033[1;34m[build-daemon]\033[0m %s\n' "$*"; }
warn() { printf '\033[1;33m[build-daemon] WARN:\033[0m %s\n' "$*" >&2; }
die()  { printf '\033[1;31m[build-daemon] ERROR:\033[0m %s\n' "$*" >&2; exit 1; }

[[ -d "$VENDOR" ]] || die "submodule not found at $VENDOR"

# Resolve the pnpm version the submodule pins (packageManager). A newer
# standalone pnpm on PATH refuses to run here (packageManager guard) AND silently
# ignores the submodule's `pnpm.overrides`, so invoke the pinned version through
# corepack regardless of what's on PATH. CI's `corepack enable` also works, but
# this is robust on a dev box with a different global pnpm.
PNPM_PIN="$(sed -n 's/.*"packageManager"[[:space:]]*:[[:space:]]*"pnpm@\([^"]*\)".*/\1/p' "$VENDOR/package.json" 2>/dev/null | head -1)"
if command -v corepack >/dev/null 2>&1 && [[ -n "$PNPM_PIN" ]]; then
  PNPM=(corepack "pnpm@$PNPM_PIN")
elif command -v pnpm >/dev/null 2>&1; then
  PNPM=(pnpm)
else
  die "pnpm is required (build-only dependency); install Node 24 (corepack) or pnpm"
fi
log "using pnpm: ${PNPM[*]} (submodule pins ${PNPM_PIN:-unknown})"

DAEMON_DIST="$VENDOR/apps/daemon/dist/cli.js"
WEB_OUT="$VENDOR/apps/web/out"

# 1. Ensure upstream build outputs exist (tsc daemon + Next static export).
if [[ "${SKIP_BUILD:-}" != "1" ]]; then
  if [[ ! -f "$DAEMON_DIST" ]]; then
    log "building daemon (tsc)…"
    ( cd "$VENDOR" && "${PNPM[@]}" --filter @open-design/daemon build )
  fi
  if [[ ! -d "$WEB_OUT" ]]; then
    log "building web static export…"
    ( cd "$VENDOR" && "${PNPM[@]}" --filter @open-design/web build )
  fi
fi
[[ -f "$DAEMON_DIST" ]] || die "daemon not built ($DAEMON_DIST); run without SKIP_BUILD"
[[ -d "$WEB_OUT"     ]] || die "web export missing ($WEB_OUT); run without SKIP_BUILD"

mkdir -p "$RUNTIME_DIR/apps"
DAEMON_OUT="$RUNTIME_DIR/apps/daemon"

# 2. pnpm deploy: self-contained {dist,bin,node_modules,package.json}. --legacy is
#    required (pnpm v10 refuses non-injected workspace deploys).
#
#    node-linker=hoisted is CRITICAL: the default isolated linker lays out
#    node_modules as SYMLINKS into a .pnpm store (e.g. @open-design/sidecar-proto
#    → ../.pnpm/…). Tauri's `bundle.resources` copier does NOT preserve symlinks,
#    so the packaged app loses every workspace dep and the daemon dies with
#    ERR_MODULE_NOT_FOUND '@open-design/sidecar-proto'. Hoisted emits real
#    directories (the daemon's 8 @open-design/* workspace deps included), which
#    survive the resource copy.
if [[ "${SKIP_DEPLOY:-}" != "1" ]]; then
  log "deploying pruned prod daemon (hoisted, symlink-free) → $DAEMON_OUT"
  rm -rf "$DAEMON_OUT"
  ( cd "$VENDOR" && "${PNPM[@]}" --filter @open-design/daemon deploy \
      --legacy --prod --config.node-linker=hoisted "$DAEMON_OUT" )
fi
[[ -f "$DAEMON_OUT/dist/cli.js" ]] || die "deploy produced no dist/cli.js"
# Guard: the workspace deps must be REAL dirs (not symlinks, not missing) or the
# packaged daemon will fail to resolve them after Tauri's symlink-dropping copy.
if [[ -L "$DAEMON_OUT/node_modules/@open-design/sidecar-proto" ]]; then
  die "node_modules/@open-design is symlinked — hoisted deploy failed; the bundle would break under Tauri's resource copy"
fi
[[ -f "$DAEMON_OUT/node_modules/@open-design/sidecar-proto/dist/index.mjs" ]] \
  || die "workspace dep @open-design/sidecar-proto missing from deploy"

NM="$DAEMON_OUT/node_modules"

# 3. Prune rules (docs/spikes/daemon-packaging.md: 145M → ~71M).
log "pruning daemon node_modules"
# node-pty: drop cross-platform prebuilds the Linux daemon can never load (−58M).
find "$NM" -type d -path '*/node-pty/prebuilds/win32-*' -prune -exec rm -rf {} + 2>/dev/null || true
find "$NM" -type d -path '*/node-pty/prebuilds/darwin-*' -prune -exec rm -rf {} + 2>/dev/null || true
# better-sqlite3: keep the prebuilt .node; drop build-time amalgamation/obj (−10M).
for bs in $(find "$NM" -type d -name better-sqlite3 2>/dev/null); do
  rm -rf "$bs/deps" "$bs/src" "$bs/build/Release/obj" "$bs/build/Release/.deps" 2>/dev/null || true
done
# dist .d.ts / .map are runtime-irrelevant (−6M).
find "$DAEMON_OUT/dist" -name '*.d.ts' -delete 2>/dev/null || true
find "$DAEMON_OUT/dist" -name '*.map'  -delete 2>/dev/null || true

# 4. Compile node-pty for linux-x64 (no Linux prebuild ships; pnpm skips its build
#    script). Best-effort: the daemon boots without it; only /api/terminal needs it
#    and that is out of V1 smoke scope (docs/spikes/native-addons.md).
PTY_DIR="$(find "$NM" -type d -name node-pty 2>/dev/null | head -1 || true)"
if [[ -n "$PTY_DIR" ]]; then
  if [[ -f "$PTY_DIR/build/Release/pty.node" ]]; then
    log "node-pty already compiled (pty.node present)"
  else
    log "compiling node-pty for linux-x64 (node-gyp rebuild)…"
    if ( cd "$PTY_DIR" && npx --yes node-gyp rebuild >/dev/null 2>&1 ); then
      log "node-pty compiled: $(du -h "$PTY_DIR/build/Release/pty.node" 2>/dev/null | awk '{print $1}')"
    else
      warn "node-pty compile failed — shipping without terminal (/api/terminal disabled, daemon still boots)"
    fi
  fi
fi

# 5. Web static export (STATIC_DIR = PROJECT_ROOT/apps/web/out).
log "copying web static export"
rm -rf "$RUNTIME_DIR/apps/web"
mkdir -p "$RUNTIME_DIR/apps/web"
cp -a "$WEB_OUT" "$RUNTIME_DIR/apps/web/out"

# 6. Catalog + agent content. cp -a preserves per-folder LICENSE/attribution files
#    (licensing guardrail). frames/community-pets live under assets/ upstream but
#    the daemon resolves them at <resource-root>/{frames,community-pets}.
log "copying content (skills, design-systems, templates, plugins, …)"
CONTENT_TOPLEVEL=(skills design-systems design-templates craft prompt-templates plugins)
for d in "${CONTENT_TOPLEVEL[@]}"; do
  if [[ -d "$VENDOR/$d" ]]; then
    rm -rf "$RUNTIME_DIR/$d"
    cp -a "$VENDOR/$d" "$RUNTIME_DIR/$d"
  else
    warn "content dir missing in submodule: $d"
  fi
done
# assets/* remapped to the root names the daemon expects.
for pair in "assets/frames:frames" "assets/community-pets:community-pets"; do
  src="${pair%%:*}"; dst="${pair##*:}"
  if [[ -d "$VENDOR/$src" ]]; then
    rm -rf "$RUNTIME_DIR/$dst"
    cp -a "$VENDOR/$src" "$RUNTIME_DIR/$dst"
  fi
done

# 7. Bundled Node runtime (idempotent; the fetch script no-ops if already pinned).
if [[ "${SKIP_NODE:-}" != "1" ]]; then
  if [[ ! -x "$RUNTIME_DIR/node" ]]; then
    log "fetching bundled Node runtime"
    RUNTIME_DIR="$RUNTIME_DIR" bash "$REPO_ROOT/scripts/fetch-node-runtime.sh"
  fi
fi

# 8. Report.
log "bundle assembled at $RUNTIME_DIR"
{
  printf '  %-22s %s\n' "node"           "$(du -sh "$RUNTIME_DIR/node" 2>/dev/null | awk '{print $1}')"
  printf '  %-22s %s\n' "apps/daemon"    "$(du -sh "$DAEMON_OUT" 2>/dev/null | awk '{print $1}')"
  printf '  %-22s %s\n' "apps/web/out"   "$(du -sh "$RUNTIME_DIR/apps/web/out" 2>/dev/null | awk '{print $1}')"
  printf '  %-22s %s\n' "content total"  "$(du -sh "${CONTENT_TOPLEVEL[@]/#/$RUNTIME_DIR/}" 2>/dev/null | tail -1 | awk '{print $1}')"
  printf '  %-22s %s\n' "RUNTIME total"  "$(du -sh "$RUNTIME_DIR" 2>/dev/null | awk '{print $1}')"
} || true
log "next: scripts/verify-bundle.sh to load-test the .node addons under bundled node"
