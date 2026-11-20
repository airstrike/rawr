// node --test test/play.test.ts
import assert from "node:assert/strict";
import { test } from "node:test";
import {
  ID_PATTERN,
  KEEP_DAYS,
  Limiter,
  MAX_CODE,
  canonical,
  day,
  firstLine,
  checkPass,
  cookie,
  hmac,
  makePass,
  ogTags,
  PASS_SECONDS,
  purgeSelection,
  parseSnippet,
  renderRequest,
  same,
  shareId,
  webAsset,
  webHeaders,
  type Snippet,
} from "../src/play.ts";
import { escape } from "../src/dashboard.ts";
import { DATA, inject, slug } from "../src/playpage.ts";

const OG = {
  title: "Rawr Playground",
  description: "fn main() {}",
  url: "https://rawr.airstrike.fyi/play/abcDEF1234",
  image: "https://rawr.airstrike.fyi/og/card.png",
  alt: "The card",
  width: 1200,
  height: 630,
};

const snippet = (over: Partial<Snippet> = {}): Snippet => ({
  code: "text(\"hi\")",
  ...over,
});

test("share ids are content-addressed and URL-safe", async () => {
  const a = await shareId(canonical(snippet()));
  assert.match(a, ID_PATTERN);
  assert.equal(await shareId(canonical(snippet())), a, "the same snippet, the same id");
  assert.notEqual(await shareId(canonical(snippet({ code: "text(\"ho\")" }))), a);
  // Other fields in the request do not matter: the stored form is canonical.
  const parsed = parseSnippet({ theme: "light", code: "text(\"hi\")" });
  assert.equal(canonical(parsed as Snippet), canonical(snippet()));
});

test("snippets are validated", () => {
  assert.deepEqual(parseSnippet({ code: "x" }), { code: "x" });
  assert.equal(typeof parseSnippet(null), "string");
  assert.equal(typeof parseSnippet({ code: "   " }), "string");
  assert.equal(typeof parseSnippet({ code: "x".repeat(MAX_CODE + 1) }), "string");
  assert.equal(typeof parseSnippet({ code: "é".repeat(MAX_CODE / 2 + 1) }), "string", "the cap is in bytes");
});

/** 6 a minute and 60 an hour. */
const LIMITS = [
  { max: 6, ms: 60_000 },
  { max: 60, ms: 3_600_000 },
];

test("the limiter allows 6 a minute and 60 an hour per key", () => {
  const limiter = new Limiter(LIMITS);
  let now = 1_000_000;
  for (let i = 0; i < 6; i++) assert.equal(limiter.take("ip", now + i), 0);
  const wait = limiter.take("ip", now + 10);
  assert.ok(wait > 0 && wait <= 60, `waits ${wait}`);
  assert.equal(limiter.take("other", now + 10), 0, "keys are apart");
  // A minute later the minute window has room again.
  assert.equal(limiter.take("ip", now + 60_001), 0);
  // Hour: spread 60 runs over the hour, then the 61st waits.
  const hourly = new Limiter(LIMITS);
  now = 5_000_000;
  for (let i = 0; i < 60; i++) assert.equal(hourly.take("ip", now + i * 59_000), 0, `run ${i}`);
  assert.ok(hourly.take("ip", now + 60 * 59_000) > 0);
  assert.equal(hourly.take("ip", now + 3_600_001 + 59_000), 0);
});

test("the limiter forgets the oldest key past its bound", () => {
  const limiter = new Limiter([{ max: 1, ms: 60_000 }], 2);
  limiter.take("a", 0);
  limiter.take("b", 0);
  limiter.take("c", 0);
  assert.equal(limiter.take("a", 1), 0, "a was forgotten");
  assert.ok(limiter.take("c", 1) > 0);
});

test("hmac and constant-time compare", async () => {
  const mac = await hmac("key", "rawr-play admin");
  assert.match(mac, /^[0-9a-f]{64}$/);
  assert.equal(await hmac("key", "rawr-play admin"), mac);
  assert.notEqual(await hmac("other", "rawr-play admin"), mac);
  assert.ok(same(mac, mac));
  assert.ok(!same(mac, mac.slice(1) + "0"));
  assert.ok(!same(mac, mac.slice(1)));
});

