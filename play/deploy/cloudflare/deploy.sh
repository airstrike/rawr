#!/bin/sh
# Deploy the Worker and site as wrangler.jsonc stands, then check them:
#   ./deploy.sh
# release.sh and the GitHub Actions release both call it. It runs the tests,
# deploys the container in one step (--containers-rollout=immediate),
# retries the deploy when Cloudflare answers 100146 (wrangler only waits 2 s
# for a new image version), a 500 or 10013 (an assets upload session's
# "unknown error", which passed on a re-run each time), waits for the bot to report the image
# named by PLAY_VERSION, and runs the landing page's own snippets and the
# docs' through /play/run. Logs go to target/logs. The site needs its
# packages: `npm ci` in site/.
set -eu
here="$(cd "$(dirname "$0")" && pwd)"
repo="$(git -C "$here" rev-parse --show-toplevel)"
site="https://rawr.airstrike.fyi"
logs="$repo/target/logs"
mkdir -p "$logs"
cd "$here"

want="$(sed -nE 's/.*"PLAY_VERSION": "([^"]*)".*/\1/p' wrangler.jsonc)"

# The site's build (site/dist) comes first: the tests read its pages, and
# wrangler deploy then merges it as built (SITE_BUILT) instead of building again.
# With SITE_BUILT=1 already set, as in the GitHub Actions release, which built
# and tested the site before the image, this build only merges.
node build.mjs >"$logs/release-build.log" 2>&1 || { echo "the build failed; see $logs/release-build.log" >&2; exit 1; }
npm test >"$logs/release-test.log" 2>&1 || { echo "npm test failed; see $logs/release-test.log" >&2; exit 1; }
npm --prefix "$repo/site" test >"$logs/release-site-test.log" 2>&1 || { echo "the site's tests failed; see $logs/release-site-test.log" >&2; exit 1; }

attempt=1
until SITE_BUILT=1 npx wrangler deploy --containers-rollout=immediate >"$logs/release-deploy.log" 2>&1; do
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

# /todo's private tabs: plan/private.mjs renders the local .Codex/ documents
# and uploads them to KV. Where they are missing (CI) it does nothing. A
# failed upload leaves the last one in place, so it warns and goes on.
if node "$repo/plan/private.mjs" >"$logs/release-private.log" 2>&1; then
  tail -1 "$logs/release-private.log"
else
  echo "warning: the private tabs were not uploaded; see $logs/release-private.log" >&2
fi

# Query the application itself: the account-wide list can retain an old image
# after the application and its running instance have advanced. A rollout that never places a
# healthy instance fails here with Cloudflare's own view, not in the snippets.
app="rawr-play-playbot"
application_id() {
  npx wrangler containers list --json 2>/dev/null |
    node -e 'let s="";process.stdin.on("data",(d)=>(s+=d)).on("end",()=>console.log(JSON.parse(s).find((x)=>x.name===process.argv[1])?.id??""))' "$app"
}
id="$(application_id || true)"
i=0
until [ -n "$id" ] && npx wrangler containers info "$id" --json 2>/dev/null |
  node -e 'let s="";process.stdin.on("data",(d)=>(s+=d)).on("end",()=>{const a=JSON.parse(s);process.exit(a.id===process.argv[1]&&a.name===process.argv[2]&&a.configuration?.image?.endsWith(":"+process.argv[3])?0:1)})' "$id" "$app" "$want"; do
  i=$((i + 1))
  if [ "$i" -gt 60 ]; then
    echo "Cloudflare did not finish rolling out image $want within 10 minutes. Its application and instances:" >&2
    npx wrangler containers list --json >&2 || true
    [ -z "$id" ] || npx wrangler containers info "$id" --json >&2 || true
    [ -z "$id" ] || npx wrangler containers instances "$id" --json >&2 || true
    exit 1
  fi
  [ -n "$id" ] || id="$(application_id || true)"
  sleep 10
done
echo "Cloudflare runs image $want after $((i * 10)) s."

# The Worker version the deploy made: the one Cloudflare serves to all
# requests. Read from Cloudflare, so a deploy that answered 100146 after its
# upload names it too.
version="$(npx wrangler deployments status --json 2>/dev/null |
  node -e 'let s="";process.stdin.on("data",(d)=>(s+=d)).on("end",()=>console.log(JSON.parse(s).versions?.find((v)=>v.percentage===100)?.version_id??""))' || true)"
[ -n "$version" ] || { echo "Cloudflare names no Worker version serving every request:" >&2; npx wrangler deployments status >&2 || true; exit 1; }

# Then the bot itself must report that image: /bot/health starts it if needed
# and answers with the image the bot reported since its container started.
# A deploy resets the Durable Object a while after it lands (about five
# minutes in the runs of 2026-11-06), and the reset fails the request it was
# answering with a 503, so /bot/health must also come from the Durable
# Object on the version just deployed before any snippet runs.
i=0
until curl -fsS "$site/bot/health" 2>/dev/null |
  node -e 'let s="";process.stdin.on("data",(d)=>(s+=d)).on("end",()=>{const h=JSON.parse(s);process.exit(h.running&&h.image===process.argv[1]&&h.version===process.argv[2]?0:1)})' "$want" "$version"; do
  i=$((i + 1))
  [ "$i" -le 120 ] || { echo "the bot did not report image $want on Worker version $version within 10 minutes:" >&2; curl -sS "$site/bot/health" >&2 || true; exit 1; }
  sleep 5
done
echo "The bot reports image $want on Worker version $version after $((i * 5)) s."

# Smoke test: the runnable snippets on the landing page and in the docs must still run.
# /play/run limits each IP, so the test sends x-play-admin to skip the limit:
# PLAY_SMOKE_TOKEN (the Worker's secret of that name, which GitHub Actions has
# too), or else the admin HMAC of ~/.config/rawr-play/log-key.
node --input-type=module -e '
  import { readdirSync, readFileSync } from "node:fs";
  import { join, relative } from "node:path";
  import { homedir } from "node:os";
  import { createHmac } from "node:crypto";
  const admin = process.env.PLAY_SMOKE_TOKEN || (() => {
    try {
      const key = readFileSync(join(homedir(), ".config/rawr-play/log-key"), "utf8").trim();
      return createHmac("sha256", key).update("rawr-play admin").digest("hex");
    } catch {
      return "";
    }
  })();
  const headers = { "content-type": "application/json", ...(admin ? { "x-play-admin": admin } : {}) };
  // Every page of the site that holds a runnable block, read from the files
  // (index.html is its directory'"'"'s path, other pages drop .html).
  const root = process.argv[2];
  const pages = readdirSync(root, { recursive: true })
    .filter((file) => file.endsWith(".html") && /<code[^>]*data-run/.test(readFileSync(join(root, file), "utf8")))
    .map((file) => "/" + relative(root, join(root, file)).replace(/index\.html$/, "").replace(/\.html$/, ""))
    .map((path) => (path.length > 1 ? path.replace(/\/$/, "") : path));
  const text = (h) => h.replace(/<[^>]+>/g, "").replace(/&lt;/g, "<").replace(/&gt;/g, ">").replace(/&quot;/g, "\"").replace(/&#39;/g, "\x27").replace(/&amp;/g, "&");
  const sources = new Set();
  const failing = new Set();
  for (const path of pages) {
    const page = await (await fetch(process.argv[1] + path)).text();
    const found = [...page.matchAll(/<code([^>]*data-run[^>]*)>([\s\S]*?)<\/code>/g)];
    if (!found.length) throw new Error("no runnable snippets on " + path);
    // A block whose shown output is an error (class="err") is expected to
    // fail to compile, so its 422 is its right answer.
    for (const match of found) {
      const [whole, , html] = match;
      const after = page.slice(match.index + whole.length);
      const strip = after.slice(0, after.indexOf("</div>", after.indexOf("<div class=\"run\"")));
      const body = JSON.stringify({ code: text(html) });
      if (strip.includes("class=\"err\"")) failing.add(body);
      sources.add(body);
    }
  }
  // Each distinct source once. Without the header, landing runs are rate
  // limited per IP, so a 429 waits as long as it asks and tries again.
  for (const body of sources) {
    // A fresh image can reset a connection or run out of render time on its
    // first iced snippets while it starts, so those two get a few retries.
    const start = Date.now();
    const seconds = () => ((Date.now() - start) / 1000).toFixed(1);
    for (let cold = 0; ; ) {
      let response;
      try {
        response = await fetch(process.argv[1] + "/play/run", { method: "POST", headers, body });
      } catch (error) {
        if (++cold > 3) throw error;
        console.log("the bot dropped the connection; retrying in 10 s");
        await new Promise((resolve) => setTimeout(resolve, 10000));
        continue;
      }
      if (response.status === 429) {
        const wait = Number(/(\d+) s/.exec(await response.text())?.[1] ?? 30);
        console.log("rate limited; waiting " + wait + " s");
        await new Promise((resolve) => setTimeout(resolve, (wait + 1) * 1000));
        continue;
      }
      if (response.status === 422 && failing.has(body)) {
        console.log("ran a site snippet: its compile error, as the page shows");
        break;
      }
      if (!response.ok) {
        const message = (await response.text()).slice(0, 200);
        if (response.status === 422 && message.includes("render time limit") && ++cold <= 3) {
          console.log("the bot ran out of render time; retrying in 10 s");
          await new Promise((resolve) => setTimeout(resolve, 10000));
          continue;
        }
        const first = JSON.parse(body).code.split("\n").find((line) => line.trim()) ?? "";
        throw new Error("run failed: " + response.status + " " + message + " on the snippet starting " + JSON.stringify(first));
      }
      // A limit means the snippet did not run as its page shows.
      const ran = await response.text();
      if (/^\s*\{/.test(ran) && JSON.parse(ran).limit === true) {
        const first = JSON.parse(body).code.split("\n").find((line) => line.trim()) ?? "";
        throw new Error("run failed: a limit stopped it after " + seconds() + " s on the snippet starting " + JSON.stringify(first));
      }
      console.log("ran a site snippet in " + seconds() + " s (" + (response.headers.get("x-run") ?? "?") + "):", response.headers.get("content-type"));
      break;
    }
  }
  console.log("ran " + sources.size + " distinct site snippets from " + pages.length + " pages");
' "$site" "$repo/site/dist"

# Port readiness must not erase the boot report accepted before the snippets.
curl -fsS "$site/bot/health" | node -e '
  const health = JSON.parse(require("node:fs").readFileSync(0, "utf8"));
  if (!health.running || health.image !== process.argv[1]) {
    console.error("the bot lost its running-image evidence after site snippets:", health);
    process.exit(1);
  }
  console.log("The bot still reports image " + health.image + " after site snippets.");
' "$want"
