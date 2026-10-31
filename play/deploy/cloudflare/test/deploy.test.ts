import assert from "node:assert/strict";
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { spawnSync } from "node:child_process";
import { test } from "node:test";

const script = readFileSync(new URL("../deploy.sh", import.meta.url), "utf8");
const gate = script.slice(script.indexOf('# Query the application itself:'), script.indexOf('# Then the bot itself'));
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
