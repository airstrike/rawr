#!/bin/sh
# Print a hash of the external dependencies in the Cargo.lock files given
# (Cargo.lock by default), for a CI cache key:
#   scripts/deps-key.sh [LOCK...]
# A lock file's packages without a `source` are the workspace's own, whose
# versions every patch release bumps, so they are left out: the key moves only
# when a dependency is added, removed or changes version.
set -eu
[ "$#" -gt 0 ] || set -- Cargo.lock
for lock in "$@"; do
  awk '
    function flush() { if (block != "" && block ~ /\nsource = /) printf "%s", block; block = "" }
    /^\[\[package\]\]$/ { flush() }
    { block = block $0 "\n" }
    END { flush() }
  ' "$lock"
done | if command -v sha256sum >/dev/null; then sha256sum; else shasum -a 256; fi | cut -c1-16
