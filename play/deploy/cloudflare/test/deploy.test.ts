import assert from "node:assert/strict";
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { spawnSync } from "node:child_process";
import { test } from "node:test";

const script = readFileSync(new URL("../deploy.sh", import.meta.url), "utf8");
const gate = script.slice(script.indexOf('# Query the application itself:'), script.indexOf('# The Worker version the deploy made'));
const current = { id: "app-id", name: "rawr-play-playbot", configuration: { image: "registry/rawr-play:new" } };

function run(details: object, listedImage = "registry/rawr-play:old", firstListFailure = false) {
  const dir = mkdtempSync(join(tmpdir(), "rawr-deploy-test-"));
  try {
    // Reproduce the real discrepancy: list and details can report different images.
    writeFileSync(join(dir, "npx"), `#!/bin/sh
case "$1 $2" in
  "wrangler containers")
    case "$3" in
      list)
        if [ "$FIRST_LIST_FAILURE" = 1 ] && [ ! -e "$SEEN" ]; then
          : > "$SEEN"
          exit 1
        fi
        printf '%s\\n' "$LISTED" ;;
      info) [ "$4" = app-id ] || exit 91; printf '%s\\n' "$DETAILS" ;;
      *) exit 92 ;;
    esac ;;
  *) exit 93 ;;
esac
`, { mode: 0o700 });
    // A rejected observation must enter the wait path; never spend real time waiting.
    writeFileSync(join(dir, "sleep"), '#!/bin/sh\n[ "$FIRST_LIST_FAILURE" = 1 ] && exit 0\nexit 23\n', { mode: 0o700 });
    return spawnSync("sh", ["-c", `set -eu\nwant=new\n${gate}`], {
      encoding: "utf8",
      timeout: 5000,
      env: {
        ...process.env,
        PATH: `${dir}:${process.env.PATH}`,
        LISTED: JSON.stringify([{ id: "app-id", name: "rawr-play-playbot", image: listedImage }]),
        DETAILS: JSON.stringify(details),
        FIRST_LIST_FAILURE: firstListFailure ? "1" : "0",
        SEEN: join(dir, "seen"),
      },
    });
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
}

test("rollout accepts fresh application details despite a stale account list", () => {
  const result = run(current);
  assert.equal(result.status, 0, result.stderr);
  assert.match(result.stdout, /Cloudflare runs image new/);
});

test("a matching image on a different application cannot complete rollout", () => {
  assert.equal(run({ ...current, id: "another-app" }).status, 23);
});

test("a fresh list cannot hide old application details", () => {
  assert.equal(run({ ...current, configuration: { image: "registry/rawr-play:old" } }, "registry/rawr-play:new").status, 23);
});

test("temporary application discovery failure retries within the rollout wait", () => {
  const result = run(current, "registry/rawr-play:old", true);
  assert.equal(result.status, 0, result.stderr);
  assert.match(result.stdout, /Cloudflare runs image new/);
});

// The wait for the bot: its image, answered by the Durable Object on the
// Worker version the deploy made (#1066).
const bot = script.slice(script.indexOf("# The Worker version the deploy made"), script.indexOf("# Smoke test"));
const status = { versions: [{ version_id: "new-version", percentage: 100 }] };

function wait(healths: object[], deployment: object = status) {
  const dir = mkdtempSync(join(tmpdir(), "rawr-deploy-test-"));
  try {
    writeFileSync(join(dir, "npx"), `#!/bin/sh
[ "$1 $2 $3" = "wrangler deployments status" ] || exit 93
printf '%s\\n' "$DEPLOYMENT"
`, { mode: 0o700 });
    // Each call answers the next health in turn, the last one from then on.
    writeFileSync(join(dir, "curl"), `#!/bin/sh
n="$(cat "$CALLS" 2>/dev/null || echo 0)"
echo $((n + 1)) > "$CALLS"
node -e 'const h=JSON.parse(process.argv[1]);console.log(JSON.stringify(h[Math.min(Number(process.argv[2]),h.length-1)]))' "$HEALTHS" "$n"
`, { mode: 0o700 });
    // Waiting costs no real time.
    writeFileSync(join(dir, "sleep"), "#!/bin/sh\nexit 0\n", { mode: 0o700 });
    const result = spawnSync("sh", ["-c", `set -eu\nwant=new\nsite=https://example.invalid\n${bot}`], {
      encoding: "utf8",
      timeout: 30000,
      env: {
        ...process.env,
        PATH: `${dir}:${process.env.PATH}`,
        DEPLOYMENT: JSON.stringify(deployment),
        HEALTHS: JSON.stringify(healths),
        CALLS: join(dir, "calls"),
      },
    });
    let calls = 0;
    try {
      calls = Number(readFileSync(join(dir, "calls"), "utf8"));
    } catch {}
    return { ...result, calls };
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
}

test("the snippets wait for the Durable Object to run the deployed Worker version", () => {
  const before = { running: true, image: "new", deployed: "new" };
  const result = wait([before, { ...before, version: "old-version" }, { ...before, version: "new-version" }]);
  assert.equal(result.status, 0, result.stderr);
  assert.equal(result.calls, 3);
  assert.match(result.stdout, /reports image new on Worker version new-version after 10 s/);
});

test("a Durable Object that never runs the deployed version fails the deploy", () => {
  const result = wait([{ running: true, image: "new", deployed: "new", version: "old-version" }]);
  assert.equal(result.status, 1);
  assert.match(result.stderr, /did not report image new on Worker version new-version within 10 minutes/);
});

test("a deployment with no version serving every request fails before the wait", () => {
  const result = wait([{}], { versions: [{ version_id: "a", percentage: 50 }, { version_id: "b", percentage: 50 }] });
  assert.equal(result.status, 1);
  assert.equal(result.calls, 0);
  assert.match(result.stderr, /names no Worker version serving every request/);
});
