#!/usr/bin/env bash
# release-version.sh — compute the release version string for CP7 releases.
#
# Per the CP7 contract the release version is **our app version + the pinned
# upstream submodule SHA**, so every artifact is traceable to the exact Open
# Design tree it embeds (the V2 CONTRACT's submodule-bump provenance, surfaced at
# release time). Format:
#
#   <productVersion>+od.<short-upstream-sha>      e.g. 0.1.0+od.6afe7ea
#
# productVersion is read from src-tauri/tauri.conf.json (the single source of
# truth the bundler also uses); the upstream SHA from the vendored submodule.
#
# Prints `key=value` lines suitable for `>> "$GITHUB_OUTPUT"`:
#   product_version=0.1.0
#   upstream_sha=6afe7ea
#   version=0.1.0+od.6afe7ea
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
CONF="$REPO_ROOT/src-tauri/tauri.conf.json"
VENDOR="$REPO_ROOT/vendor/open-design"

[ -f "$CONF" ] || { echo "error: $CONF not found" >&2; exit 1; }

# Read version from tauri.conf.json. Prefer jq; fall back to a grep for CI hosts
# without jq.
if command -v jq >/dev/null 2>&1; then
  PRODUCT_VERSION="$(jq -r '.version' "$CONF")"
else
  PRODUCT_VERSION="$(grep -oE '"version"[[:space:]]*:[[:space:]]*"[^"]+"' "$CONF" | head -1 | sed -E 's/.*"([^"]+)"$/\1/')"
fi
[ -n "$PRODUCT_VERSION" ] && [ "$PRODUCT_VERSION" != "null" ] || { echo "error: could not read version from $CONF" >&2; exit 1; }

# Prefer the committed submodule gitlink so this works even when the submodule
# content isn't checked out (the gitlink SHA is the pinned upstream tree);
# fall back to the working-tree submodule HEAD.
UPSTREAM_SHA="$(git -C "$REPO_ROOT" ls-tree HEAD vendor/open-design 2>/dev/null | awk '{print substr($3,1,7)}')"
[ -n "$UPSTREAM_SHA" ] || UPSTREAM_SHA="$(git -C "$VENDOR" rev-parse --short HEAD 2>/dev/null || echo unknown)"

VERSION="${PRODUCT_VERSION}+od.${UPSTREAM_SHA}"

printf 'product_version=%s\n' "$PRODUCT_VERSION"
printf 'upstream_sha=%s\n'    "$UPSTREAM_SHA"
printf 'version=%s\n'         "$VERSION"
