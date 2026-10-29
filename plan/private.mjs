#!/usr/bin/env node
// The private tabs of /todo: renders .Codex/DECISIONS.md and
// .Codex/DECISION_CODE.md to HTML and uploads them to the rawr-play Worker's
// TODO_PRIVATE KV namespace, which serves them behind the password
// (play/deploy/cloudflare/src/todo.ts). Nothing here enters the repository or
// the site's build.
//
//   node plan/private.mjs             render and upload both documents
//   node plan/private.mjs --dry-run   render and check them, upload nothing
//
// Where .Codex/ is missing (CI, a fresh clone) it says so and exits 0, so the
// release may run it anywhere. Even behind the password the HTML is scrubbed:
// local paths become relative ones, anything shaped like a token or key is
// redacted, and the old project's name becomes "upstream". Each T number
// becomes its issue (#N, linked), through plan/items.json; a T number no
// item records stops the upload. Needs the site's packages (`npm ci` in
// site/) for its Markdown renderer, and wrangler's login.
import { execFileSync, spawnSync } from "node:child_process";
import { existsSync, mkdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { createRequire } from "node:module";
import { dirname, join } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { OLD_NAME, SECRET, withoutLocalPaths } from "./plan.mjs";

const PLAN = dirname(fileURLToPath(import.meta.url));
const ROOT = dirname(PLAN);
const WORKER = join(ROOT, "play/deploy/cloudflare");
const REPO = JSON.parse(readFileSync(join(PLAN, "project.json"), "utf8")).repo;
/** The KV key of each document, as /todo/private/<key> names it. */
const DOCS = { decisions: "DECISIONS.md", code: "DECISION_CODE.md" };

// .Codex/ lives in the main checkout, which worktrees share.
function codexDir() {
  const common = execFileSync("git", ["-C", ROOT, "rev-parse", "--path-format=absolute", "--git-common-dir"], { encoding: "utf8" }).trim();
  return { dir: join(dirname(common), ".Codex"), checkout: dirname(common) };
}

const escapeRe = (s) => s.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");

/** The old name, in the case it was written in. */
const upstream = (match) => (match === match.toUpperCase() ? "UPSTREAM" : match[0] === "R" ? "Upstream" : "upstream");

/** The Markdown with no local paths, no secrets and no old name. */
export function scrub(markdown, checkouts = []) {
  let out = markdown;
  for (const root of checkouts) out = out.replace(new RegExp(`${escapeRe(root)}/?`, "g"), "");
  out = withoutLocalPaths(out)
    .replace(new RegExp(SECRET.source, "g"), "[redacted]")
    .replace(new RegExp(`\\.\\./${OLD_NAME.source}\\b`, "gi"), "../rawr")
    .replace(new RegExp(OLD_NAME.source, "gi"), upstream);
  return out;
}

/**
 * Each T number in the HTML's text, outside code and links, as a link to its
 * issue. A T number no item records is an error.
 */
export function linkIssues(html, items, repo) {
  const byT = new Map(items.map((i) => [i.t, i.issue]));
  let skip = 0;
  return html
    .split(/(<[^>]*>)/)
    .map((piece) => {
      if (piece.startsWith("<")) {
        const tag = /^<(\/?)(a|code|pre)\b/i.exec(piece);
        if (tag) skip += tag[1] ? -1 : 1;
        return piece;
      }
      if (skip > 0) return piece;
      // "T335 (#335)" says the issue twice once T numbers render as #N.
      const once = piece.replace(/\b(T\d{3,}) \(#(\d+)\)/g, (m, t, n) => (byT.get(t) === +n ? t : m));
      return once.replace(/\bT\d{3,}\b/g, (t) => {
        const issue = byT.get(t);
        if (issue === undefined) throw new Error(`${t} is no recorded item (plan/items.json)`);
        return `<a href="https://github.com/${repo}/issues/${issue}">#${issue}</a>`;
      });
    })
    .join("");
}

/** What must never reach the Worker: local paths, the old name, secrets. */
export function leaks(html) {
  const found = [];
  if (/\/(?:Users|home)\/[^/\s]+/.test(html)) found.push("a local path");
  if (new RegExp(OLD_NAME.source, "i").test(html)) found.push("the old name");
  if (new RegExp(SECRET.source).test(html)) found.push("something shaped like a secret");
  return found;
}

async function renderer() {
  const site = createRequire(join(ROOT, "site/package.json"));
  let path;
  try {
    path = site.resolve("satteri");
  } catch {
    throw new Error("the site's packages are not installed; run `npm ci` in site/");
  }
  const { markdownToHtml } = await import(pathToFileURL(path).href);
  // Raw HTML in the Markdown stays text, so the page never runs any of it.
  const asText = { name: "raw-html-as-text", html(node, ctx) { ctx.replaceNode(node, { type: "text", value: node.value }); } };
  return (markdown) => markdownToHtml(markdown, { mdastPlugins: [asText] }).html;
}

export async function render(markdown, { items, repo = REPO, checkouts = [] }) {
  const toHtml = await renderer();
  // A link to the other document opens its tab.
  const html = linkIssues(toHtml(scrub(markdown, checkouts)), items, repo)
    .replace(/href="(?:\.\/)?DECISIONS\.md(?:#[^"]*)?"/g, 'href="#decisions"')
    .replace(/href="(?:\.\/)?DECISION_CODE\.md(?:#[^"]*)?"/g, 'href="#code"');
  const found = leaks(html);
  if (found.length) throw new Error(`the rendered HTML still holds ${found.join(", ")}`);
  return html;
}

async function main(args) {
  const dry = args.includes("--dry-run");
  const { dir, checkout } = codexDir();
  const present = Object.entries(DOCS).filter(([, file]) => existsSync(join(dir, file)));
  if (present.length === 0) {
    console.log("private: no .Codex/ documents here; nothing to upload");
    return 0;
  }
  const items = JSON.parse(readFileSync(join(PLAN, "items.json"), "utf8")).items;
  const checkouts = [...new Set([ROOT, checkout])];
  const pairs = [];
  for (const [key, file] of present) {
    const value = await render(readFileSync(join(dir, file), "utf8"), { items, checkouts });
    console.log(`private: ${key} (${file}) is ${(value.length / 1024).toFixed(0)} KiB of HTML`);
    pairs.push({ key, value });
  }
  if (dry) {
    console.log("private: dry run; nothing uploaded");
    return 0;
  }
  // wrangler reads the pairs from a file: under target/, readable by the
  // owner only, and deleted once the upload ends.
  const work = join(ROOT, "target/private");
  mkdirSync(work, { recursive: true, mode: 0o700 });
  const file = join(work, `upload-${process.pid}.json`);
  try {
    writeFileSync(file, JSON.stringify(pairs), { mode: 0o600 });
    const run = spawnSync("npx", ["wrangler", "kv", "bulk", "put", file, "--binding", "TODO_PRIVATE", "--remote"], {
      cwd: WORKER,
      encoding: "utf8",
    });
    if (run.status !== 0) {
      console.error(`${run.stdout}${run.stderr}`.trim().split("\n").slice(-8).join("\n"));
      throw new Error("wrangler could not upload the documents");
    }
  } finally {
    rmSync(file, { force: true });
  }
  console.log(`private: uploaded ${pairs.map((p) => p.key).join(" and ")} to TODO_PRIVATE`);
  return 0;
}

if (process.argv[1] && fileURLToPath(import.meta.url) === process.argv[1]) {
  main(process.argv.slice(2)).then(
    (code) => (process.exitCode = code),
    (error) => {
      console.error(`private: ${error.message}`);
      process.exitCode = 1;
    },
  );
}