test("a pass verifies for 30 days under its key only", async () => {
  const now = Date.UTC(2026, 9, 22);
  const pass = await makePass("key", now);
  assert.match(pass, /^\d+\.[0-9a-f]{64}$/);
  assert.ok(await checkPass("key", pass, now));
  assert.ok(await checkPass("key", pass, now + (PASS_SECONDS - 1) * 1000));
  assert.ok(!(await checkPass("key", pass, now + PASS_SECONDS * 1000)), "an expired pass is refused");
  assert.ok(!(await checkPass("other", pass, now)), "another key's pass is refused");
  const [expires, mac] = pass.split(".");
  assert.ok(!(await checkPass("key", `${Number(expires) + 60}.${mac}`, now)), "a moved expiry is refused");
  const far = Math.floor(now / 1000) + 2 * PASS_SECONDS;
  assert.ok(!(await checkPass("key", `${far}.${await hmac("key", `pass ${far}`)}`, now)), "an expiry past 30 days is refused");
  for (const bad of [null, "", "x", `${expires}.`, `${expires}.${mac.toUpperCase()}`, `${expires}.${mac}x`]) {
    assert.ok(!(await checkPass("key", bad, now)));
  }
});

test("a cookie is read by its exact name", () => {
  assert.equal(cookie("a=1; rawr_todo=123.abc; b=2", "rawr_todo"), "123.abc");
  assert.equal(cookie("xrawr_todo=1", "rawr_todo"), null);
  assert.equal(cookie(null, "rawr_todo"), null);
});

/** The site's build, which build.mjs merges into the Worker's assets. */
const DIST = new URL("../../../../site/dist/", import.meta.url);
async function built(path: string): Promise<string> {
  const { existsSync, readFileSync } = await import("node:fs");
  assert.ok(existsSync(new URL("play.html", DIST)), "site/dist is missing: run `npm run build` in site/ (build.mjs does)");
  return readFileSync(new URL(path, DIST), "utf8");
}

/** The island's data as the Worker wrote it into the page. */
const playData = (html: string) => JSON.parse(html.match(/<script type="application\/json" id="play-data">([\s\S]*?)<\/script>/)![1]);

