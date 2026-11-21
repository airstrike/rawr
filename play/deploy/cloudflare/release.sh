#!/bin/sh
# The GitHub Actions release (.github/workflows/release.yml) is the main
# path. This script stays for releasing from a Mac.
# Release the committed HEAD to Cloudflare in one step:
#   ./release.sh            build and push the image, then deploy
#   ./release.sh --worker   deploy the Worker and site only (no new image)
# It refuses to run while the Worker or site sources have uncommitted changes,
# since `wrangler deploy` ships the working tree. deploy.sh then tests,
# deploys and waits for the bot. Last, this script commits the image bump
# and pushes main.
# The site's build runs every snippet in rawr-web, so this script first
# fetches the set the site runs into target/web (live-web.sh). The modules
# themselves are never deployed: the Worker serves them from R2.
set -eu
here="$(cd "$(dirname "$0")" && pwd)"
repo="$(git -C "$here" rev-parse --show-toplevel)"

dirty="$(git -C "$repo" status --porcelain -- play/deploy/cloudflare site | grep -v '^?? play/deploy/cloudflare/package-lock.json$' || true)"
if [ -n "$dirty" ]; then
  echo "Commit these first. wrangler deploy ships the working tree:" >&2
  echo "$dirty" >&2
  exit 1
fi

cd "$here"
if [ "${1:-}" != "--worker" ]; then
  ./push-image.sh
else
  # Keep the image Cloudflare runs, which a GitHub release may have advanced
  # past the committed wrangler.jsonc.
  ./pin-image.sh >/dev/null
fi
./live-web.sh >/dev/null
./deploy.sh
want="$(sed -nE 's/.*"PLAY_VERSION": "([^"]*)".*/\1/p' wrangler.jsonc)"

if ! git -C "$repo" diff --quiet -- play/deploy/cloudflare/wrangler.jsonc; then
  git -C "$repo" add play/deploy/cloudflare/wrangler.jsonc
  git -C "$repo" commit -q -m "Deploy the ?play image built from $want"
  echo "Committed the image bump."
fi

# What the bot runs is also what GitHub shows.
if [ "$(git -C "$repo" rev-parse --abbrev-ref HEAD)" = main ]; then
  git -C "$repo" push -q origin main:main && echo "Pushed main to origin."
fi
