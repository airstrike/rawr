// Builds the site (site/, an Astro package: `npm run build` writes
// site/dist) and merges its pages with the bot's files (public/) into dist/,
// the one assets directory the Worker serves. Wrangler runs it before every
// deploy and dev (`build` in wrangler.jsonc). A path in both fails.
// It also writes src/runnable.gen.ts, the hashes of the sources /play/run
// accepts: every <code data-run> block in the built pages (allowlist.mjs).
//   node build.mjs              build the site, then merge
//   SITE_BUILT=1 node build.mjs merge the site's existing dist/
// The site needs its packages: `npm ci` in site/ once.
import { spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import { cpSync, existsSync, readdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { join, relative } from "node:path";
import { allowList, module, sitePages } from "./allowlist.mjs";

const here = import.meta.dirname;
const site = join(here, "../../../site");
const sources = [join(site, "dist"), join(here, "public")];
const dist = join(here, "dist");

if (!process.env.SITE_BUILT) {
  if (!existsSync(join(site, "node_modules"))) {
    console.error("build: the site's packages are not installed; run `npm ci` in site/");
    process.exit(1);
  }
  const built = spawnSync("npm", ["run", "build"], { cwd: site, stdio: "inherit" });
  if (built.status !== 0) {
    console.error("build: the site's build failed");
    process.exit(1);
  }
}
if (!existsSync(join(sources[0], "play.html"))) {
  console.error("build: site/dist has no play.html; build the site first");
  process.exit(1);
}

function files(dir) {
  return readdirSync(dir, { recursive: true, withFileTypes: true })
    .filter((entry) => entry.isFile())
    .map((entry) => relative(dir, join(entry.parentPath, entry.name)));
}

const seen = new Map();
for (const source of sources) {
  for (const file of files(source)) {
    if (seen.has(file)) {
      console.error(`build: ${file} is in both ${seen.get(file)} and ${source}`);
      process.exit(1);
    }
    seen.set(file, source);
  }
}

let runnable;
try {
  runnable = await allowList(sitePages(sources[0]));
} catch (error) {
  console.error(`build: ${error.message}`);
  process.exit(1);
}
writeFileSync(join(here, "src/runnable.gen.ts"), module(runnable.hashes));
console.log(`build: ${runnable.blocks.length} run blocks, ${runnable.hashes.length} distinct sources`);

rmSync(dist, { recursive: true, force: true });
for (const source of sources) cpSync(source, dist, { recursive: true });
console.log(`build: ${seen.size} files in dist/`);

// Link previews (Discord, Slack, iMessage) cache the card by its URL, so the
// pages name it with its content hash: a new card is a new URL.
const card = "og/card.png";
const version = createHash("sha256").update(readFileSync(join(dist, card))).digest("hex").slice(0, 12);
for (const file of files(dist).filter((file) => file.endsWith(".html"))) {
  const path = join(dist, file);
  const page = readFileSync(path, "utf8");
  if (page.includes(card)) writeFileSync(path, page.replaceAll(card, `${card}?v=${version}`));
}
console.log(`build: the card is ${card}?v=${version}`);
