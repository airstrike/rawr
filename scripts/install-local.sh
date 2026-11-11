#!/bin/sh
# Installs rawr, cargo-rawr, rawr-import and rawr-import-driver from this
# checkout into ~/.cargo/bin with the fast `local` profile, then builds
# rawr's runtime for the toolchain (`rawr cache warm`). The two cargo calls
# share target/local, so dependencies compile once; the driver's call is
# separate so its `rustc-driver` feature doesn't reach rawr-import. The
# builds' JSON messages name every unit they produced or used, and
# scripts/prune-build.mjs removes the rest of target/local: each version
# gives the workspace's crates new hashes, and nothing else removes the
# old ones (#1731).
set -eu
repo="$(cd "$(dirname "$0")/.." && pwd)"
cd "$repo"
bin="${CARGO_HOME:-$HOME/.cargo}/bin"
profile=target/local
mkdir -p "$profile"
started="$profile/install-started"
messages="$profile/install-messages.json"
rm -f "$started"
: > "$started"
cargo build --profile local --locked -q --message-format=json-render-diagnostics \
  -p rawr-cli -p rawr-import-cli > "$messages"
cargo build --profile local --locked -q --message-format=json-render-diagnostics \
  -p rawr-import --features rustc-driver --bin rawr-import-driver >> "$messages"
node scripts/prune-build.mjs "$profile" "$started" "$messages"
rm -f "$started" "$messages"
for name in rawr cargo-rawr rawr-import rawr-import-driver; do
  cp "target/local/$name" "$bin/$name.new"
  mv "$bin/$name.new" "$bin/$name"
done
echo "installed $(git rev-parse --short HEAD)"
"$bin/rawr" cache warm
