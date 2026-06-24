#!/usr/bin/env bash
#
# fetch-node-runtime.sh — pin + place a Node 24 linux-x64 runtime as a bundle
# resource (CP6).
#
# The packaged app ships its own Node so the user needs zero Node/pnpm. The CP1
# native-addon spike proved both `.node` addons load under ABI 137 (any Node
# 24.x), so we pin *a* Node 24.x by exact version for reproducibility. The binary
# is stripped to match the ~101 M measured in the daemon-packaging spike, and the
# upstream LICENSE is copied alongside it (licensing guardrail — we redistribute
# the Node binary).
#
# Output (under the staging runtime dir, default src-tauri/bundle-resources/runtime):
#   runtime/node          stripped node 24 linux-x64 ELF
#   runtime/NODE_LICENSE  Node.js license text (redistribution attribution)
#   runtime/NODE_VERSION  the exact pinned version (provenance)
#
# Env:
#   NODE_VERSION   override the pinned version (default below)
#   RUNTIME_DIR    override the staging runtime dir
#   FORCE=1        re-download even if runtime/node already exists
set -euo pipefail

# Pinned Node 24.x. ABI NODE_MODULE_VERSION 137 — matches the better-sqlite3 /
# node-pty prebuilds validated in docs/spikes/native-addons.md. Bump deliberately
# (re-run scripts/verify-bundle.sh after any bump).
NODE_VERSION="${NODE_VERSION:-24.16.0}"
ARCH="linux-x64"

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
RUNTIME_DIR="${RUNTIME_DIR:-$REPO_ROOT/src-tauri/bundle-resources/runtime}"
NODE_DEST="$RUNTIME_DIR/node"

log() { printf '\033[1;34m[fetch-node]\033[0m %s\n' "$*"; }
die() { printf '\033[1;31m[fetch-node] ERROR:\033[0m %s\n' "$*" >&2; exit 1; }

if [[ -x "$NODE_DEST" && "${FORCE:-}" != "1" ]]; then
  have="$("$NODE_DEST" --version 2>/dev/null || true)"
  if [[ "$have" == "v$NODE_VERSION" ]]; then
    log "node v$NODE_VERSION already present at $NODE_DEST (FORCE=1 to refetch)"
    exit 0
  fi
  log "replacing $NODE_DEST ($have → v$NODE_VERSION)"
fi

command -v curl >/dev/null || die "curl is required"
command -v tar  >/dev/null || die "tar is required"

mkdir -p "$RUNTIME_DIR"
tarball="node-v${NODE_VERSION}-${ARCH}.tar.xz"
url="https://nodejs.org/dist/v${NODE_VERSION}/${tarball}"
shasums_url="https://nodejs.org/dist/v${NODE_VERSION}/SHASUMS256.txt"

tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

log "downloading $url"
curl -fsSL "$url" -o "$tmp/$tarball" || die "download failed: $url"

# Verify the published SHA256 (provenance of the redistributed binary).
if curl -fsSL "$shasums_url" -o "$tmp/SHASUMS256.txt" 2>/dev/null; then
  expected="$(grep " $tarball\$" "$tmp/SHASUMS256.txt" | awk '{print $1}')"
  if [[ -n "$expected" ]]; then
    actual="$(sha256sum "$tmp/$tarball" | awk '{print $1}')"
    [[ "$actual" == "$expected" ]] || die "SHA256 mismatch: got $actual, expected $expected"
    log "sha256 verified ($expected)"
  fi
else
  log "WARNING: could not fetch SHASUMS256.txt; skipping checksum verification"
fi

log "extracting node + LICENSE"
tar -xJf "$tmp/$tarball" -C "$tmp"
extracted="$tmp/node-v${NODE_VERSION}-${ARCH}"
[[ -f "$extracted/bin/node" ]] || die "node binary missing in tarball"

cp "$extracted/bin/node" "$NODE_DEST"
chmod 0755 "$NODE_DEST"

# strip: 118 M → ~101 M (daemon-packaging spike). Best-effort.
if command -v strip >/dev/null; then
  before="$(du -h "$NODE_DEST" | awk '{print $1}')"
  strip "$NODE_DEST" || log "WARNING: strip failed; shipping unstripped node"
  after="$(du -h "$NODE_DEST" | awk '{print $1}')"
  log "stripped node $before → $after"
fi

# Licensing guardrail: redistribute Node's LICENSE next to the binary.
if [[ -f "$extracted/LICENSE" ]]; then
  cp "$extracted/LICENSE" "$RUNTIME_DIR/NODE_LICENSE"
else
  log "WARNING: LICENSE not found in tarball"
fi
printf '%s\n' "$NODE_VERSION" > "$RUNTIME_DIR/NODE_VERSION"

log "verifying placed runtime"
placed="$("$NODE_DEST" --version 2>/dev/null || true)"
[[ "$placed" == "v$NODE_VERSION" ]] || die "placed node reports '$placed', expected v$NODE_VERSION"
log "done: $NODE_DEST ($placed, $(du -h "$NODE_DEST" | awk '{print $1}'))"
