#!/bin/sh
# Build rawr-web into DIR, ready to serve:
#   web/build.sh && python3 -m http.server -d target/wasm/site
#   web/build.sh play/deploy/cloudflare/public/web    (the release does this)
# DIR defaults to target/wasm/site, which also gets the demo page
# (web/www/index.html). wasm-bindgen-cli must match the wasm-bindgen in
# Cargo.lock, so it is installed under target/wasm/tools at that version.
# The module is the opt-level 3 build without its name section (D519).
# wasm-opt is not run: it cuts the raw size by 7 % but the brotli size grows
# by 1.5 %. Its fonts go to DIR/fonts: Fira Mono, and the Noto fallbacks
# ?play renders with (play/fetch-fonts.sh), which the module fetches when a
# snippet needs them. The module and each font get a brotli copy (.br) at
# quality 11, which the Worker serves to a browser that takes brotli.
set -eu
cd "$(dirname "$0")/.."
out="${1:-target/wasm/site}"
export CARGO_INCREMENTAL=0 RUSTFLAGS='--cfg getrandom_backend="wasm_js"'
version=$(awk '/^name = "wasm-bindgen"$/ { getline; gsub(/"/, "", $3); print $3 }' Cargo.lock)
bindgen=target/wasm/tools/bin/wasm-bindgen
if ! "$bindgen" --version 2>/dev/null | grep -q "$version"; then
  CARGO_TARGET_DIR=target/wasm/tools-build cargo +stable install wasm-bindgen-cli \
    --version "$version" --locked --root target/wasm/tools
  rm -rf target/wasm/tools-build
fi
CARGO_TARGET_DIR=target/wasm cargo build -p rawr-web --target wasm32-unknown-unknown --release
mkdir -p "$out/fonts"
"$bindgen" --target web --no-typescript --remove-name-section --out-dir "$out" \
  target/wasm/wasm32-unknown-unknown/release/rawr_web.wasm

# The fonts, pinned and checked as play/fetch-fonts.sh checks its own.
fonts=target/play/fonts
play/fetch-fonts.sh "$fonts"
fira=target/wasm/FiraMono-Regular.ttf
mkdir -p target/wasm
if [ ! -f "$fira" ]; then
  curl -fsSL --retry 3 -o "$fira" https://raw.githubusercontent.com/mozilla/Fira/4.202/ttf/FiraMono-Regular.ttf
fi
if command -v sha256sum >/dev/null 2>&1; then sum=$(sha256sum "$fira" | cut -d' ' -f1)
else sum=$(shasum -a 256 "$fira" | cut -d' ' -f1); fi
[ "$sum" = 8c86f2963208a353c3435e28ecb38a99ab68f14d7433ba00c1822cef9a9c1b44 ] || { echo "checksum mismatch: $fira" >&2; exit 1; }
# pack FILE: write FILE.br, brotli at quality 11 with a 16 MiB window.
pack() {
  node -e 'const fs = require("fs"), z = require("zlib"), c = z.constants;
    fs.writeFileSync(process.argv[1] + ".br", z.brotliCompressSync(fs.readFileSync(process.argv[1]),
      { params: { [c.BROTLI_PARAM_QUALITY]: 11, [c.BROTLI_PARAM_LGWIN]: 24 } }));' "$1"
}
pack "$out/rawr_web_bg.wasm"
# web/src/fonts.rs names these.
for font in "$fira" "$fonts/NotoSansSymbols-Regular.ttf" "$fonts/NotoSansSymbols2-Regular.ttf" \
  "$fonts/NotoColorEmoji.ttf" "$fonts/NotoSansCJK-Regular.ttc"; do
  # The fonts are pinned, so a brotli copy is made once and kept beside them.
  [ "$font.br" -nt "$font" ] || pack "$font"
  cp "$font" "$font.br" "$out/fonts/"
done

if [ $# -eq 0 ]; then
  cp web/www/index.html "$out/"
fi
ls -l "$out" "$out/fonts"
