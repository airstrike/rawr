#!/bin/sh
# Put the rawr-web set the site runs now into target/web at the repository's
# root, for the site's build (site/src/lib/runs.ts runs the docs' snippets in
# it) and for local checks:
#   ./live-web.sh        from the live site, with no credentials
#   ./live-web.sh --r2   from the R2 bucket, with wrangler's (the site workflow)
# It reads the pointer (/web/current.json), then the set's SHA256SUMS
# (/web/<version>/SHA256SUMS), fetches each file it names and checks its
# hash, so a partial or stale download is never used. The pointer goes
# beside them as current.json. A set whose protocol differs from this
# checkout's (site/src/scripts/web.ts) is still fetched, with a warning: the
# build runs its snippets in it, and the page refuses it until they match.
set -eu
here="$(cd "$(dirname "$0")" && pwd)"
repo="$(git -C "$here" rev-parse --show-toplevel)"
site="${PLAY_SITE:-https://rawr.airstrike.fyi}"
out="$repo/target/web"
bucket="$(sed -nE 's/.*"bucket_name": "([^"]*)".*/\1/p' "$here/wrangler.jsonc")"
from=site
[ "${1:-}" != "--r2" ] || from=r2

# get PATH FILE: the site's PATH (/web/…), or its key in the bucket.
get() {
  if [ "$from" = r2 ]; then
    (cd "$here" && npx wrangler r2 object get "$bucket$1" --file "$2" --remote >/dev/null)
  else
    # No Accept-Encoding: the Worker sends each file as stored.
    curl -fsS -o "$2" "$site$1"
  fi
}

rm -rf "$out"
mkdir -p "$out"
if ! get /web/current.json "$out/current.json"; then
  echo "live-web: no /web/current.json ($from); run the Playground workflow, which builds rawr-web and publishes it" >&2
  exit 1
fi
named="$(node --input-type=module -e '
  import { readFileSync } from "node:fs";
  const [file, page] = process.argv.slice(1);
  const { parsePointer, WEB_PROTOCOL } = await import(page);
  let pointer = null;
  try { pointer = parsePointer(JSON.parse(readFileSync(file, "utf8"))); } catch {}
  if (!pointer) { console.error("live-web: current.json is not a pointer"); process.exit(1); }
  console.log(pointer.version, pointer.protocol, WEB_PROTOCOL);' "$out/current.json" "$repo/site/src/scripts/web.ts")"
set -- $named
version="$1"
if [ "$2" != "$3" ]; then
  echo "live-web: warning: the set is protocol $2 and this checkout's page speaks $3; the page won't load it until they match" >&2
fi

get "/web/$version/SHA256SUMS" "$out/SHA256SUMS"
while read -r _ name; do
  case "$name" in
    "" | */* | .*) echo "live-web: unexpected name '$name' in SHA256SUMS" >&2; exit 1 ;;
  esac
  get "/web/$version/$name" "$out/$name"
done < "$out/SHA256SUMS"
node -e 'const fs = require("fs"), path = require("path"), crypto = require("crypto");
  const dir = process.argv[1];
  for (const line of fs.readFileSync(path.join(dir, "SHA256SUMS"), "utf8").split("\n").filter(Boolean)) {
    const [hash, name] = line.split(/ +/);
    const got = crypto.createHash("sha256").update(fs.readFileSync(path.join(dir, name))).digest("hex");
    if (got !== hash) { console.error("live-web: " + name + " does not match SHA256SUMS"); process.exit(1); }
  }' "$out"
echo "live-web: set $version ($from) in $out"
ls -l "$out"
