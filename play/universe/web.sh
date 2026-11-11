#!/bin/sh
# The browser's universe (#1395): ?play's universe built for wasm32, whose
# runner rawr-web instantiates once per Run (web/build.sh calls this).
#   sh play/universe/web.sh [rawr-import]
# It builds a copy of the universe in target/wasm/universe, without the
# headless shell's `[patch]`: in a browser iced runs on its own web shell,
# iced_winit, which draws into a canvas on the page. Cargo builds the copy
# for wasm32 (CARGO_BUILD_TARGET), and rawr-import reads, extracts and
# generates for that target as Cargo builds it (#1394). rawr-import writes
# the runner it builds, optimized for size as rawr-web is, to
# target/wasm/universe/target/universe/runner, the directory
# RAWR_UNIVERSE_OUT names (#1752).
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
  CARGO_BUILD_TARGET=wasm32-unknown-unknown \
  RUSTFLAGS='--cfg getrandom_backend="wasm_js"' \
  CARGO_PROFILE_DEV_OPT_LEVEL=s \
  CARGO_PROFILE_DEV_DEBUG_ASSERTIONS=false \
  CARGO_PROFILE_DEV_OVERFLOW_CHECKS=false \
  RAWR_UNIVERSE_RUNNER=1 \
  RAWR_UNIVERSE_OUT="$copy/target/universe"
# A runner left from an earlier build is gone first, so only this build's
# can be found there.
runner="$RAWR_UNIVERSE_OUT/runner"
rm -f "$runner"
log="$copy/target/web.log"
mkdir -p "$copy/target"
status=0
# `run` builds the runner and then starts it, which a wasm32 module can't
# be here: once the runner is built, that start is the one failure left.
"$import" run "$copy" 2> "$log" || status=$?
cat "$log" >&2
if [ "$status" -ne 0 ] && ! grep -q "runner exited before connecting its protocol socket" "$log"; then
  exit "$status"
fi
if [ ! -f "$runner" ]; then
  echo "web.sh: $import wrote no runner to $RAWR_UNIVERSE_OUT" >&2
  exit 1
fi
echo "web.sh: built $runner"
