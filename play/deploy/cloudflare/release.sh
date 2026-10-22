#!/bin/sh
# Release the committed HEAD to Cloudflare in one step:
#   ./release.sh            build and push the image, then deploy
#   ./release.sh --worker   deploy the Worker and site only (no new image)
# It refuses to run while the Worker or site sources have uncommitted changes,
# since `wrangler deploy` ships the working tree. It retries the deploy when
# Cloudflare answers 100146 (wrangler only waits 2 s for a new image version),
# waits for the bot to report the new image, runs the landing page's own
# snippets through /play/run, and commits the image bump.
set -eu
here="$(cd "$(dirname "$0")" && pwd)"
repo="$(git -C "$here" rev-parse --show-toplevel)"
site="https://rawr.airstrike.fyi"
logs="$repo/target/logs"
mkdir -p "$logs"

dirty="$(git -C "$repo" status --porcelain -- play/deploy/cloudflare site | grep -v '^?? play/deploy/cloudflare/package-lock.json$' || true)"
if [ -n "$dirty" ]; then
  echo "Commit these first. wrangler deploy ships the working tree:" >&2
  echo "$dirty" >&2
  exit 1
fi

cd "$here"
if [ "${1:-}" != "--worker" ]; then
  ./push-image.sh
fi
want="$(sed -nE 's/.*"PLAY_VERSION": "([^"]*)".*/\1/p' wrangler.jsonc)"

npm test >"$logs/release-test.log" 2>&1 || { echo "npm test failed; see $logs/release-test.log" >&2; exit 1; }

attempt=1
until npx wrangler deploy >"$logs/release-deploy.log" 2>&1; do
  if grep -q -e 100146 -e "Internal Server Error" "$logs/release-deploy.log" && [ "$attempt" -lt 5 ]; then
    echo "Cloudflare answered 100146 or a 500; retrying in 30 s (attempt $attempt)."
    attempt=$((attempt + 1))
    sleep 30
  else
    echo "wrangler deploy failed; see $logs/release-deploy.log" >&2
    exit 1
  fi
done
grep "Version ID" "$logs/release-deploy.log" || true

# The bot restarts onto the new image on the next /bot/health or cron tick.
i=0
until curl -fsS "$site/bot" 2>/dev/null | grep -q "$want"; do
  i=$((i + 1))
  [ "$i" -le 30 ] || { echo "the bot did not report image $want within 5 minutes" >&2; exit 1; }
  curl -fsS "$site/bot/health" >/dev/null 2>&1 || true
  sleep 10
done
echo "The bot runs image $want."

# Smoke test: the landing page's runnable snippets must still run.
node --input-type=module -e '
  const page = await (await fetch(process.argv[1] + "/")).text();
  const blocks = [...page.matchAll(/<code[^>]*data-run[^>]*>([\s\S]*?)<\/code>/g)];
  if (!blocks.length) throw new Error("no runnable snippets on the page");
  const text = (h) => h.replace(/<[^>]+>/g, "").replace(/&lt;/g, "<").replace(/&gt;/g, ">").replace(/&quot;/g, "\"").replace(/&#39;/g, "\x27").replace(/&amp;/g, "&");
  for (const [, html] of blocks) {
    const response = await fetch(process.argv[1] + "/play/run", { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify({ code: text(html) }) });
    if (!response.ok) throw new Error("run failed: " + response.status + " " + (await response.text()).slice(0, 200));
    console.log("ran a page snippet:", response.headers.get("content-type"));
  }
' "$site"

if ! git -C "$repo" diff --quiet -- play/deploy/cloudflare/wrangler.jsonc; then
  git -C "$repo" add play/deploy/cloudflare/wrangler.jsonc
  git -C "$repo" commit -q -m "Deploy the ?play image built from $want"
  echo "Committed the image bump."
fi
