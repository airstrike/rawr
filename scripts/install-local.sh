#!/bin/sh
# Installs rawr, cargo-rawr, rawr-import and rawr-import-driver from this
# checkout into ~/.cargo/bin with the fast `local` profile, then builds
# rawr's runtime for the toolchain (`rawr cache warm`). The two cargo calls
# share target/local, so dependencies compile once; the driver's call is
# separate so its `rustc-driver` feature doesn't reach rawr-import. Both go
# through scripts/cargo.mjs, which records the units each names and removes
# from target/local what neither does: each version gives the workspace's
# crates new hashes, and nothing else removes the old ones (#1731, #1732).
set -eu
repo="$(cd "$(dirname "$0")/.." && pwd)"
cd "$repo"
bin="${CARGO_HOME:-$HOME/.cargo}/bin"
node scripts/cargo.mjs build --profile local --locked -q -p rawr-cli -p rawr-import-cli
node scripts/cargo.mjs build --profile local --locked -q \
  -p rawr-import --features rustc-driver --bin rawr-import-driver
for name in rawr cargo-rawr rawr-import rawr-import-driver; do
  cp "target/local/$name" "$bin/$name.new"
  mv "$bin/$name.new" "$bin/$name"
done
echo "installed $(git rev-parse --short HEAD)"
"$bin/rawr" cache warm
