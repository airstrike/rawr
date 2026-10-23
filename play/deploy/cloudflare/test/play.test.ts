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
  same,
  shareId,
  type Snippet,
} from "../src/play.ts";
import { escape } from "../src/dashboard.ts";
import { render } from "../src/playpage.ts";

const OG = {
  title: "Rawr Playground",
  description: "use iced::widget::{button, column};",
  url: "https://rawr.airstrike.fyi/play/abcDEF1234",
  image: "https://rawr.airstrike.fyi/play/abcDEF1234.png",
  alt: "The first frame",
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

test("link preview tags", () => {
  const tags = ogTags({ ...OG, description: 'say "hi" <b>' }, escape);
  assert.ok(tags.includes('<meta property="og:image" content="https://rawr.airstrike.fyi/play/abcDEF1234.png">'));
  assert.ok(tags.includes('<meta property="og:image:width" content="328">'));
  assert.ok(tags.includes('<meta property="og:image:height" content="382">'));
  assert.ok(tags.includes('<meta name="twitter:card" content="summary_large_image">'));
  assert.ok(tags.includes('<meta property="og:image:alt" content="The first frame">'));
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
  // What the page shows (and its Run button sends) is examples/scripts/cart.rawr, marked as a program.
  assert.equal(sources[0], "// rawr\n" + readFileSync(new URL("examples/scripts/cart.rawr", root), "utf8").trimEnd());
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
  assert.deepEqual(at("docs/index.html"), ["cart.rs"]);
  assert.deepEqual(at("docs/start.html"), ["cart.rs"]);
  assert.deepEqual(at("docs/language.html"), ["parse.rs", "generics.rs", "closures.rs"]);
  assert.deepEqual(at("docs/play.html"), ["", "fill.rs", "circle.rs"]);
  // The docs' cart.rs is the landing page's, so it is one entry.
  assert.equal(hashes.length, new Set(blocks.map((block: { hash: string }) => block.hash)).size);
  assert.equal(hashes.length, blocks.length - 2);
  assert.ok(hashes.includes(await sha256("let xs = vec![3, 1, 2];\nxs.iter().sum::<i64>()")));
  assert.ok(!hashes.includes(await sha256("println!(\"hi\");")));
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
