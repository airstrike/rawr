#!/usr/bin/env bash
# Regenerate rawr's std table (src/std_table/) for the pinned toolchain
# (D711 step 1). Run it after moving CHANNEL in
# importer/src/cargo/toolchain.rs and commit what changes.
#
#     scripts/std-table.sh          # rewrite src/std_table/
#     scripts/std-table.sh --check  # fail when it isn't current
#
# The table is taken for one target, so it reads the same on every host:
# what differs by target (`arch`, `simd`, `os`) is left out of it.
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
target="x86_64-unknown-linux-gnu"
channel="$(sed -n 's/^pub const CHANNEL: &str = "\(.*\)";$/\1/p' \
    "$root/importer/src/cargo/toolchain.rs")"
if [[ -z "$channel" ]]; then
    echo "std-table: no CHANNEL in importer/src/cargo/toolchain.rs" >&2
    exit 1
fi

# The driver CI already built (RAWR_IMPORT_DRIVER), or a debug one.
driver="${RAWR_IMPORT_DRIVER:-}"
if [[ -z "$driver" ]]; then
    rustup toolchain install "$channel" --profile minimal \
        --component rustc-dev --no-self-update >&2
    cargo "+$channel" build --manifest-path "$root/Cargo.toml" -p rawr-import \
        --features rustc-driver --bin rawr-import-driver >&2
    driver="$root/target/debug/rawr-import-driver"
fi
rustup target add --toolchain "$channel" "$target" >&2

sysroot="$(rustc "+$channel" --print sysroot)"
out="$root/target/std-table"
rm -rf "$out"
mkdir -p "$out"
DYLD_LIBRARY_PATH="$sysroot/lib" LD_LIBRARY_PATH="$sysroot/lib" \
    "$driver" --std-table "$sysroot/bin/rustc" "$out" "$target"
printf '%s %s\n' "$channel" "$target" > "$out/TOOLCHAIN"

table="$root/src/std_table"
if [[ "${1:-}" == "--check" ]]; then
    for file in paths.tsv items.tsv impls.tsv TOOLCHAIN; do
        if ! cmp -s "$out/$file" "$table/$file"; then
            echo "std-table: src/std_table/$file is not current for $channel;" \
                "run scripts/std-table.sh" >&2
            exit 1
        fi
    done
    echo "std-table: current for $channel"
else
    mkdir -p "$table"
    cp "$out/paths.tsv" "$out/items.tsv" "$out/impls.tsv" "$out/TOOLCHAIN" "$table/"
    echo "std-table: wrote src/std_table for $channel"
fi
