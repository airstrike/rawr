#!/bin/sh
# Deploy the Worker and site as wrangler.jsonc stands, then check them:
#   ./deploy.sh
# release.sh and the GitHub Actions release both call it. It runs the tests,
# deploys the container in one step (--containers-rollout=immediate),
# retries the deploy when Cloudflare answers 100146 (wrangler only waits 2 s
# for a new image version) or a 500, waits for the bot to report the image
# named by PLAY_VERSION, and runs the landing page's own snippets and the
# docs' through /play/run. Logs go to target/logs.
set -eu
here="$(cd "$(dirname "$0")" && pwd)"
repo="$(git -C "$here" rev-parse --show-toplevel)"
site="https://rawr.airstrike.fyi"
logs="$repo/target/logs"
mkdir -p "$logs"
cd "$here"

want="$(sed -nE 's/.*"PLAY_VERSION": "([^"]*)".*/\1/p' wrangler.jsonc)"

npm test >"$logs/release-test.log" 2>&1 || { echo "npm test failed; see $logs/release-test.log" >&2; exit 1; }

attempt=1
until npx wrangler deploy --containers-rollout=immediate >"$logs/release-deploy.log" 2>&1; do
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
# The rollout is immediate (one step, not wrangler's gradual default), so
# poll every 5 s and poke /bot/health each time.
i=0
until curl -fsS "$site/bot" 2>/dev/null | grep -q "$want"; do
  i=$((i + 1))
  [ "$i" -le 60 ] || { echo "the bot did not report image $want within 5 minutes" >&2; exit 1; }
  curl -fsS "$site/bot/health" >/dev/null 2>&1 || true
  sleep 5
done
echo "The bot runs image $want after $((i * 5)) s of polling."

# Smoke test: the runnable snippets on the landing page and in the docs must still run.
node --input-type=module -e '
  const blocks = [];
  for (const path of ["/", "/docs", "/docs/start", "/docs/language", "/docs/play"]) {
    const page = await (await fetch(process.argv[1] + path)).text();
    const found = [...page.matchAll(/<code[^>]*data-run[^>]*>([\s\S]*?)<\/code>/g)];
    if (!found.length) throw new Error("no runnable snippets on " + path);
    blocks.push(...found);
  }
  const text = (h) => h.replace(/<[^>]+>/g, "").replace(/&lt;/g, "<").replace(/&gt;/g, ">").replace(/&quot;/g, "\"").replace(/&#39;/g, "\x27").replace(/&amp;/g, "&");
  for (const [, html] of blocks) {
    const response = await fetch(process.argv[1] + "/play/run", { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify({ code: text(html) }) });
    if (!response.ok) throw new Error("run failed: " + response.status + " " + (await response.text()).slice(0, 200));
    console.log("ran a site snippet:", response.headers.get("content-type"));
  }
' "$site"
