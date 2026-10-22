// Merges the landing page (site/public) and the bot's files (public/) into
// dist/, the one assets directory the Worker serves. Wrangler runs it before
// every deploy and dev (`build` in wrangler.jsonc). A path in both fails.
import { cpSync, readdirSync, rmSync } from "node:fs";
import { join, relative } from "node:path";

const here = import.meta.dirname;
const sources = [join(here, "../../../site/public"), join(here, "public")];
const dist = join(here, "dist");

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

rmSync(dist, { recursive: true, force: true });
for (const source of sources) cpSync(source, dist, { recursive: true });
console.log(`build: ${seen.size} files in dist/`);
