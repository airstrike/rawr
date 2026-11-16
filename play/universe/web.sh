#!/bin/sh
# The browser's universe (#1395): ?play's universe built for wasm32, whose
# runner rawr-web instantiates once per Run (web/build.sh calls this).
#   sh play/universe/web.sh [rawr-import]
# It builds a copy of the universe in target/wasm/universe, without the
# headless shell's `[patch]`: in a browser iced runs on its own web shell,
# iced_winit, which draws into a canvas on the page. The copy builds as any
# project does under `rawr run --web`: rawr-import builds its crates and
# runner for wasm32 (#1394, #1752) and packs the runner with wasm-bindgen,
# here into target/wasm/universe/site as universe.js and universe_bg.wasm.
# RAWR_UNIVERSE_RUNNER makes the runner a universe's, which runs every
# snippet whose native calls it covers, and the profile variables optimize
# it for size, as rawr-web is.
set -eu
cd "$(dirname "$0")/../.."
import="${1:-rawr-import}"
copy=target/wasm/universe
mkdir -p "$copy"
# The universe's own files; its build and examples stay the copy's.
for entry in examples.sh examples.txt programs src; do
  rm -rf "${copy:?}/$entry"
  cp -R "play/universe/$entry" "$copy/$entry"
done
# Its manifest without the `[patch]` table and the line under it.
awk '/^\[patch\./ { skip = 1; next } skip && /^$/ { skip = 0 } !skip' \
  play/universe/Cargo.toml > "$copy/Cargo.toml"
sh "$copy/examples.sh"
# The universe's toolchain is the extraction nightly, as in the image's
# universe stage (play/Dockerfile).
export RUSTUP_TOOLCHAIN="${RUSTUP_TOOLCHAIN:-nightly-2026-07-11}" \
  RUSTFLAGS='--cfg getrandom_backend="wasm_js"' \
  CARGO_PROFILE_DEV_OPT_LEVEL=s \
  CARGO_PROFILE_DEV_DEBUG_ASSERTIONS=false \
  CARGO_PROFILE_DEV_OVERFLOW_CHECKS=false \
  RAWR_UNIVERSE_RUNNER=1
"$import" run --web --out "$copy/site" "$copy"
echo "web.sh: packed $copy/site"
