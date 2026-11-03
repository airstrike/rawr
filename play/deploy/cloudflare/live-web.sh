#!/bin/sh
# Put the rawr-web that the site serves now into public/web:
#   ./live-web.sh
# A Worker deploy replaces every asset, so a deploy that builds no rawr-web
# (the site stage, the bot stage run alone, `npm run release:worker`) ships
# the live one again. web/build.sh writes /web/SHA256SUMS; this script reads
# it from the site, fetches each file it names and checks its hash, so a
# partial or stale download never ships.
set -eu
here="$(cd "$(dirname "$0")" && pwd)"
site="${PLAY_SITE:-https://rawr.airstrike.fyi}"
out="$here/public/web"

rm -rf "$out"
mkdir -p "$out"
# No Accept-Encoding: the Worker sends each file as stored.
if ! curl -fsS -o "$out/SHA256SUMS" "$site/web/SHA256SUMS"; then
  echo "live-web: $site/web/SHA256SUMS is missing; run the Playground workflow, which builds and deploys rawr-web" >&2
  exit 1
fi
while read -r _ name; do
  case "$name" in
    "" | */* | .*) echo "live-web: unexpected name '$name' in SHA256SUMS" >&2; exit 1 ;;
  esac
  curl -fsS -o "$out/$name" "$site/web/$name"
done < "$out/SHA256SUMS"
node -e 'const fs = require("fs"), path = require("path"), crypto = require("crypto");
  const dir = process.argv[1];
  for (const line of fs.readFileSync(path.join(dir, "SHA256SUMS"), "utf8").split("\n").filter(Boolean)) {
    const [hash, name] = line.split(/ +/);
    const got = crypto.createHash("sha256").update(fs.readFileSync(path.join(dir, name))).digest("hex");
    if (got !== hash) { console.error("live-web: " + name + " does not match SHA256SUMS"); process.exit(1); }
  }' "$out"
ls -l "$out"
