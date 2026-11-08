// node --test test/play.test.ts
import assert from "node:assert/strict";
import { test } from "node:test";
import {
  ID_PATTERN,
  KEEP_DAYS,
  Limiter,
  MAX_CODE,
  RENDER_LIMITS,
  canonical,
  day,
  firstLine,
  checkPass,
  cookie,
  hmac,
  makePass,
  ogTags,
  PASS_COOKIE,
  PASS_SECONDS,
  passCookie,
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
import { DATA, TURNSTILE_JS, inject, slug } from "../src/playpage.ts";

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

test("the limiter allows 6 a minute and 60 an hour per key", () => {
  const limiter = new Limiter(RENDER_LIMITS);
  let now = 1_000_000;
  for (let i = 0; i < 6; i++) assert.equal(limiter.take("ip", now + i), 0);
  const wait = limiter.take("ip", now + 10);
  assert.ok(wait > 0 && wait <= 60, `waits ${wait}`);
  assert.equal(limiter.take("other", now + 10), 0, "keys are apart");
  // A minute later the minute window has room again.
  assert.equal(limiter.take("ip", now + 60_001), 0);
  // Hour: spread 60 runs over the hour, then the 61st waits.
  const hourly = new Limiter(RENDER_LIMITS);
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
  assert.ok(!(await checkPass("key", pass, now + PASS_SECONDS * 1000)), "an expired pass falls back to Turnstile");
  assert.ok(!(await checkPass("other", pass, now)), "another key's pass is refused");
  const [expires, mac] = pass.split(".");
  assert.ok(!(await checkPass("key", `${Number(expires) + 60}.${mac}`, now)), "a moved expiry is refused");
  const far = Math.floor(now / 1000) + 2 * PASS_SECONDS;
  assert.ok(!(await checkPass("key", `${far}.${await hmac("key", `pass ${far}`)}`, now)), "an expiry past 30 days is refused");
  for (const bad of [null, "", "x", `${expires}.`, `${expires}.${mac.toUpperCase()}`, `${expires}.${mac}x`]) {
    assert.ok(!(await checkPass("key", bad, now)));
  }
});

test("the pass cookie is HttpOnly, Secure and SameSite=Lax for 30 days", () => {
  const header = passCookie("123.abc");
  assert.ok(header.startsWith(`${PASS_COOKIE}=123.abc;`));
  for (const part of ["Max-Age=2592000", "Path=/play", "HttpOnly", "Secure", "SameSite=Lax"]) {
    assert.ok(header.split("; ").includes(part), part);
  }
  assert.equal(cookie(`a=1; ${PASS_COOKIE}=123.abc; b=2`, PASS_COOKIE), "123.abc");
  assert.equal(cookie(`x${PASS_COOKIE}=1`, PASS_COOKIE), null);
  assert.equal(cookie(null, PASS_COOKIE), null);
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
    sitekey: "0xKEY",
    shared: "abcDEF1234",
    og: OG,
  });
  assert.ok(!html.includes("</script><script>alert(1)") && !html.includes("</script><script>x()"), "code cannot close the script");
  const data = playData(html);
  assert.equal(data.snippet.code, "</script><script>alert(1)</script>");
  assert.equal(data.shared, "abcDEF1234");
  assert.equal(data.sitekey, "0xKEY");
  assert.equal(data.pass, false);
  assert.deepEqual(data.examples, [{ name: "Async tasks", code: "fn main() {} // </script><script>x()</script>", group: "Rust", slug: "async-tasks" }]);
  // Turnstile's script comes up front without a pass, and not with one.
  assert.ok(html.includes(`<script src="${TURNSTILE_JS}" async defer></script>`));
  const passed = inject(page, { snippet: snippet(), examples: [], sitekey: "0xKEY", pass: true, shared: null, og: OG });
  assert.ok(!passed.includes("challenges.cloudflare.com/turnstile/v0/api.js\" async"));
  assert.equal(playData(passed).pass, true);
  assert.throws(() => inject("<html></html>", { snippet: snippet(), examples: [], sitekey: null, shared: null, og: OG }), /no #play-data/);
});

test("the playground's link preview is the request's, once", async () => {
  const html = inject(await built("play.html"), { snippet: snippet(), examples: [], sitekey: null, shared: "abcDEF1234", og: OG });
  assert.equal(html.match(/property="og:title"/g)?.length, 1);
  assert.equal(html.match(/name="description"/g)?.length, 1);
  assert.ok(html.includes('<meta property="og:image" content="https://rawr.airstrike.fyi/og/card.png">'));
  assert.ok(html.includes('<meta property="og:image:height" content="630">'));
  assert.ok(html.includes('<meta name="twitter:card" content="summary_large_image">'));
});

