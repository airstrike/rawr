#!/bin/sh
# Point wrangler.jsonc at a rawr-play image: its "image" and PLAY_VERSION, the
# image's tag. The Durable Object restarts a bot whose image is not that tag.
#   ./pin-image.sh IMAGE   the image just pushed (push-image.sh, the release)
#   ./pin-image.sh         the image Cloudflare runs now, so a deploy of the
#                          Worker and site keeps it (the site workflow,
#                          release.sh --worker)
# It prints the tag. wrangler has no flag for the container image, so the
# file names it.
set -eu
here="$(cd "$(dirname "$0")" && pwd)"
cd "$here"

image="${1:-}"
if [ -z "$image" ]; then
  # The application's own details, as deploy.sh reads them: the account-wide
  # list can retain an old image after the application has advanced.
  app="rawr-play-playbot"
  id="$(npx wrangler containers list --json 2>/dev/null |
    node -e 'let s="";process.stdin.on("data",(d)=>(s+=d)).on("end",()=>console.log(JSON.parse(s).find((x)=>x.name===process.argv[1])?.id??""))' "$app")"
  [ -n "$id" ] || { echo "pin-image: Cloudflare has no $app application" >&2; exit 1; }
  image="$(npx wrangler containers info "$id" --json 2>/dev/null |
    node -e 'let s="";process.stdin.on("data",(d)=>(s+=d)).on("end",()=>console.log(JSON.parse(s).configuration?.image??""))')"
fi
case "$image" in
  registry.cloudflare.com/*/rawr-play:?*) ;;
  *) echo "pin-image: not a rawr-play image: '$image'" >&2; exit 1 ;;
esac
tag="${image##*:}"

# -i.bak works with both GNU and BSD sed.
sed -i.bak -E \
  -e "s|\"image\": \"[^\"]*\"|\"image\": \"$image\"|" \
  -e "s|\"PLAY_VERSION\": \"[^\"]*\"|\"PLAY_VERSION\": \"$tag\"|" wrangler.jsonc
rm wrangler.jsonc.bak
grep -qF -e "\"image\": \"$image\"" wrangler.jsonc
grep -qF -e "\"PLAY_VERSION\": \"$tag\"" wrangler.jsonc
echo "$tag"