test("the Worker writes each request's data into the site's playground page", async () => {
  const page = await built("play.html");
  assert.ok(page.includes(DATA), "the static page holds the island's placeholder");
  const html = inject(page, {
    snippet: snippet({ code: "</script><script>alert(1)</script>" }),
    examples: [{ name: "Async tasks", code: "fn main() {} // </script><script>x()</script>", group: "Rust" }],
    shared: "abcDEF1234",
    og: OG,
  });
  assert.ok(!html.includes("</script><script>alert(1)") && !html.includes("</script><script>x()"), "code cannot close the script");
  const data = playData(html);
  assert.equal(data.snippet.code, "</script><script>alert(1)</script>");
  assert.equal(data.shared, "abcDEF1234");
  assert.deepEqual(data.examples, [{ name: "Async tasks", code: "fn main() {} // </script><script>x()</script>", group: "Rust", slug: "async-tasks" }]);
  // The page runs in the browser only: it holds no browser check.
  assert.deepEqual(Object.keys(data).sort(), ["examples", "shared", "snippet"]);
  assert.ok(!html.includes("challenges.cloudflare.com/turnstile"));
  assert.throws(() => inject("<html></html>", { snippet: snippet(), examples: [], shared: null, og: OG }), /no #play-data/);
});

test("the playground's link preview is the request's, once", async () => {
  const html = inject(await built("play.html"), { snippet: snippet(), examples: [], shared: "abcDEF1234", og: OG });
  assert.equal(html.match(/property="og:title"/g)?.length, 1);
  assert.equal(html.match(/name="description"/g)?.length, 1);
  assert.ok(html.includes('<meta property="og:image" content="https://rawr.airstrike.fyi/og/card.png">'));
  assert.ok(html.includes('<meta property="og:image:height" content="630">'));
  assert.ok(html.includes('<meta name="twitter:card" content="summary_large_image">'));
});

test("the playground page is the island, with no server to run on, and loads no rawr-web up front", async () => {
  const html = await built("play.html");
  assert.ok(html.includes("<astro-island"), "the island");
  assert.ok(!html.includes("Run on server") && !html.includes("Where it runs"));
  assert.ok(!/<script[^>]*src="\/web\/rawr_web\.js"/.test(html), "rawr-web loads on demand");
  assert.ok(!html.includes('rel="modulepreload" href="/web/'));
  for (const href of ["/docs/play", "/bot", "/bot/invite"]) assert.ok(html.includes(`href="${href}"`), href);
});

test("every page has the site's header, with Docs or Playground current", async () => {
  const header = (page: string) => page.match(/<header class="bar">[\s\S]*?<\/header>/)![0];
  const landing = header(await built("index.html"));
  assert.ok(!landing.includes("aria-current"));
  assert.equal(header(await built("play.html")), landing.replace(`<a href="/play">`, `<a href="/play" aria-current="page">`));
  // A docs page also has the control that opens the docs' tree as a drawer on narrow screens.
  assert.equal(
    header(await built("docs/run.html")),
    landing.replace(
      /<a href="\/docs">Docs<\/a>\n\s*\n/,
      `<a class="docs-link" href="/docs" aria-current="true">Docs</a>\n    <button type="button" class="docs-toggle label" popovertarget="docs-nav">Docs</button>\n`,
    ),
  );
  assert.ok(landing.includes("data-theme-switch"), "the theme's switch");
});

test("link preview tags", () => {
  const tags = ogTags({ ...OG, description: 'say "hi" <b>' }, escape);
  assert.ok(tags.includes('<meta property="og:image" content="https://rawr.airstrike.fyi/og/card.png">'));
  assert.ok(tags.includes('<meta property="og:image:width" content="1200">'));
  assert.ok(tags.includes('<meta property="og:image:height" content="630">'));
  assert.ok(tags.includes('<meta name="twitter:card" content="summary_large_image">'));
  assert.ok(tags.includes('<meta property="og:image:alt" content="The card">'));
  assert.ok(tags.includes("&#34;hi&#34; &#60;b&#62;"));
  const unsized = ogTags({ ...OG, width: undefined, height: undefined }, escape);
  assert.ok(!unsized.includes("og:image:width"), "no size without one");
});

test("first line", () => {
  assert.equal(firstLine("\n\n   use std::fmt;  \nfn main() {}"), "use std::fmt;");
  assert.equal(firstLine("x".repeat(200)).length, 150);
  assert.equal(firstLine("  \n "), "");
});

test("days and the purge selection", () => {
  assert.equal(day(0), 0);
  assert.equal(day(86_400_000 - 1), 0);
  assert.equal(day(86_400_000), 1);
  const today = 20_000;
  const { remove, stamp } = purgeSelection(
    new Map<string, number | undefined>([
      ["old", today - 31],
      ["young", today - 29],
      ["edge", today - KEEP_DAYS],
      ["today", today],
      ["unstamped", undefined],
    ]),
    today,
  );
  assert.deepEqual(remove, ["old"]);
  assert.deepEqual(stamp, ["unstamped"], "untimestamped shares are stamped, not removed");
});

/** Every .html file under `dir`, as a map from its relative path to its text. */
async function sitePages(dir: string): Promise<Map<string, string>> {
  const { readdirSync, readFileSync } = await import("node:fs");
  const { join, relative } = await import("node:path");
  const pages = new Map<string, string>();
  for (const entry of readdirSync(dir, { recursive: true, withFileTypes: true })) {
    if (!entry.isFile() || !entry.name.endsWith(".html")) continue;
    const path = join(entry.parentPath, entry.name);
    pages.set(relative(dir, path), readFileSync(path, "utf8"));
  }
  return pages;
}

test("the landing page's rawr-web snippets are its files, exactly", async () => {
  const { readFileSync } = await import("node:fs");
  const { decode } = await import("../../../../site/src/lib/docs.ts");
  const root = new URL("../../../../", import.meta.url);
  const html = readFileSync(new URL("site/dist/index.html", root), "utf8");
  // A universe block (data-where="universe") is iced's counter or a program
  // of its own, so only the blocks rawr-web runs are checked here.
  const blocks = [...html.matchAll(/<code\b([^>]*\bdata-run="([^"]*)"[^>]*)>([\s\S]*?)<\/code>/g)]
    .filter(([, attributes]) => !/\bdata-where="universe"/.test(attributes))
    .map(([, , name, text]) => ({ name, code: decode(text) }));
  assert.ok(blocks.length > 0, "the landing page has a run block");
  // What the page shows (and its Run button runs) is the examples/scripts
  // file its block names, marked as a program.
  for (const [i, block] of blocks.entries()) {
    assert.ok(block.name, `landing block ${i} names its file`);
    const file = readFileSync(new URL(`examples/scripts/${block.name}`, root), "utf8").trimEnd();
    assert.equal(block.code, "// rawr\n" + file, block.name);
  }
});

test("every link between the site's pages lands on a page and an anchor", async () => {
  const { existsSync, readFileSync } = await import("node:fs");
  const root = new URL("../../../../site/dist/", import.meta.url).pathname;
  const pages = await sitePages(root);
  // The assets binding serves /a from a.html or a/index.html, and other files as they are.
  const file = (path: string) =>
    [path.slice(1), `${path.slice(1)}.html`, `${path.slice(1)}/index.html`.replace(/^\//, "")]
      .find((candidate) => candidate !== "" && existsSync(root + candidate));
  const ids = (html: string) => new Set([...html.matchAll(/ id="([^"]+)"/g)].map((m) => m[1]));
  const broken: string[] = [];
  for (const [page, html] of pages) {
    const self = "/" + page.replace(/(^|\/)index\.html$/, "").replace(/\.html$/, "");
    for (const [, href] of html.matchAll(/href="([^"]+)"/g)) {
      if (/^(https?:|mailto:)/.test(href)) continue;
      const [path, anchor] = href.split("#");
      // The Worker serves /play and /bot itself.
      if (/^\/(play|bot)(\/|$)/.test(path) && !path.startsWith("/play.svg")) continue;
      const target = path === "" ? page : path === "/" ? "index.html" : file(path.startsWith("/") ? path : `${self.replace(/[^/]*$/, "")}${path}`);
      if (!target) {
        broken.push(`${page}: ${href}`);
        continue;
      }
      if (anchor && target.endsWith(".html") && !ids(readFileSync(root + target, "utf8")).has(anchor)) {
        broken.push(`${page}: ${href} has no #${anchor}`);
      }
    }
  }
  assert.deepEqual(broken, []);
});

/**
 * Run the site's run.ts over a stand-in DOM that holds one run block, click
 * its Run button, and return the sources rawr-web ran and what the strip
 * shows. With `edit`, the code is edited first. `web` is "fails" for a
 * rawr-web that doesn't load. `where` is the block's data-where.
 */
async function runJs(
  block: { name: string; code: string },
  edit = "",
  { web = "loads", where }: { web?: "loads" | "fails"; where?: string } = {},
): Promise<{ ran: string[]; shown: string }> {
  const { setup } = await import("../../../../site/src/scripts/run.ts");
  const element = (props: Record<string, unknown> = {}): any => {
    const listeners: Record<string, ((event: unknown) => void)[]> = {};
    const attributes: Record<string, string> = {};
    return {
      dataset: {},
      classList: { add() {}, remove() {} },
      childNodes: [],
      lastChild: null,
      innerHTML: "",
      setAttribute: (key: string, value: string) => (attributes[key] = value),
      getAttribute: (key: string) => attributes[key] ?? null,
      removeAttribute: (key: string) => delete attributes[key],
      addEventListener: (type: string, listener: (event: unknown) => void) => (listeners[type] ??= []).push(listener),
      fire: (type: string) => (listeners[type] ?? []).forEach((listener) => listener({ preventDefault() {} })),
      append() {},
      before() {},
      after() {},
      remove() {},
      replaceChildren() {},
      focus() {},
      cloneNode: () => element(),
      closest: () => null,
      querySelector: () => null,
      ...props,
    };
  };
  let shown = "";
  const strip = element();
  const out = element({ closest: () => strip, replaceChildren: (child?: { innerHTML: string }) => (shown = child?.innerHTML ?? "") });
  const source = element({ textContent: block.code, innerHTML: block.code, innerText: block.code });
  source.dataset = where ? { run: block.name, where } : { run: block.name };
  const button = element();
  button.dataset = { source: "src", out: "out" };
  const document = {
    head: element(),
    createElement: () => element(),
    createElementNS: () => element(),
    createDocumentFragment: () => element(),
    querySelectorAll: (selector: string) => (selector === ".runbtn" ? [button] : []),
    getElementById: (id: string) => ({ src: source, out })[id] ?? null,
    execCommand() {},
  };
  const ran: string[] = [];
  const loader = async () => {
    if (web === "fails") throw new Error("no module");
    return {
      run_text: (code: string) => {
        ran.push(code);
        return JSON.stringify({ kind: "text", status: 0, output: [{ stream: "stdout", text: "ran\n" }] });
      },
    };
  };
  setup({ document, window: {}, web: loader } as any);
  if (edit) {
    source.innerText = block.code + edit;
    source.fire("input");
  }
  button.fire("click");
  // The run settles in microtasks, all of which run before this timer.
  await new Promise((done) => setTimeout(done, 0));
  return { ran, shown };
}

test("run.ts runs a block and an edit of it in rawr-web, and nowhere else", async () => {
  const block = { name: "x.rs", code: "println!(\"x\");" };
  assert.deepEqual(await runJs(block), { ran: [block.code], shown: "<span>ran</span>" });
  assert.deepEqual((await runJs(block, "\n// edited")).ran, [block.code + "\n// edited"]);
  const failed = await runJs(block, "", { web: "fails" });
  assert.deepEqual(failed.ran, []);
  assert.match(failed.shown, /class="err">rawr did not load in this browser: no module/);
  // A universe block on a page without the universe says so.
  const universe = await runJs(block, "", { where: "universe" });
  assert.deepEqual(universe.ran, []);
  assert.match(universe.shown, /runs on the browser's prebuilt runner, which this page doesn't load/);
});

test("text results are read back, and give a preview line", async () => {
  const { firstOutputLine, parseTextResult } = await import("../src/play.ts");
  const json = '{"kind":"text","status":1,"output":[{"stream":"stdout","text":"\\ntotal: 27\\n"},{"stream":"stderr","text":"Error: \\"x\\"\\n"}],"truncated":false,"limit":false}';
  const result = parseTextResult(json);
  assert.ok(result);
  assert.equal(result.status, 1);
  assert.equal(result.output.length, 2);
  assert.equal(firstOutputLine(result), "total: 27");
  assert.equal(result.note, undefined);
  const noted = parseTextResult('{"kind":"text","status":0,"output":[],"note":"messages are ignored for programs without iced"}');
  assert.equal(noted?.note, "messages are ignored for programs without iced");
  assert.equal(parseTextResult("\x89PNG"), null);
  assert.equal(parseTextResult('{"kind":"image"}'), null);
});

test("a render request posts the snippet to the bot", async () => {
  const request = renderRequest('{"code":"x"}');
  assert.equal(request.method, "POST");
  assert.equal(request.url, "http://container/render");
  assert.equal(request.headers.get("content-type"), "application/json");
  assert.equal(await request.text(), '{"code":"x"}');
});

test("example slugs are stable, readable and unique", async () => {
  assert.equal(slug("Async tasks"), "async-tasks");
  assert.equal(slug("Counter"), "counter");
  assert.equal(slug("Enums, match and ?"), "enums-match-and");
  assert.equal(slug("Use after move (error)"), "use-after-move-error");
  // examples.ts imports .rs files, which only the Worker's build reads: its names come from its source.
  const source = (await import("node:fs")).readFileSync(new URL("../src/examples.ts", import.meta.url), "utf8");
  const slugs = [...source.matchAll(/name: "([^"]+)"/g)].map((m) => slug(m[1]));
  assert.ok(slugs.length >= 8);
  assert.ok(slugs.every((s) => /^[a-z0-9]+(-[a-z0-9]+)*$/.test(s)));
  assert.equal(new Set(slugs).size, slugs.length);
});

test("rawr-web's files are served with types the edge compresses", () => {
  const headers = (path: string) => webHeaders(path, new Headers({ etag: "\"x\"", "content-type": "application/octet-stream" }));
  assert.equal(headers("/web/rawr_web_bg.wasm")?.get("content-type"), "application/wasm");
  assert.equal(headers("/web/rawr_web_bg.wasm")?.get("etag"), "\"x\"");
  assert.equal(headers("/web/rawr_web_bg.wasm")?.get("cache-control"), null, "the module revalidates");
  assert.equal(headers("/web/rawr_web.js")?.get("content-type"), "text/javascript; charset=utf-8");
  assert.equal(headers("/web/index.html"), null);
});

test("a browser that takes brotli gets rawr-web's brotli copy as is", async () => {
  const stored: Record<string, string> = { "/web/rawr_web_bg.wasm": "wasm", "/web/rawr_web_bg.wasm.br": "packed", "/web/rawr_web.js": "js" };
  const assets = {
    fetch: async (request: Request) => {
      const body = stored[new URL(request.url).pathname];
      return body ? new Response(body, { headers: { etag: `"${body}"` } }) : new Response("no", { status: 404 });
    },
  };
  const get = (path: string, encoding: string) =>
    webAsset(new Request("https://rawr.airstrike.fyi" + path, { headers: { "accept-encoding": encoding } }), assets);
  const packed = await get("/web/rawr_web_bg.wasm", "gzip, deflate, br, zstd");
  assert.equal(await packed.text(), "packed");
  assert.equal(packed.headers.get("content-encoding"), "br");
  assert.equal(packed.headers.get("content-type"), "application/wasm");
  const plain = await get("/web/rawr_web_bg.wasm", "gzip");
  assert.equal(await plain.text(), "wasm");
  assert.equal(plain.headers.get("content-encoding"), null);
  assert.equal(plain.headers.get("content-type"), "application/wasm");
  // No brotli copy: the stored file, which the edge compresses itself.
  const js = await get("/web/rawr_web.js", "br");
  assert.equal(await js.text(), "js");
  assert.equal(js.headers.get("content-type"), "text/javascript; charset=utf-8");
  assert.equal((await get("/web/missing.wasm", "br")).status, 404);
  assert.equal((await get("/web/missing.wasm", "gzip")).status, 404);
});

test("a module stored only as its brotli copy goes out as that copy to every client", async () => {
  // The universe's module (#1459): raw, it is over Cloudflare's limit on an asset.
  const stored: Record<string, string> = { "/web/universe_bg.wasm.br": "packed" };
  const assets = {
    fetch: async (request: Request) => {
      const body = stored[new URL(request.url).pathname];
      return body ? new Response(body, { headers: { etag: `"${body}"` } }) : new Response("no", { status: 404 });
    },
  };
  for (const encoding of ["gzip, deflate, br, zstd", "gzip", ""]) {
    const served = await webAsset(new Request("https://rawr.airstrike.fyi/web/universe_bg.wasm", { headers: { "accept-encoding": encoding } }), assets);
    assert.equal(served.status, 200);
    assert.equal(await served.text(), "packed");
    assert.equal(served.headers.get("content-encoding"), "br");
    assert.equal(served.headers.get("content-type"), "application/wasm");
    assert.equal(served.headers.get("etag"), "\"packed\"");
  }
  // live-web.sh fetches the copy itself by its name, as stored.
  const copy = await webAsset(new Request("https://rawr.airstrike.fyi/web/universe_bg.wasm.br"), assets);
  assert.equal(await copy.text(), "packed");
  assert.equal(copy.headers.get("content-encoding"), null);
});

test("the build names each asset over Cloudflare's limit", async () => {
  const { mkdtempSync, mkdirSync, rmSync, writeFileSync } = await import("node:fs");
  const { tmpdir } = await import("node:os");
  const { join } = await import("node:path");
  const { ASSET_LIMIT, oversized } = await import("../limits.mjs");
  assert.equal(ASSET_LIMIT, 25 * 1024 * 1024);
  const dir = mkdtempSync(join(tmpdir(), "rawr-limits-test-"));
  try {
    mkdirSync(join(dir, "web"));
    writeFileSync(join(dir, "web/universe_bg.wasm"), Buffer.alloc(11));
    writeFileSync(join(dir, "web/universe_bg.wasm.br"), Buffer.alloc(10));
    writeFileSync(join(dir, "index.html"), Buffer.alloc(3));
    assert.deepEqual(oversized(dir, 10), [["web/universe_bg.wasm", 11]]);
    assert.deepEqual(oversized(dir), []);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});
