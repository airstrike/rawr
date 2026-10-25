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
  pngSize,
  stillBody,
  previewHeaders,
  purgeSelection,
  parseSnippet,
  renderRequest,
  USER_HEADER,
  same,
  shareId,
  webAsset,
  webHeaders,
  type Snippet,
} from "../src/play.ts";
import { escape } from "../src/dashboard.ts";
import { render, slug, WEB_MODULE, type Example } from "../src/playpage.ts";

const OG = {
  title: "Rawr Playground",
  description: "use iced::widget::{button, column};",
  url: "https://rawr.airstrike.fyi/play/abcDEF1234",
  image: "https://rawr.airstrike.fyi/play/abcDEF1234.png",
  alt: "The last frame",
  width: 328,
  height: 382,
};

const snippet = (over: Partial<Snippet> = {}): Snippet => ({
  code: "text(\"hi\")",
  theme: "dark",
  size: "",
  messages: [],
  ...over,
});

test("share ids are content-addressed and URL-safe", async () => {
  const a = await shareId(canonical(snippet()));
  assert.match(a, ID_PATTERN);
  assert.equal(await shareId(canonical(snippet())), a, "the same snippet, the same id");
  assert.notEqual(await shareId(canonical(snippet({ theme: "light" }))), a, "options are part of it");
  assert.notEqual(await shareId(canonical(snippet({ code: "text(\"ho\")" }))), a);
  // Key order in the request does not matter: the stored form is canonical.
  const parsed = parseSnippet({ messages: [], size: "", theme: "dark", code: "text(\"hi\")" });
  assert.equal(canonical(parsed as Snippet), canonical(snippet()));
});

