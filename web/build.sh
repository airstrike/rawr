#!/bin/sh
# Build rawr-web into DIR, ready to serve:
#   web/build.sh && python3 -m http.server -d target/wasm/site
#   web/build.sh play/deploy/cloudflare/public/web    (the release does this)
# DIR defaults to target/wasm/site, which also gets the demo page
# (web/www/index.html). wasm-bindgen-cli must match the wasm-bindgen of the
# Cargo.lock a module is built from, so each version it takes is installed
# under target/wasm/tools/<version>.
# The module is the opt-level 3 build without its name section (D519).
# wasm-opt is not run: it cuts the raw size by 7 % but the brotli size grows
# by 1.5 %. The module gets a brotli copy (.br) at quality 11, which the
# Worker serves to a browser that takes brotli.
#
# Beside it goes the browser's universe (#1395): ?play's universe built for
# wasm32 by play/universe/web.sh, with the rawr-import that RAWR_IMPORT names
# (rawr-import on the PATH by default), as universe.js, universe_bg.wasm and
# universe.toml, the manifest a snippet compiles beside.
set -eu
cd "$(dirname "$0")/.."
out="${1:-target/wasm/site}"
export CARGO_INCREMENTAL=0 RUSTFLAGS='--cfg getrandom_backend="wasm_js"'

# bindgen LOCK: the wasm-bindgen-cli for the wasm-bindgen LOCK names,
# installed first if it isn't.
bindgen() {
  version=$(awk '/^name = "wasm-bindgen"$/ { getline; gsub(/"/, "", $3); print $3 }' "$1")
  tools="target/wasm/tools/$version"
  if ! "$tools/bin/wasm-bindgen" --version 2>/dev/null | grep -q "$version"; then
    CARGO_TARGET_DIR=target/wasm/tools-build cargo +stable install wasm-bindgen-cli \
      --version "$version" --locked --root "$tools" >&2
    rm -rf target/wasm/tools-build
  fi
  echo "$tools/bin/wasm-bindgen"
}

CARGO_TARGET_DIR=target/wasm cargo build -p rawr-web --target wasm32-unknown-unknown --release
mkdir -p "$out"
"$(bindgen Cargo.lock)" --target web --no-typescript --remove-name-section --out-dir "$out" \
  target/wasm/wasm32-unknown-unknown/release/rawr_web.wasm

sh play/universe/web.sh "${RAWR_IMPORT:-rawr-import}"
runner=target/wasm/universe/target/rawr/runner
"$(bindgen "$runner/Cargo.lock")" --target web --no-typescript --remove-name-section \
  --out-name universe --out-dir "$out" "$runner/target/wasm32-unknown-unknown/debug/play.wasm"
cp target/wasm/universe/Cargo.toml "$out/universe.toml"

# pack FILE: write FILE.br, brotli at quality 11 with a 16 MiB window.
pack() {
  node -e 'const fs = require("fs"), z = require("zlib"), c = z.constants;
    fs.writeFileSync(process.argv[1] + ".br", z.brotliCompressSync(fs.readFileSync(process.argv[1]),
      { params: { [c.BROTLI_PARAM_QUALITY]: 11, [c.BROTLI_PARAM_LGWIN]: 24 } }));' "$1"
}
pack "$out/rawr_web_bg.wasm"
pack "$out/universe_bg.wasm"

# SHA256SUMS names each file with its hash, as sha256sum writes it. A deploy
# that keeps the live rawr-web (play/deploy/cloudflare/live-web.sh) reads it
# from the site to know which files to fetch and to check them.
node -e 'const fs = require("fs"), path = require("path"), crypto = require("crypto");
  const dir = process.argv[1];
  const lines = fs.readdirSync(dir).filter((f) => f !== "SHA256SUMS" && fs.statSync(path.join(dir, f)).isFile()).sort()
    .map((f) => crypto.createHash("sha256").update(fs.readFileSync(path.join(dir, f))).digest("hex") + "  " + f + "\n");
  fs.writeFileSync(path.join(dir, "SHA256SUMS"), lines.join(""));' "$out"

if [ $# -eq 0 ]; then
  cp web/www/index.html "$out/"
fi
ls -l "$out"
