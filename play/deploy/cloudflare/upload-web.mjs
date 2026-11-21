// Publish a set of rawr-web's files (web/build.sh's output) to the R2 bucket
// the Worker serves /web from, then point the site at it:
//   node upload-web.mjs DIR [--commit SHA]
// The Playground workflow runs it after each build, and seed-web.sh once to
// fill an empty bucket. It uses wrangler's credentials: a login, or
// CLOUDFLARE_API_TOKEN and CLOUDFLARE_ACCOUNT_ID in CI.
//
// The set's version is the first 16 hex digits of the SHA-256 of its
// SHA256SUMS, which names every file with its hash: the same files are
// always the same version, and a version's files never change. Every file
// goes to web/<version>/<name> first. web/current.json, the pointer, goes
// last, so a page never reads a pointer to a set still uploading. The
// pointer names the version, the commit the set was built from (--commit,
// else HEAD) and its protocol: site/src/scripts/web.ts's WEB_PROTOCOL in this
// checkout, which must be the one the set was built from.
//
// A set built from an ancestor of the commit the pointer names leaves the
// pointer as it is, so builds that finish out of order never move the site
// back to older modules. A commit git doesn't know (rewritten history, or a
// seed whose commit is unknown) never holds the pointer back.
import { spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import { readFileSync, writeFileSync } from "node:fs";
import { join, resolve } from "node:path";
import { POINTER, WEB_PROTOCOL, parsePointer } from "../../../site/src/scripts/web.ts";
import { IMMUTABLE, REVALIDATE, webType } from "./src/web.ts";

const here = import.meta.dirname;
const fail = (message) => {
  console.error(`upload-web: ${message}`);
  process.exit(1);
};

const args = process.argv.slice(2);
const flag = args.indexOf("--commit");
const commitArg = flag >= 0 ? args.splice(flag, 2)[1] : undefined;
if (args.length !== 1 || (flag >= 0 && commitArg === undefined)) fail("usage: node upload-web.mjs DIR [--commit SHA]");
const dir = resolve(args[0]);
const git = (...rest) => spawnSync("git", ["-C", here, ...rest], { encoding: "utf8" });
const commit = commitArg ?? git("rev-parse", "HEAD").stdout.trim();

const bucket = /"bucket_name":\s*"([^"]+)"/.exec(readFileSync(join(here, "wrangler.jsonc"), "utf8"))?.[1];
if (!bucket) fail("wrangler.jsonc names no R2 bucket");

// The set: every file SHA256SUMS names, each checked against its hash.
let sums;
try {
  sums = readFileSync(join(dir, "SHA256SUMS"));
} catch {
  fail(`${dir} has no SHA256SUMS; build the set with web/build.sh`);
}
const files = sums
  .toString("utf8")
  .split("\n")
  .filter(Boolean)
  .map((line) => {
    const [hash, name] = line.split(/ +/);
    if (!name || name.includes("/") || name.startsWith(".")) fail(`unexpected name '${name}' in SHA256SUMS`);
    if (createHash("sha256").update(readFileSync(join(dir, name))).digest("hex") !== hash) fail(`${name} does not match SHA256SUMS`);
    return name;
  });
if (!files.includes("rawr_web.js")) fail(`${dir} is not a rawr-web set: SHA256SUMS names no rawr_web.js`);
const version = createHash("sha256").update(sums).digest("hex").slice(0, 16);

/** Run wrangler on the bucket; a failed upload is tried again. */
function wrangler(rest, tries = 1) {
  for (let attempt = 1; ; attempt++) {
    const run = spawnSync("npx", ["wrangler", ...rest, "--remote"], { cwd: here, encoding: "utf8" });
    if (run.status === 0 || attempt >= tries) return run;
    console.log(`upload-web: wrangler failed (attempt ${attempt}); trying again`);
  }
}

// The pointer now, which an older build leaves in place.
const now = wrangler(["r2", "object", "get", `${bucket}${POINTER}`, "--pipe"]);
let named = null;
if (now.status === 0) {
  try {
    named = parsePointer(JSON.parse(now.stdout));
  } catch {}
  if (!named) console.log(`upload-web: the bucket's ${POINTER.slice(1)} is not a pointer; it is replaced`);
} else if (!/The specified key does not exist/.test(now.stdout + now.stderr)) {
  fail(`could not read ${bucket}${POINTER}:\n${now.stdout}${now.stderr}`);
}
const known = (sha) => sha !== "" && git("cat-file", "-e", `${sha}^{commit}`).status === 0;
if (named && named.commit !== commit && known(named.commit) && known(commit) && git("merge-base", "--is-ancestor", commit, named.commit).status === 0) {
  console.log(`upload-web: the site runs set ${named.version}, built from ${named.commit}, newer than ${commit}; nothing is uploaded`);
  process.exit(0);
}

for (const name of [...files, "SHA256SUMS"]) {
  const put = wrangler(
    ["r2", "object", "put", `${bucket}/web/${version}/${name}`, "--file", join(dir, name), "--content-type", webType(name), "--cache-control", IMMUTABLE],
    3,
  );
  if (put.status !== 0) fail(`could not upload ${name}:\n${put.stdout}${put.stderr}`);
  console.log(`upload-web: web/${version}/${name}`);
}

const pointer = { version, commit, protocol: WEB_PROTOCOL, uploaded: new Date().toISOString() };
const file = join(dir, "current.json");
writeFileSync(file, JSON.stringify(pointer, null, 2) + "\n");
const put = wrangler(
  ["r2", "object", "put", `${bucket}${POINTER}`, "--file", file, "--content-type", webType("current.json"), "--cache-control", REVALIDATE],
  3,
);
if (put.status !== 0) fail(`could not write the pointer:\n${put.stdout}${put.stderr}`);
console.log(`upload-web: ${POINTER.slice(1)} names set ${version}, built from ${commit || "an unknown commit"}, protocol ${WEB_PROTOCOL}`);
