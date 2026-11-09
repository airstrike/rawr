// Cloudflare's limit on one static asset of a Worker: 25 MiB. A larger file
// fails wrangler's upload ("Asset too large") after the build and the tests,
// so build.mjs checks dist/ first and names each file over it (#1459).
import { readdirSync, statSync } from "node:fs";
import { join, relative } from "node:path";

export const ASSET_LIMIT = 25 * 1024 * 1024;

/** Each file under DIR over the limit, as [its path in DIR, its size in bytes]. */
export function oversized(dir, limit = ASSET_LIMIT) {
  return readdirSync(dir, { recursive: true, withFileTypes: true })
    .filter((entry) => entry.isFile())
    .map((entry) => join(entry.parentPath, entry.name))
    .map((path) => [relative(dir, path), statSync(path).size])
    .filter(([, size]) => size > limit)
    .sort(([a], [b]) => a.localeCompare(b));
}

/** A size in MiB, as wrangler writes it. */
export function mib(size) {
  return `${(size / 1024 / 1024).toFixed(1)} MiB`;
}
