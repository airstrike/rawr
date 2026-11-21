#!/bin/sh
# Fill an empty R2 bucket with a rawr-web set, once, before the first deploy
# of a Worker that serves /web from it:
#   ./seed-web.sh live        the set the live site serves now, by its
#                             /web/SHA256SUMS (a Worker that still ships the
#                             modules as assets serves it too)
#   ./seed-web.sh artifact    the newest `web` artifact a Playground build
#                             uploaded (gh, logged in to this repository),
#                             with its run's commit
# Then upload-web.mjs uploads the set and writes the pointer. Both use
# wrangler's login. The set is the protocol this checkout speaks
# (site/src/scripts/web.ts), so run it from the commit that introduced R2.
set -eu
here="$(cd "$(dirname "$0")" && pwd)"
repo="$(git -C "$here" rev-parse --show-toplevel)"
site="${PLAY_SITE:-https://rawr.airstrike.fyi}"
out="$repo/target/web-seed"

rm -rf "$out"
mkdir -p "$out"
case "${1:-}" in
  live)
    # No Accept-Encoding: the Worker sends each file as stored.
    curl -fsS -o "$out/SHA256SUMS" "$site/web/SHA256SUMS"
    while read -r _ name; do
      case "$name" in
        "" | */* | .*) echo "seed-web: unexpected name '$name' in SHA256SUMS" >&2; exit 1 ;;
      esac
      curl -fsS -o "$out/$name" "$site/web/$name"
    done < "$out/SHA256SUMS"
    commit=""
    ;;
  artifact)
    found="$(cd "$repo" && gh api 'repos/{owner}/{repo}/actions/artifacts?name=web&per_page=20' \
      --jq '[.artifacts[] | select(.expired | not)][0] | "\(.id) \(.workflow_run.head_sha)"')"
    [ -n "$found" ] && [ "$found" != "null null" ] || { echo "seed-web: no unexpired web artifact; use 'live'" >&2; exit 1; }
    set -- $found
    commit="$2"
    (cd "$repo" && gh api "repos/{owner}/{repo}/actions/artifacts/$1/zip") > "$out.zip"
    unzip -q -d "$out" "$out.zip"
    rm "$out.zip"
    ;;
  *)
    echo "usage: ./seed-web.sh live|artifact" >&2
    exit 2
    ;;
esac

node "$here/upload-web.mjs" "$out" --commit "$commit"
rm -rf "$out"
