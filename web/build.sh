#!/bin/sh
# Build rawr-web into target/wasm/site, ready to serve:
#   web/build.sh && python3 -m http.server -d target/wasm/site
# wasm-bindgen-cli must match the wasm-bindgen in Cargo.lock, so it is
# installed under target/wasm/tools at that version.
set -eu
cd "$(dirname "$0")/.."
export CARGO_INCREMENTAL=0 RUSTFLAGS='--cfg getrandom_backend="wasm_js"'
version=$(awk '/^name = "wasm-bindgen"$/ { getline; gsub(/"/, "", $3); print $3 }' Cargo.lock)
bindgen=target/wasm/tools/bin/wasm-bindgen
if ! "$bindgen" --version 2>/dev/null | grep -q "$version"; then
  CARGO_TARGET_DIR=target/wasm/tools-build cargo +stable install wasm-bindgen-cli \
    --version "$version" --locked --root target/wasm/tools
fi
CARGO_TARGET_DIR=target/wasm cargo build -p rawr-web --target wasm32-unknown-unknown --release
"$bindgen" --target web --no-typescript --out-dir target/wasm/site \
  target/wasm/wasm32-unknown-unknown/release/rawr_web.wasm
cp web/www/index.html target/wasm/site/
