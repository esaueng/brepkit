#!/usr/bin/env bash
set -euo pipefail

# Re-pin every `[workspace.dependencies] brepkit-*.version` to `=<workspace
# version>`.
#
# release-please rewrites those values through per-crate jsonpath entries in
# release-please-config.json and writes a bare version, dropping the `=` that
# publish verification depends on (see the comment above the block in
# Cargo.toml). check-versions.sh fails the release PR when that happens; this
# script is the one-command fix. Safe to run any time — it is idempotent.

command -v cargo >/dev/null || {
  echo "❌ cargo is required but not installed."
  exit 1
}

ROOT=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
MANIFEST="$ROOT/Cargo.toml"

# Read the workspace version straight from the manifest rather than via
# `cargo metadata`, which refuses to run while the requirements are malformed.
WS_VERSION=$(sed -n '/^\[workspace\.package\]/,/^\[/p' "$MANIFEST" \
  | sed -n 's/^version *= *"\([^"]*\)".*/\1/p' | head -1)

if [ -z "$WS_VERSION" ]; then
  echo "❌ could not read [workspace.package] version from $MANIFEST"
  exit 1
fi

# Rewrite only `brepkit-* = {path = "...", version = "..."}` lines, leaving the
# `[workspace.package] version` and every third-party requirement untouched.
perl -i -pe '
  s/^(brepkit-[a-z]+ *= *\{path *= *"[^"]*", *version *= *")=?[^"]*(")/$1='"$WS_VERSION"'$2/
' "$MANIFEST"

echo "✅ pinned workspace sibling requirements to =$WS_VERSION"

if ! git -C "$ROOT" diff --quiet -- Cargo.toml; then
  git -C "$ROOT" --no-pager diff --stat -- Cargo.toml
fi
