#!/usr/bin/env bash
# Builds examples/rules, the workspace's `rules` package, an app that adds
# the rawr crate, and checks what it prints. It catches breakage on the path
# a Rust app takes when it embeds rawr, which the iced checks don't cover:
# an exported type, a typed handle, a reload, and errors that read like
# rustc's.
#
# Usage: scripts/embed-check.sh
set -euo pipefail

repo="$(git -C "$(dirname "${BASH_SOURCE[0]}")" rev-parse --show-toplevel)"
manifest="$repo/Cargo.toml"
mkdir -p "$repo/target"
err="$repo/target/rules.err"

run() {
    cargo run -q --manifest-path "$manifest" -p rules -- "$@"
}

same() {
    if [[ "$2" != "$3" ]]; then
        printf 'examples/rules %s printed:\n%s\nexpected:\n%s\n' "$1" "$2" "$3" >&2
        exit 1
    fi
}

totals=$'coffee       -14.50\ngroceries    -85.58\nincome      2500.00\nother        -56.49'
same "with rules.rs" "$(run)" "$totals"

# A typo fails as the rules load. `fn main() -> Result` prints the error as
# rustc would.
if run scripts/typo.rs >/dev/null 2>"$err"; then
    echo "examples/rules accepted scripts/typo.rs" >&2
    exit 1
fi
same "with scripts/typo.rs" "$(sed -n '/^Error: /,$p' "$err")" 'Error: error[E0609]: no field `payye` on type `Transaction`
 --> scripts/typo.rs:4:11-4:16
 4 |     if tx.payye.contains("Coffee") {
   |           ^^^^^'

# A wrong return type fails as the app looks up `category`.
if run scripts/wrong.rs >/dev/null 2>"$err"; then
    echo "examples/rules accepted scripts/wrong.rs" >&2
    exit 1
fi
mismatch='   | ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ expected `fn(Transaction) -> Option<String>`, found `fn(Transaction) -> String`'
same "with scripts/wrong.rs" "$(sed -n '/^Error: /,$p' "$err")" "Error: error[E0308]: mismatched types
 --> scripts/wrong.rs:3:1-3:39
 3 | fn category(tx: Transaction) -> String {
$mismatch"

# An edit saved over the rules is reloaded, and `category` runs it.
same "after an edit" "$(run rules.rs scripts/edited.rs)" "$totals
reloaded
coffee       -14.50
fuel         -41.00
groceries    -85.58
income      2500.00
streaming    -15.49"

# An edit that no longer fits `category` is refused, and the old rules
# keep running. The copy the app reloads lives beside its binary, so only
# the error's first and last lines are compared.
same "after a wrong edit" "$(run rules.rs scripts/wrong.rs 2>"$err")" "$totals
$totals"
same "after a wrong edit" "$(sed -n '/^error\[/,$p' "$err" | sed -n '1p;$p')" "error[E0308]: mismatched types
$mismatch"
echo "examples/rules: ok"
