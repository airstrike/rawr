#!/usr/bin/env bash
# Builds examples/app, an app that adds the rawr crate, as its own workspace
# and checks what it prints. It catches breakage on the path a Rust app takes
# when it embeds Rawr, which the iced checks don't cover.
#
# Usage: scripts/embed-check.sh
set -euo pipefail

repo="$(git -C "$(dirname "${BASH_SOURCE[0]}")" rev-parse --show-toplevel)"
export CARGO_TARGET_DIR="$repo/target/embed-app"

expected=$'113.87\n5 items, 108.88 total'
actual="$(cargo run -q --manifest-path "$repo/examples/app/Cargo.toml")"
if [[ "$actual" != "$expected" ]]; then
    printf 'examples/app printed:\n%s\nexpected:\n%s\n' "$actual" "$expected" >&2
    exit 1
fi
echo "examples/app: ok"