test("snippets are validated and normalized", () => {
  assert.deepEqual(parseSnippet({ code: "x", theme: "light", size: " 400X300 ", messages: ["A", " ", " B "] }), {
    code: "x",
    theme: "light",
    size: "400x300",
    messages: ["A", "B"],
  });
  assert.equal((parseSnippet({ code: "x", theme: "pink" }) as Snippet).theme, "dark");
  assert.equal(typeof parseSnippet(null), "string");
  assert.equal(typeof parseSnippet({ code: "   " }), "string");
  assert.equal(typeof parseSnippet({ code: "x".repeat(MAX_CODE + 1) }), "string");
  assert.equal(typeof parseSnippet({ code: "é".repeat(MAX_CODE / 2 + 1) }), "string", "the cap is in bytes");
  assert.equal(typeof parseSnippet({ code: "x", size: "big" }), "string");
  assert.equal(typeof parseSnippet({ code: "x", messages: "A" }), "string");
  assert.equal(typeof parseSnippet({ code: "x", messages: Array(33).fill("A") }), "string");
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

test("a page with a pass loads no widget until the server refuses the pass", () => {
  const html = render({ snippet: snippet(), examples: [], sitekey: "0xKEY", pass: true, shared: null, og: OG });
  assert.ok(!html.includes("<script src=\"https://challenges.cloudflare.com"), "no Turnstile script up front");
  assert.ok(html.includes(`id="ts"`), "the widget can still mount as a fallback");
  assert.ok(html.includes("let pass = true;"));
  const scripts = [...html.matchAll(/<script>([\s\S]*?)<\/script>/g)].map((m) => m[1]);
  assert.doesNotThrow(() => new Function(scripts[0]));
  assert.ok(scripts[0].includes('appearance: "interaction-only"'));
  const fresh = render({ snippet: snippet(), examples: [], sitekey: "0xKEY", shared: null, og: OG });
  assert.ok(fresh.includes("let pass = false;"));
});

test("the page escapes the code and embeds the examples safely", () => {
  const html = render({
    snippet: snippet({ code: "</textarea><script>alert(1)</script>", messages: ["Message::A"] }),
    examples: [{ name: "Counter", code: "fn main() {} // </script><script>x()</script>" }],
    sitekey: "0xKEY",
    shared: "abcDEF1234",
    og: OG,
  });
  assert.ok(!html.includes("</textarea><script>alert(1)"));
  assert.ok(html.includes("&#60;/textarea&#62;"));
  assert.ok(!html.includes("</script><script>x()"), "example code cannot close the script");
  assert.ok(html.includes("Message::A"));
  assert.ok(html.includes("turnstile/v0/api.js"));
  assert.ok(html.includes("shared · abcDEF1234"));
  assert.ok(html.includes('href="/bot/invite"') && html.includes('href="/bot"'));
  const bare = render({ snippet: snippet(), examples: [], sitekey: null, shared: null, og: OG });
  assert.ok(!bare.includes("challenges.cloudflare.com") && !bare.includes(`id="ts"`));
});

test("the page's inline script parses", () => {
  const html = render({
    snippet: snippet({ messages: ["Message::Increment", "Message::Increment"] }),
    examples: [{ name: "Counter", code: "x", messages: ["Message::Increment"] }],
    sitekey: "0xKEY",
    shared: null,
    og: OG,
  });
  const scripts = [...html.matchAll(/<script>([\s\S]*?)<\/script>/g)].map((m) => m[1]);
  assert.equal(scripts.length, 1);
  assert.doesNotThrow(() => new Function(scripts[0]));
  assert.ok(scripts[0].includes("localStorage"), "the draft is kept");
  assert.ok(html.includes('<option value="reset">Reset</option>'));
});

test("the examples menu groups Rust before iced, each option by its index", () => {
  const html = render({
    snippet: snippet(),
    examples: [
      { name: "Letter histogram", code: "fn main() {}", group: "Rust" },
      { name: "Word count", code: "fn main() {}", group: "Rust" },
      { name: "Counter", code: "x", messages: ["Message::Increment"], group: "iced" },
    ],
    sitekey: null,
    shared: null,
    og: OG,
  });
  assert.ok(
    html.includes(
      '<option value="">Examples</option><optgroup label="Rust"><option value="0">Letter histogram</option>' +
        '<option value="1">Word count</option></optgroup><optgroup label="iced"><option value="2">Counter</option>' +
        '</optgroup><option value="reset">Reset</option>',
    ),
  );
});

test("link preview tags", () => {
  const tags = ogTags({ ...OG, description: 'say "hi" <b>' }, escape);
  assert.ok(tags.includes('<meta property="og:image" content="https://rawr.airstrike.fyi/play/abcDEF1234.png">'));
  assert.ok(tags.includes('<meta property="og:image:width" content="328">'));
  assert.ok(tags.includes('<meta property="og:image:height" content="382">'));
  assert.ok(tags.includes('<meta name="twitter:card" content="summary_large_image">'));
  assert.ok(tags.includes('<meta property="og:image:alt" content="The last frame">'));
  assert.ok(tags.includes("&#34;hi&#34; &#60;b&#62;"));
  const unsized = ogTags({ ...OG, width: undefined, height: undefined }, escape);
  assert.ok(!unsized.includes("og:image:width"), "no size until the preview is rendered");
  const page = render({ snippet: snippet(), examples: [], sitekey: null, shared: "abcDEF1234", og: OG });
  assert.ok(page.includes("og:image:height"));
});

test("first line, headers, PNG size", () => {
  assert.equal(firstLine("\n\n   use iced::Element;  \nfn main() {}"), "use iced::Element;");
  assert.equal(firstLine("x".repeat(200)).length, 150);
  assert.equal(firstLine("  \n "), "");
  assert.equal(previewHeaders(true)["cache-control"], "public, max-age=31536000, immutable");
  assert.equal(previewHeaders(true)["content-type"], "image/png");
  assert.equal(previewHeaders(false)["cache-control"], "public, max-age=300");
  const header = new Uint8Array(24);
  header.set([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]);
  new DataView(header.buffer).setUint32(16, 328);
  new DataView(header.buffer).setUint32(20, 382);
  assert.deepEqual(pngSize(header), { width: 328, height: 382 });
  assert.equal(pngSize(new Uint8Array(24)), null);
  assert.equal(pngSize(header.slice(0, 20)), null);
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
  const html = readFileSync(new URL("site/public/index.html", root), "utf8");
  const sources = runnable(html);
  assert.equal(sources.length, 2, "cart.rs and the counter");
  assert.deepEqual(runBlocks(html).map((block) => block.name), ["cart.rs", "main.rs"]);
  // What the page shows (and its Run button sends) is examples/scripts/cart.rs, marked as a program.
  assert.equal(sources[0], "// rawr\n" + readFileSync(new URL("examples/scripts/cart.rs", root), "utf8").trimEnd());
  assert.match(sources[1], /^\/\/ rawr\nuse iced::Center;\n/);
  assert.ok(sources[1].includes("fn view(&self) -> impl Widget<Message> {"));
  assert.equal(decodeHtml("&amp;self &lt;T&gt; &rsquo; &#39; &#x41;"), "&self <T> ’ ' A");
  // The allow-list is the set of their hashes; anything else is refused.
  const allowed = new Set(await Promise.all(sources.map(sha256)));
  assert.ok(allowed.has(await sha256(sources[0])));
  assert.ok(!allowed.has(await sha256(sources[0] + " ")));
  assert.equal(await sha256("abc"), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
});

test("the allow-list covers every page's run blocks, the docs included", async () => {
  const { allowList, sitePages } = await import("../allowlist.mjs");
  const { sha256 } = await import("../src/play.ts");
  const pages = sitePages(new URL("../../../../site/public", import.meta.url).pathname);
  const { hashes, blocks } = await allowList(pages);
  const at = (page: string) => blocks.filter((block: { page: string }) => block.page === page).map((block: { name: string }) => block.name);
  assert.deepEqual(at("index.html"), ["cart.rs", "main.rs"]);
  assert.deepEqual(at("docs/index.html"), []);
  assert.deepEqual(at("docs/start.html"), ["hello.rs", "cart.rs"]);
  assert.deepEqual(at("docs/reference/overview.html"), ["parse.rs"]);
  assert.deepEqual(at("docs/reference/items.html"), [
    "nested-fn.rs", "structs.rs", "enums.rs", "impl.rs", "traits.rs", "unsafe-impl.rs", "consts.rs", "aliases.rs", "geometry.rs", "derive.rs",
  ]);
  assert.deepEqual(at("docs/reference/types.html"), [
    "integers.rs", "sqrt.rs", "text.rs", "tuples.rs", "option.rs", "rc.rs", "impl-iter.rs", "fn-pointer.rs", "casts.rs",
  ]);
  assert.deepEqual(at("docs/reference/generics.html"), ["generics.rs", "shapes.rs", "countdown.rs", "bag.rs"]);
  assert.deepEqual(at("docs/reference/patterns.html"), [
    "match.rs", "destructure.rs", "let-else.rs", "irrefutable.rs", "labels.rs", "block-value.rs", "iter-mut.rs",
  ]);
  assert.deepEqual(at("docs/reference/closures.html"), ["closures.rs", "capture.rs", "boxed-fn.rs", "copy-closure.rs", "struct-update.rs"]);
  assert.deepEqual(at("docs/reference/moves.html"), ["partial-move.rs", "clone-out.rs", "rc-refcell.rs"]);
  assert.deepEqual(at("docs/reference/macros.html"), ["macros.rs", "format.rs"]);
  assert.deepEqual(at("docs/reference/std.html"), [
    "strings.rs", "find-retain.rs", "iterators.rs", "array-map.rs", "floats.rs", "time.rs",
    "hashmap.rs", "refcell.rs", "mem.rs", "ordering.rs", "conversions.rs", "path.rs",
  ]);
  assert.deepEqual(at("docs/reference/iced.html"), ["sun.rs", "widget.rs"]);
  assert.deepEqual(at("docs/iced/index.html"), ["counter.rs"]);
  assert.deepEqual(at("docs/play/index.html"), ["expression.rs", "view.rs", "increment.rs", ""]);
  assert.deepEqual(at("docs/reference/play.html"), ["fill.rs", "dots.rs", "circle.rs", "channel.rs"]);
  assert.deepEqual(at("docs/embedding.html"), [], "host code needs the rawr crate");
  assert.deepEqual(at("docs/imports.html"), [], "a dependency needs a Cargo project");
  // Every name is one source: the docs' cart.rs is the landing page's block.
  const names = blocks.filter((block: { name: string }) => block.name !== "").map((block: { name: string }) => block.name);
  assert.equal(new Set(names).size, names.length - 1);
  // The docs' cart.rs and the iced page's counter.rs are the landing page's
  // cart.rs and main.rs, so each pair is one entry.
  assert.equal(hashes.length, new Set(blocks.map((block: { hash: string }) => block.hash)).size);
  assert.equal(hashes.length, blocks.length - 2);
  assert.ok(hashes.includes(await sha256(canonical(snippet({ code: "let xs = vec![3, 1, 2];\nxs.iter().sum::<i64>()" })))));
  assert.ok(!hashes.includes(await sha256(canonical(snippet({ code: "println!(\"hi\");" })))));
});

test("every link between the site's pages lands on a page and an anchor", async () => {
  const { existsSync, readFileSync } = await import("node:fs");
  const { sitePages } = await import("../allowlist.mjs");
  const root = new URL("../../../../site/public/", import.meta.url).pathname;
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

test("docs pages that moved redirect to their new paths", async () => {
  const { existsSync, readFileSync } = await import("node:fs");
  const root = new URL("../../../../site/public/", import.meta.url).pathname;
  const source = readFileSync(new URL("../src/index.ts", import.meta.url), "utf8");
  const block = source.match(/export const DOCS_MOVED[^{]*\{([^}]*)\}/);
  assert.ok(block);
  const moved = Object.fromEntries([...block[1].matchAll(/"([^"]+)": "([^"]+)"/g)].map((m) => [m[1], m[2]]));
  assert.deepEqual(Object.keys(moved).sort(), [
    "/docs/cli",
    "/docs/cli.html",
    "/docs/faq",
    "/docs/faq.html",
    "/docs/iced.html",
    "/docs/iced/reference",
    "/docs/iced/reference.html",
    "/docs/language",
    "/docs/language.html",
    "/docs/play.html",
    "/docs/play/reference",
    "/docs/play/reference.html",
    "/docs/reference",
    "/docs/requirements",
    "/docs/requirements.html",
  ]);
  for (const [from, to] of Object.entries(moved)) {
    assert.ok(!existsSync(root + from.slice(1) + (from.endsWith(".html") ? "" : ".html")), `${from} is still a page`);
    assert.ok(existsSync(root + to.slice(1) + ".html") || existsSync(root + to.slice(1) + "/index.html"), `${to} is not a page`);
  }
});

test("docs.js sends a moved anchor to a page and an id that exist", async () => {
  const { existsSync, readFileSync } = await import("node:fs");
  const root = new URL("../../../../site/public/", import.meta.url).pathname;
  const script = readFileSync(root + "docs/docs.js", "utf8");
  const moved: Record<string, Record<string, string>> = JSON.parse(script.match(/const MOVED = (\{.*\});/)![1]);
  const page = (path: string) =>
    [`${path.slice(1)}.html`, `${path.slice(1)}/index.html`].find((candidate) => existsSync(root + candidate));
  const broken: string[] = [];
  for (const [from, anchors] of Object.entries(moved)) {
    if (!page(from)) broken.push(`${from} is not a page`);
    for (const [id, to] of Object.entries(anchors)) {
      const [path, anchor] = to.split("#");
      const file = page(path);
      if (!file) broken.push(`${from}#${id}: ${path} is not a page`);
      else if (anchor && !readFileSync(root + file, "utf8").includes(` id="${anchor}"`)) broken.push(`${from}#${id}: ${to} has no such id`);
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
  // A block's data-play and the ?play header the page shows above it agree.
  const app = (header: string, play: string | null) =>
    `<div class="file"><b>${header}</b></div>\n  <pre><code data-run="app.rs"${play === null ? "" : ` data-play="${play}"`}>x</code></pre>`;
  await fails({ "a.html": app("?play light", null) }, /shows \?play light but has no data-play/);
  await fails({ "a.html": app("app.rs", "light") }, /has data-play but no \?play header above it/);
  await fails({ "a.html": app("?play light msg=A", "light msg=B") }, /has data-play="light msg=B" but its header shows \?play light msg=A/);
  await fails({ "a.html": app("?play pink", "pink") }, /unknown \?play option `pink`/);
  await fails({ "a.html": app("?play", "") + app("?play light", "light") }, /differs from the block of the same name/);
  const agreed = await allowList(new Map([["a.html", app("?play light msg=A", "light  msg=A")]]));
  assert.deepEqual(agreed.blocks[0].snippet, { code: "x", theme: "light", size: "", messages: ["A"] });
});

test("a ?play line's options read as the bot reads them", async () => {
  const { playLine } = await import("../src/play.ts");
  assert.deepEqual(playLine("320x200 light msg=Message::Increment msg=Message::Increment"), {
    size: "320x200",
    theme: "light",
    messages: ["Message::Increment", "Message::Increment"],
  });
  assert.deepEqual(playLine("size=40X30 theme=dark message=Go"), { size: "40X30", theme: "dark", messages: ["Go"] });
  assert.deepEqual(playLine(""), { messages: [] });
  assert.equal(typeof playLine("mine"), "string");
  assert.equal(typeof playLine("theme=pink"), "string");
  assert.deepEqual(parseSnippet({ code: "x", play: "light 40X30 msg=A", messages: ["B"] }), {
    code: "x",
    theme: "light",
    size: "40x30",
    messages: ["B", "A"],
  });
  assert.equal(typeof parseSnippet({ code: "x", play: "size=big" }), "string");
  assert.equal(typeof parseSnippet({ code: "x", play: 3 }), "string");
});

/**
 * Load the site's run.js over a stand-in DOM that holds one run block, click
 * its Run button, and return the request it sends. With `edit`, the code is
 * edited first, so the request goes to /play/render.
 */
async function runJs(block: { name: string; code: string; play: string | null }, edit = ""): Promise<{ url: string; body: any }> {
  const { readFileSync } = await import("node:fs");
  const script = readFileSync(new URL("../../../../site/public/run.js", import.meta.url), "utf8");
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
  source.dataset = { run: block.name, ...(block.play === null ? {} : { play: block.play }) };
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
  new Function("document", "window", "fetch", script)(document, {}, fetch);
  if (edit) {
    source.innerText = block.code + edit;
    source.fire("input");
  }
  button.fire("click");
  return request;
}

test("run.js sends a block's ?play options, and /play/run allows that exact snippet", async () => {
  const { readFileSync } = await import("node:fs");
  const { allowList, sitePages } = await import("../allowlist.mjs");
  const { runBlocks, sha256 } = await import("../src/play.ts");
  const root = new URL("../../../../site/public/", import.meta.url);
  const html = readFileSync(new URL("docs/play/index.html", root), "utf8");
  const app = runBlocks(html).find((block) => block.name === "increment.rs")!;
  // The page shows the bot's ?play line, and the block sends the same options.
  assert.equal(app.header, "light msg=Message::Increment msg=Message::Increment");
  assert.equal(app.play, app.header);

  // Run as shown: /play/run gets the code and the options, which make two
  // messages in the light theme, and that pair is on the allow-list.
  const run = await runJs(app);
  assert.equal(run.url, "/play/run");
  assert.deepEqual(run.body, { code: app.code, play: app.play });
  const sent = parseSnippet(run.body) as Snippet;
  assert.deepEqual(sent.messages, ["Message::Increment", "Message::Increment"]);
  assert.equal(sent.theme, "light");
  const { hashes } = await allowList(sitePages(root.pathname));
  assert.ok(hashes.includes(await sha256(canonical(sent))));
  // The code without its options, which drew the counter at 0, isn't allowed.
  assert.ok(!hashes.includes(await sha256(canonical(parseSnippet({ code: app.code }) as Snippet))));

  // An edit goes to /play/render with the same options.
  const edited = await runJs(app, "\n// edited");
  assert.equal(edited.url, "/play/render");
  assert.equal(edited.body.play, app.play);
  assert.deepEqual((parseSnippet(edited.body) as Snippet).messages, ["Message::Increment", "Message::Increment"]);

  // A block with no options sends its code alone, as before.
  const sum = runBlocks(html).find((block) => block.name === "")!;
  assert.deepEqual((await runJs(sum)).body, { code: sum.code });
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

test("the playground shows text results as a terminal", () => {
  const page = render({ snippet: snippet(), examples: [], sitekey: null, shared: null, og: OG });
  assert.ok(page.includes('const terminal = (result)'));
  assert.ok(page.includes('type.startsWith("application/json")'));
  assert.ok(page.includes("ignored for programs without iced"));
});

test("a share preview asks for a still image", () => {
  const body = JSON.parse(stillBody(JSON.stringify({ code: "x", messages: ["Message::A"] })));
  assert.deepEqual(body, { code: "x", messages: ["Message::A"], still: true });
});

test("a render request names its caller for the animation allowance", async () => {
  const request = renderRequest('{"code":"x"}', "abc123");
  assert.equal(request.method, "POST");
  assert.equal(request.url, "http://container/render");
  assert.equal(request.headers.get(USER_HEADER), "abc123");
  assert.equal(request.headers.get("content-type"), "application/json");
  assert.equal(await request.text(), '{"code":"x"}');
});

test("the page offers Run in browser, the default, and Run on server; rawr-web loads once the page is up", () => {
  const html = render({ snippet: snippet(), examples: [], sitekey: null, shared: null, og: OG });
  assert.match(html, /<input type="radio" name="where" value="server"><span>Run on server<\/span>/);
  assert.match(html, /<input type="radio" name="where" value="browser" checked><span>Run in browser<\/span>/);
  // Nothing loads the module in the HTML: the inline script imports it after the page loads.
  assert.ok(!html.includes(`<script type="module"`));
  assert.ok(!html.includes(`src="${WEB_MODULE}"`));
  const script = [...html.matchAll(/<script>([\s\S]*?)<\/script>/g)].map((m) => m[1])[0];
  assert.ok(script.includes(`import(${JSON.stringify(WEB_MODULE)})`));
  assert.ok(script.includes(`addEventListener("load", preload`));
  // The server runs what the browser cannot.
  for (const reason of ["rawr-web did not load", "this browser has no WebGL2 or WebGPU", "rawr-web cannot run this yet"]) {
    assert.ok(script.includes(reason), reason);
  }
  assert.ok(!script.includes("tokio's timers"), "an async main runs in the browser");
});

test("the playground's header is the site's, with Playground current, and its own links in the footer", async () => {
  const html = render({ snippet: snippet(), examples: [], sitekey: null, shared: null, og: OG });
  const site = (await import("node:fs")).readFileSync(new URL("../../../../site/public/index.html", import.meta.url), "utf8");
  const header = (page: string) => page.match(/<header class="bar">[\s\S]*?<\/header>/)![0];
  assert.equal(header(html), header(site).replace(`<a href="/play">`, `<a href="/play" aria-current="page">`));
  const footer = html.match(/<footer>[\s\S]*?<\/footer>/)![0];
  for (const href of ["/docs/play", "/bot", "/bot/invite"]) assert.ok(footer.includes(`href="${href}"`), href);
});

/** The page's inline script against a stand-in DOM, with `web` as the loaded rawr-web. */
function standIn(options: { code?: string; web?: Record<string, unknown> | null; gpu?: boolean; url?: string; shared?: string | null; examples?: Example[] } = {}) {
  const { code = "", web = null, gpu = true, url = "http://localhost/play", shared = null, examples = [] } = options;
  const html = render({ snippet: snippet({ code }), examples, sitekey: null, shared, og: OG });
  const script = [...html.matchAll(/<script>([\s\S]*?)<\/script>/g)].map((m) => m[1])[0];
  const data = html.match(/<script type="application\/json" id="example-data">([\s\S]*?)<\/script>/)![1];
  const nodes: Record<string, any> = {};
  const listen = () => ({ on: {} as Record<string, Function[]>, addEventListener(type: string, f: Function) { (this.on[type] ??= []).push(f); } });
  const node = (id: string, value = "") => (nodes[id] ??= {
    id, value, textContent: id === "msghint" ? "(sent before preview)" : "", children: [] as any[], style: {}, dataset: {},
    classList: { toggle() {}, add() {} }, ...listen(), append(...c: any[]) { this.children.push(...c); },
    replaceChildren(...c: any[]) { this.children = c; },
  });
  node("code", code); node("messages"); node("size");
  node("example-data").textContent = data;
  // Radios in one group: checking one unchecks the other.
  const where: Record<"server" | "browser", any> = {} as any;
  for (const value of ["server", "browser"] as const) {
    let on = value === "browser";
    where[value] = { value, ...listen(), get checked() { return on; },
      set checked(next: boolean) { on = next; if (next) where[value === "server" ? "browser" : "server"].checked = false; } };
  }
  const fetched: string[] = [];
  const document = {
    readyState: "loading",
    getElementById: node,
    createElement: (tag: string) => tag === "canvas" ? { getContext: () => (gpu ? {} : null) } : { ...node("new-" + Math.random()), tag },
    querySelectorAll: (q: string) => q.includes("where") ? Object.values(where) : [],
    querySelector: (q: string) => q === "input[name=where]:checked" ? (where.browser.checked ? where.browser : where.server)
      : q === "input[name=where][value=server]" ? where.server : { value: "dark" },
    addEventListener() {}, head: { append() {} },
  };
  const loads: string[] = [];
  const load = async (url: string) => { loads.push(url); if (!web) throw new Error("404"); return { default: async () => {}, ...web }; };
  let fetch: Function = async (url: string) => { fetched.push(url); return { ok: true, status: 200, headers: new Map([["content-type", "application/json"]]), json: async () => ({ status: 0, output: [{ stream: "stdout", text: "from the server\n" }] }) }; };
  const addresses: string[] = [];
  const parsed = new URL(url);
  const location = { pathname: parsed.pathname, search: parsed.search, hash: parsed.hash, origin: parsed.origin };
  const window = listen();
  const run = new Function("document", "localStorage", "fetch", "navigator", "performance", "history", "location", "window", "loadModule",
    script.replace(/import\(/, "loadModule("));
  const navigator: any = {};
  run(document, { getItem: () => null, setItem() {} }, (...args: unknown[]) => fetch(...args), navigator, { now: () => 0 }, { replaceState: (_: unknown, __: string, to: string) => addresses.push(to) }, location, window, load);
  const choose = (value: "server" | "browser") => {
    where[value].checked = true;
    for (const f of where[value].on.change ?? []) f();
  };
  return { nodes, where, loads, fetched, addresses, window, choose,
    setFetch: (next: Function) => { fetch = next; },
    clipboard: (write: (text: string) => void) => { navigator.clipboard = { writeText: async (text: string) => write(text) }; } };
}

async function browserRun(code: string, web: Record<string, unknown> | null, gpu = true) {
  const page = standIn({ code, web, gpu });
  await page.nodes.run.onclick();
  return { out: page.nodes.out, status: page.nodes.status.textContent, loads: page.loads, fetched: page.fetched, where: page.where };
}

test("Run in browser runs a plain program in rawr-web and shows its output as the server's", async () => {
  const web = { kind: () => "text", run_text: () => JSON.stringify({ kind: "text", status: 3, output: [{ stream: "stdout", text: "hi\n" }] }) };
  const { out, status, loads, fetched } = await browserRun("println!(\"hi\")", web);
  assert.deepEqual(loads, [WEB_MODULE]);
  assert.deepEqual(fetched, [], "the server is not asked");
  assert.equal(out.children[0].className, "term");
  assert.match(status, /in the browser · exit 3$/);
});

test("Run in browser mounts an iced snippet on a canvas holder and runs it there", async () => {
  const calls: unknown[][] = [];
  const web = { kind: () => "iced", run: (...args: unknown[]) => { calls.push(args); } };
  const { out, status, fetched } = await browserRun("text(\"hi\")", web);
  assert.equal(calls.length, 1);
  assert.equal(calls[0][0], "text(\"hi\")");
  assert.equal(out.children[0].className, "live");
  assert.ok(out.children[0].innerHTML.includes(`id="iced"`));
  assert.match(status, /· live$/);
  assert.deepEqual(fetched, []);
});

test("Run in browser falls back to the server when the module does not load or cannot draw", async () => {
  const failed = await browserRun("1 + 2", null);
  assert.deepEqual(failed.fetched, ["/play/render"]);
  assert.match(failed.status, /on the server: rawr-web did not load$/);
  assert.ok(failed.where.server.checked, "the choice goes back to the server");
  const iced = await browserRun("text(\"hi\")", { kind: () => "iced", run() { throw new Error("not called"); } }, false);
  assert.deepEqual(iced.fetched, ["/play/render"]);
  assert.match(iced.status, /no WebGL2 or WebGPU$/);
  const later = await browserRun("1 + 2", { kind: () => "server" });
  assert.deepEqual(later.fetched, ["/play/render"]);
  assert.match(later.status, /rawr-web cannot run this yet$/);
});

test("Run in browser runs an async main in rawr-web and shows its output as it is written", async () => {
  let seen: any[] = [];
  const page = standIn({ code: "#[tokio::main]\nasync fn main() {}", web: {
    kind: () => "async",
    run_async: async (_code: string, write: (stream: string, text: string) => void) => {
      write("stdout", "fetch at 100 ms\n");
      // The page shows a chunk the moment it is written.
      seen = page.nodes.out.children[0].children.map((span: any) => span.textContent);
      write("stdout", "3 jobs in 300 ms\n");
      return JSON.stringify({ kind: "text", status: 0, output: [{ stream: "stdout", text: "fetch at 100 ms\n3 jobs in 300 ms\n" }] });
    },
  } });
  await page.nodes.run.onclick();
  assert.deepEqual(seen, ["fetch at 100 ms\n"]);
  assert.deepEqual(page.fetched, [], "the server is not asked");
  assert.equal(page.nodes.out.children[0].className, "term");
  assert.equal(page.nodes.out.children[0].children[0].textContent, "fetch at 100 ms\n3 jobs in 300 ms\n");
  assert.match(page.nodes.status.textContent, /in the browser$/);
});

test("example slugs are stable, readable and unique", async () => {
  assert.equal(slug("Async tasks"), "async-tasks");
  assert.equal(slug("Counter"), "counter");
  assert.equal(slug("Enums, match and ?"), "enums-match-and");
  assert.equal(slug("Use after move (error)"), "use-after-move-error");
  // examples.ts imports .rs files, which only the Worker's build reads: its names come from its source.
  const source = (await import("node:fs")).readFileSync(new URL("../src/examples.ts", import.meta.url), "utf8");
  const slugs = [...source.matchAll(/name: "([^"]+)"/g)].map((m) => slug(m[1]));
  assert.ok(slugs.length > 20);
  assert.ok(slugs.every((s) => /^[a-z0-9]+(-[a-z0-9]+)*$/.test(s)));
  assert.equal(new Set(slugs).size, slugs.length);
});

const EXAMPLE_SET: Example[] = [
  { name: "Async tasks", code: "async fn main() {}", group: "Rust" },
  { name: "Counter", code: "counter code", messages: ["Message::Increment"], group: "iced" },
];

test("the address restores an example from its hash and the server from ?run=server", () => {
  const page = standIn({ url: "http://localhost/play?run=server#counter", examples: EXAMPLE_SET });
  assert.equal(page.nodes.code.value, "counter code");
  assert.equal(page.nodes.messages.value, "Message::Increment");
  assert.ok(page.where.server.checked && !page.where.browser.checked);
  assert.deepEqual(page.addresses, ["/play?run=server#counter"]);
  // The browser is the default: ?run=browser is dropped, and the hash kept.
  const browser = standIn({ url: "http://localhost/play?run=browser#async-tasks", examples: EXAMPLE_SET });
  assert.equal(browser.nodes.code.value, "async fn main() {}");
  assert.ok(browser.where.browser.checked);
  assert.deepEqual(browser.addresses, ["/play#async-tasks"]);
  // A bare address writes nothing.
  assert.deepEqual(standIn({ examples: EXAMPLE_SET }).addresses, []);
});

test("a share's code wins over an example's hash, which the share page drops", () => {
  const page = standIn({ code: "shared code", url: "http://localhost/play/abcDEF1234?run=server#counter", shared: "abcDEF1234", examples: EXAMPLE_SET });
  assert.equal(page.nodes.code.value, "shared code");
  assert.deepEqual(page.addresses, ["/play/abcDEF1234?run=server"]);
});

test("a share's link keeps ?run=server and drops the example's hash; opening it restores the server", async () => {
  const copied: string[] = [];
  const page = standIn({ code: "x", url: "http://localhost/play?run=server#counter", examples: EXAMPLE_SET });
  page.clipboard((text: string) => copied.push(text));
  page.setFetch(async () => ({ ok: true, json: async () => ({ id: "abcDEF1234" }) }));
  await page.nodes.share.onclick();
  assert.equal(page.addresses.at(-1), "/play/abcDEF1234?run=server");
  assert.deepEqual(copied, ["http://localhost/play/abcDEF1234?run=server"]);
  const opened = standIn({ code: "shared code", url: "http://localhost/play/abcDEF1234?run=server", shared: "abcDEF1234" });
  assert.ok(opened.where.server.checked);
  assert.equal(opened.nodes.code.value, "shared code");
});

test("choosing an example, the run mode and an edit keep the address with replaceState", async () => {
  const page = standIn({ code: "x", url: "http://localhost/play/abcDEF1234", shared: "abcDEF1234", examples: EXAMPLE_SET, web: { kind: () => "text", run_text: () => JSON.stringify({ kind: "text", status: 0, output: [] }) } });
  const target = { value: "1" };
  page.nodes.examples.onchange({ target });
  assert.equal(page.nodes.code.value, "counter code");
  assert.equal(page.addresses.at(-1), "/play#counter");
  page.choose("server");
  assert.equal(page.addresses.at(-1), "/play?run=server#counter");
  // An edit drops the hash: the code is no longer the example's.
  for (const f of page.nodes.code.on.input) f();
  assert.equal(page.addresses.at(-1), "/play?run=server");
  page.choose("browser");
  assert.equal(page.addresses.at(-1), "/play");
  await new Promise((done) => setTimeout(done, 10));
});

test("in the browser, rawr-web loads once the page has loaded, and a restored example runs", async () => {
  const ran: string[] = [];
  const web = { kind: () => "text", run_text: (code: string) => { ran.push(code); return JSON.stringify({ kind: "text", status: 0, output: [] }); } };
  const page = standIn({ url: "http://localhost/play#async-tasks", examples: EXAMPLE_SET, web });
  assert.deepEqual(page.loads, [], "nothing loads before the page has");
  for (const f of page.window.on.load) f();
  await new Promise((done) => setTimeout(done, 10));
  assert.deepEqual(page.loads, [WEB_MODULE]);
  assert.deepEqual(ran, ["async fn main() {}"]);
  // On the server, nothing loads.
  const server = standIn({ url: "http://localhost/play?run=server", web });
  for (const f of server.window.on.load) f();
  await new Promise((done) => setTimeout(done, 10));
  assert.deepEqual(server.loads, []);
});

test("rawr-web's files are served with types the edge compresses", () => {
  const headers = (path: string) => webHeaders(path, new Headers({ etag: "\"x\"", "content-type": "application/octet-stream" }));
  assert.equal(headers("/web/rawr_web_bg.wasm")?.get("content-type"), "application/wasm");
  assert.equal(headers("/web/rawr_web_bg.wasm")?.get("etag"), "\"x\"");
  assert.equal(headers("/web/rawr_web_bg.wasm")?.get("cache-control"), null, "the module revalidates");
  assert.equal(headers("/web/rawr_web.js")?.get("content-type"), "text/javascript; charset=utf-8");
  assert.equal(headers("/web/fonts/NotoSansCJK-Regular.ttc")?.get("content-type"), "font/ttf");
  assert.equal(headers("/web/fonts/FiraMono-Regular.ttf")?.get("cache-control"), "public, max-age=604800");
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
