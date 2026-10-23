#!/bin/sh
# Build the linux/amd64 rawr-play image with Apple `container` (no Docker),
# push it to Cloudflare's registry and point wrangler.jsonc at it.
#   ./push-image.sh            (CLOUDFLARE_ACCOUNT_ID overrides the account)
# Needs `npx wrangler login` first. Builds the committed HEAD. The release
# build needs more than the builder's default 2 GB, so the builder VM (26 GB; 20 GB ran out compiling rawr with a cold cache) is
# started with 20 GB and deleted (with its cache) afterwards.
set -eu
CLOUDFLARE_ACCOUNT_ID="${CLOUDFLARE_ACCOUNT_ID:-fce80b8101fbb2f85548560babc6c7bc}"
here="$(cd "$(dirname "$0")" && pwd)"
repo="$(git -C "$here" rev-parse --show-toplevel)"
sha="$(git -C "$repo" rev-parse --short=12 HEAD)"
registry=registry.cloudflare.com
image="$registry/$CLOUDFLARE_ACCOUNT_ID/rawr-play:$sha"
context="$repo/target/play/context"

rm -rf "$context"
mkdir -p "$context"
git -C "$repo" archive HEAD | tar -x -C "$context"
container builder delete --force >/dev/null 2>&1 || true
container builder start --cpus 4 --memory 26g
trap 'container builder delete --force >/dev/null 2>&1 || true' EXIT
container build --platform linux/amd64 --build-arg PLAY_VERSION="$sha" -f "$context/play/Dockerfile" -t "$image" "$context"
rm -rf "$context"

# `container image push` can't log in to registry.cloudflare.com (no Bearer
# challenge), so the image is saved as an OCI layout and pushed with crane,
# using a throwaway Docker config for the short-lived credentials.
crane="$(command -v crane || echo "$repo/target/tools/crane")"
[ -x "$crane" ] || { echo "needs crane (go-containerregistry) on PATH or at $crane" >&2; exit 1; }
oci="$repo/target/play/oci"
rm -rf "$oci" "$oci.tar"
mkdir -p "$oci"
container image save --platform linux/amd64 -o "$oci.tar" "$image"
tar -xf "$oci.tar" -C "$oci"
export DOCKER_CONFIG="$(mktemp -d "$repo/target/play/docker-config.XXXXXX")"
trap 'container builder delete --force >/dev/null 2>&1 || true; rm -rf "$DOCKER_CONFIG" "$oci" "$oci.tar"' EXIT
credentials="$(cd "$here" && npx wrangler containers registries credentials "$registry" --push --json)"
field() { printf '%s' "$credentials" | node -e "let s='';process.stdin.on('data',d=>s+=d).on('end',()=>process.stdout.write(JSON.parse(s)['$1']))"; }
field password | "$crane" auth login "$registry" -u "$(field username)" --password-stdin
"$crane" push "$oci" "$image"
container image delete "$image" >/dev/null 2>&1 || true

sed -i '' -E "s|\"image\": \"[^\"]*\"|\"image\": \"$image\"|" "$here/wrangler.jsonc"
# The Durable Object restarts a running bot whose image is not this one.
sed -i '' -E "s|\"PLAY_VERSION\": \"[^\"]*\"|\"PLAY_VERSION\": \"$sha\"|" "$here/wrangler.jsonc"
echo "pushed $image; now: npx wrangler deploy"
