#!/usr/bin/env bash
# Builds examples/app, an app that adds the rawr crate, as its own workspace
# and checks what it prints. It catches breakage on the path a Rust app takes
# when it embeds rawr, which the iced checks don't cover: an exported type
# with fields and methods, an exported function, and a compile error that
# reads like rustc's.
#
# Usage: scripts/embed-check.sh
set -euo pipefail

repo="$(git -C "$(dirname "${BASH_SOURCE[0]}")" rev-parse --show-toplevel)"
export CARGO_TARGET_DIR="$repo/target/embed-app"
manifest="$repo/examples/app/Cargo.toml"

expected=$'113.87\n5 items, 108.88 total\nAda x2\n54.44'
actual="$(cargo run -q --manifest-path "$manifest")"
if [[ "$actual" != "$expected" ]]; then
    printf 'examples/app printed:\n%s\nexpected:\n%s\n' "$actual" "$expected" >&2
    exit 1
fi

# A typo in a script: `fn main() -> Result` prints the error as rustc would.
expected='Error: error[E0609]: no field `itmes` on type `Order`
 --> scripts/typo.rs:4:45-4:50
 4 |     format!("{} x{}", order.customer, order.itmes)
   |                                             ^^^^^'
if cargo run -q --manifest-path "$manifest" -- scripts/typo.rs >/dev/null 2>"$CARGO_TARGET_DIR/typo.err"; then
    echo "examples/app accepted scripts/typo.rs" >&2
    exit 1
fi
actual="$(sed -n '/^Error: /,$p' "$CARGO_TARGET_DIR/typo.err")"
if [[ "$actual" != "$expected" ]]; then
    printf 'examples/app reported:\n%s\nexpected:\n%s\n' "$actual" "$expected" >&2
    exit 1
fi
echo "examples/app: ok"
