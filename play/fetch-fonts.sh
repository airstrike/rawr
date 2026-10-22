#!/bin/sh
# Fetch the fonts the ?play renderer bundles into DIR, pinned and checked:
#   play/fetch-fonts.sh target/play/fonts
# The image runs this in its own stage (play/Dockerfile); tests read the same
# files (play/tests/fonts.rs) so a Mac's system fonts cannot hide tofu.
# Noto: CJK (one collection: JP, KR, SC, TC, HK), color emoji, sans, mono
# and symbols, under the SIL Open Font License.
set -eu
dir="${1:?usage: fetch-fonts.sh DIR}"
mkdir -p "$dir"
noto=https://raw.githubusercontent.com/notofonts/notofonts.github.io/93fac8abebe6c7fed0c5b7d2456d8fa484fa146c/fonts
if command -v sha256sum >/dev/null 2>&1; then sum() { sha256sum "$1" | cut -d' ' -f1; }
else sum() { shasum -a 256 "$1" | cut -d' ' -f1; }; fi
while read -r hash url; do
  file="$dir/${url##*/}"
  if [ ! -f "$file" ] || [ "$(sum "$file")" != "$hash" ]; then
    curl -fsSL --retry 3 -o "$file" "$url"
  fi
  [ "$(sum "$file")" = "$hash" ] || { echo "checksum mismatch: $file" >&2; exit 1; }
done <<LIST
b76b0433203017ca80401b2ee0dd69350349871c4b19d504c34dbdd80541690a https://raw.githubusercontent.com/notofonts/noto-cjk/Sans2.004/Sans/OTC/NotoSansCJK-Regular.ttc
39ee3c587e10e89669b9ff32703261d10d5f9c4dd5ad147b6b5a1c5200591817 https://raw.githubusercontent.com/googlefonts/noto-emoji/v2.047/fonts/NotoColorEmoji.ttf
478c558ea716033cd60c03438f628dfa75694dcf6b5f6d505a2f05fd2b4f3823 $noto/NotoSans/hinted/ttf/NotoSans-Regular.ttf
65b5e2b2c4a1fba9ae8be1f026cb35b03dcb8886d9b2a4147054fde12f7e767d $noto/NotoSansMono/hinted/ttf/NotoSansMono-Regular.ttf
d0e98e9a2c046594c5021437273943be7e79e0fd980fde125279e22302212595 $noto/NotoSansSymbols/hinted/ttf/NotoSansSymbols-Regular.ttf
c4a0a80f0041ce4be81e2478faad22776d23edb98ae3f0d19bd37044820ecf9d $noto/NotoSansSymbols2/hinted/ttf/NotoSansSymbols2-Regular.ttf
LIST
