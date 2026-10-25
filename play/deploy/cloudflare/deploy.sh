#!/bin/sh
# Deploy the Worker and site as wrangler.jsonc stands, then check them:
#   ./deploy.sh
# release.sh and the GitHub Actions release both call it. It runs the tests,
# deploys the container in one step (--containers-rollout=immediate),
# retries the deploy when Cloudflare answers 100146 (wrangler only waits 2 s
# for a new image version), a 500 or 10013 (an assets upload session's
# "unknown error", which passed on a re-run each time), waits for the bot to report the image
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
  # Since 2026-10-24 Cloudflare answers 100146 to wrangler's last request even
  # though the upload and the rollout went through. The bot's poll below proves
  # whether the new image runs, so a 100146 after the upload is not a failure.
  if grep -q "^Uploaded rawr-play" "$logs/release-deploy.log" && grep -q 100146 "$logs/release-deploy.log"; then
    echo "Cloudflare answered 100146 after the upload; checking the bot instead of deploying again."
    break
  fi
  if grep -q -e 100146 -e "Internal Server Error" -e "code: 10013" "$logs/release-deploy.log" && [ "$attempt" -lt 5 ]; then
    echo "Cloudflare answered 100146, a 500 or 10013; retrying in 30 s (attempt $attempt)."
    grep -e 100146 -e "Internal Server Error" -e "code: 10013" -e ERROR "$logs/release-deploy.log" | head -5
    attempt=$((attempt + 1))
    sleep 30
  else
    echo "wrangler deploy failed; the end of $logs/release-deploy.log:" >&2
    tail -40 "$logs/release-deploy.log" >&2
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
  import { readdirSync, readFileSync } from "node:fs";
  import { join, relative } from "node:path";
  // Every page of the site that holds a runnable block, read from the files
  // (index.html is its directory'"'"'s path, other pages drop .html).
  const root = process.argv[2];
  const pages = readdirSync(root, { recursive: true })
    .filter((file) => file.endsWith(".html") && readFileSync(join(root, file), "utf8").includes("data-run"))
    .map((file) => "/" + relative(root, join(root, file)).replace(/index\.html$/, "").replace(/\.html$/, ""))
    .map((path) => (path.length > 1 ? path.replace(/\/$/, "") : path));
  const text = (h) => h.replace(/<[^>]+>/g, "").replace(/&lt;/g, "<").replace(/&gt;/g, ">").replace(/&quot;/g, "\"").replace(/&#39;/g, "\x27").replace(/&amp;/g, "&");
  const sources = new Set();
  for (const path of pages) {
    const page = await (await fetch(process.argv[1] + path)).text();
    const found = [...page.matchAll(/<code[^>]*data-run[^>]*>([\s\S]*?)<\/code>/g)];
    if (!found.length) throw new Error("no runnable snippets on " + path);
    for (const [, html] of found) sources.add(text(html));
  }
  // Each distinct source once. Landing runs are rate limited per IP, so a
  // 429 waits as long as it asks and tries again.
  for (const code of sources) {
    for (;;) {
      const response = await fetch(process.argv[1] + "/play/run", { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify({ code }) });
      if (response.status === 429) {
        const wait = Number(/(\d+) s/.exec(await response.text())?.[1] ?? 30);
        console.log("rate limited; waiting " + wait + " s");
        await new Promise((resolve) => setTimeout(resolve, (wait + 1) * 1000));
        continue;
      }
      if (!response.ok) throw new Error("run failed: " + response.status + " " + (await response.text()).slice(0, 200));
      console.log("ran a site snippet:", response.headers.get("content-type"));
      break;
    }
  }
  console.log("ran " + sources.size + " distinct site snippets from " + pages.length + " pages");
' "$site" "$repo/site/public"