test("the playground page is the island, with Run in browser and Run on server, and loads no rawr-web up front", async () => {
  const html = await built("play.html");
  assert.ok(html.includes("<astro-island"), "the island");
  assert.ok(html.includes(">Run on server<") && html.includes(">Run in browser<"));
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

test("the landing page's runnable sources are its files, exactly", async () => {
  const { readFileSync } = await import("node:fs");
  const { decodeHtml, runBlocks, runnable, sha256 } = await import("../src/play.ts");
  const root = new URL("../../../../", import.meta.url);
  const html = readFileSync(new URL("site/dist/index.html", root), "utf8");
  const blocks = runBlocks(html);
  const sources = runnable(html);
  assert.ok(blocks.length > 0, "the landing page has a run block");
  assert.equal(sources.length, blocks.length);
  // What the page shows (and its Run button sends) is the examples/scripts
  // file its block names, marked as a program. A library's snippet is one of
  // the server universe's programs instead (play/universe), so the server
  // runs it as that program: the iced tab is iced's counter, which
  // examples.txt names, and the ratatui tab is programs/ui.rs.
  const universe = (path: string) => readFileSync(new URL(`play/universe/${path}`, root), "utf8");
  const icedExamples = universe("examples.txt").replace(/#.*/g, "").split(/\s+/);
  for (const [i, block] of blocks.entries()) {
    assert.ok(block.name, `landing block ${i} names its file`);
    if (block.name === "main.rs") {
      assert.ok(icedExamples.includes("counter"), "the universe holds iced's counter");
      assert.match(sources[i], /^\/\/ rawr\n[\s\S]*iced::run\(Counter::update, Counter::view\)/);
      continue;
    }
    const path = block.name === "ui.rs" ? `play/universe/programs/${block.name}` : `examples/scripts/${block.name}`;
    const file = readFileSync(new URL(path, root), "utf8").trimEnd();
    assert.equal(sources[i], "// rawr\n" + file, block.name);
  }
  assert.equal(decodeHtml("&amp;self &lt;T&gt; &rsquo; &#39; &#x41;"), "&self <T> ’ ' A");
  // The allow-list is the set of their hashes; anything else is refused.
  const allowed = new Set(await Promise.all(sources.map(sha256)));
  assert.ok(allowed.has(await sha256(sources[0])));
  assert.ok(!allowed.has(await sha256(sources[0] + " ")));
  assert.equal(await sha256("abc"), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
});

// Which blocks each page holds is the docs' to change, so this checks what
// holds for any set of them rather than listing today's.
test("the allow-list covers every page's run blocks, the docs included", async () => {
  const { allowList, sitePages } = await import("../allowlist.mjs");
  const { runSnippet, sha256 } = await import("../src/play.ts");
  const pages = sitePages(new URL("../../../../site/dist", import.meta.url).pathname);
  const { hashes, blocks } = await allowList(pages);
  const all = blocks as { page: string; name: string; code: string; hash: string }[];
  const at = (page: string) => all.filter((block) => block.page === page).map((block) => block.name);
  assert.ok(at("index.html").length > 0, "the landing page has run blocks");
  assert.ok(all.some((block) => block.page.startsWith("docs/reference/")), "the reference has run blocks");
  assert.deepEqual(at("docs/apps.html"), [], "an app needs a Cargo project: ?play runs plain programs only");
  assert.deepEqual(at("docs/embedding.html"), [], "host code needs the rawr crate");
  assert.deepEqual(at("docs/imports.html"), [], "a dependency needs a Cargo project");
  // Every name is one source wherever it appears, so the docs' copy of a
  // landing block is the same entry.
  const byName = new Map<string, string>();
  for (const block of all.filter((block) => block.name !== "")) {
    const first = byName.get(block.name);
    if (first === undefined) byName.set(block.name, block.hash);
    else assert.equal(block.hash, first, `${block.page}: ${block.name}`);
  }
  // The allow-list is exactly the blocks' distinct hashes, and each block's
  // entry is the hash of what its Run button sends.
  assert.deepEqual(hashes, [...new Set(all.map((block) => block.hash))].sort());
  for (const block of all) {
    const sent = runSnippet(block);
    assert.ok(typeof sent !== "string", `${block.page}: ${block.name}: ${sent}`);
    assert.ok(hashes.includes(await sha256(canonical(sent))), `${block.page}: ${block.name}`);
  }
  assert.ok(!hashes.includes(await sha256(canonical(snippet({ code: "println!(\"hi\");" })))));
});

test("every link between the site's pages lands on a page and an anchor", async () => {
  const { existsSync, readFileSync } = await import("node:fs");
  const { sitePages } = await import("../allowlist.mjs");
  const root = new URL("../../../../site/dist/", import.meta.url).pathname;
  const pages: Map<string, string> = sitePages(root);
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

test("the allow-list build fails on empty, marked-up or disagreeing blocks", async () => {
  const { allowList } = await import("../allowlist.mjs");
  const fails = (pages: Record<string, string>, message: RegExp) =>
    assert.rejects(allowList(new Map(Object.entries(pages))), message);
  await fails({ "a.html": "<p>no snippets</p>" }, /no <code data-run> blocks/);
  await fails({ "a.html": '<code data-run="x.rs">  \n</code>' }, /a\.html \(data-run="x\.rs"\) is empty/);
  await fails({ "a.html": '<code data-run><span class="k">fn</span> main() {}</code>' }, /holds markup/);
  await fails(
    { "a.html": '<code data-run="x.rs">fn main() {}</code>', "b.html": '<code data-run="x.rs">fn main() { }</code>' },
    /b\.html \(data-run="x\.rs"\) differs from the block of the same name in a\.html/,
  );
  const ok = await allowList(new Map([["a.html", '<code data-run="x.rs">1 &lt; 2</code>'], ["b.html", '<code data-run="x.rs">1 &lt; 2</code>']]));
  assert.equal(ok.hashes.length, 1);
  assert.equal(ok.blocks[0].code, "1 < 2");
});

/**
 * Run the site's run.ts over a stand-in DOM that holds one run block, click
 * its Run button, and return the request it sends. With `edit`, the code is
 * edited first, so the request goes to /play/render. With `web`, the page
 * has rawr-web, and a run in it comes back as the url "browser"; `web` is
 * "fails" for a rawr-web that doesn't load. `where` is the block's data-where.
 */
async function runJs(
  block: { name: string; code: string },
  edit = "",
  { web, where }: { web?: "loads" | "fails"; where?: string } = {},
): Promise<{ url: string; body: any }> {
  const { setup } = await import("../../../../site/src/scripts/run.ts");
  const element = (props: Record<string, unknown> = {}): any => {
    const listeners: Record<string, ((event: unknown) => void)[]> = {};
    const attributes: Record<string, string> = {};
    return {
      dataset: {},
      classList: { add() {}, remove() {} },
      childNodes: [],
      lastChild: null,
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
  const strip = element();
  const out = element({ closest: () => strip });
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
  let sent!: (request: { url: string; body: any }) => void;
  const request = new Promise<{ url: string; body: any }>((resolve) => (sent = resolve));
  const fetch = async (url: string, init?: { body?: string }) => {
    if (url === "/play/gate") return Response.json({ sitekey: null, pass: false });
    sent({ url, body: JSON.parse(init?.body ?? "null") });
    return Response.json({ kind: "text", status: 0, output: [] });
  };
  const loader = web && (async () => {
    if (web === "fails") throw new Error("no module");
    return {
      run_text: (code: string) => {
        sent({ url: "browser", body: { code } });
        return JSON.stringify({ kind: "text", status: 0, output: [] });
      },
    };
  });
  setup({ document, window: {}, fetch, web: loader } as any);
  if (edit) {
    source.innerText = block.code + edit;
    source.fire("input");
  }
  button.fire("click");
  return request;
}

test("run.ts sends a docs block's code to /play/run, and an edit to /play/render", async () => {
  const { readFileSync } = await import("node:fs");
  const { allowList, sitePages } = await import("../allowlist.mjs");
  const { runBlocks, sha256 } = await import("../src/play.ts");
  const root = new URL("../../../../site/dist/", import.meta.url);
  const html = readFileSync(new URL("docs/play/index.html", root), "utf8");
  const sum = runBlocks(html).find((block) => block.name === "")!;
  // A block sends its code alone, and that code is on the allow-list.
  const run = await runJs(sum);
  assert.equal(run.url, "/play/run");
  assert.deepEqual(run.body, { code: sum.code });
  const { hashes } = await allowList(sitePages(root.pathname));
  assert.ok(hashes.includes(await sha256(canonical(parseSnippet(run.body) as Snippet))));
  // An edit is the visitor's own code: it goes to /play/render.
  const edited = await runJs(sum, "\n// edited");
  assert.equal(edited.url, "/play/render");
});

test("run.ts runs a block in the browser when rawr-web loads, and on the server when it doesn't or the block says so", async () => {
  const block = { name: "x.rs", code: "println!(\"x\");" };
  assert.deepEqual(await runJs(block, "", { web: "loads" }), { url: "browser", body: { code: block.code } });
  // An edit runs in the browser too, with no browser check.
  assert.equal((await runJs(block, "\n// edited", { web: "loads" })).url, "browser");
  assert.equal((await runJs(block, "", { web: "fails" })).url, "/play/run");
  assert.equal((await runJs(block, "", { web: "loads", where: "server" })).url, "/play/run");
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

test("landing runs are limited lightly per IP", async () => {
  const { LANDING_LIMITS } = await import("../src/play.ts");
  const limiter = new Limiter(LANDING_LIMITS);
  for (let i = 0; i < 20; i++) assert.equal(limiter.take("ip", 1_000 + i), 0);
  assert.ok(limiter.take("ip", 2_000) > 0);
  assert.equal(limiter.take("other", 2_000), 0);
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
});
