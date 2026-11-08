// node --test test/docs.test.ts
// The build's passes over a page: highlighting, run blocks, headings, the navigation.
import assert from "node:assert/strict";
import { readdirSync, readFileSync } from "node:fs";
import { test } from "node:test";
import { NEXT_IMAGE, PAGES, anchors, breadcrumbs, decode, headingIds, highlightBlocks, pager, parse, runOutputs, sidebar, source, toc, type Runs } from "../src/lib/docs.ts";
import type { Result } from "../src/lib/output.ts";
import { highlightRust, highlightToml } from "../src/lib/highlight.ts";

test("Rust gets the site's syntax classes", () => {
  const html = highlightRust('// rawr\nfn main() { let s: String = "a<b".into(); println!("{}", 1.5); }');
  assert.ok(html.startsWith('<span class="c">// rawr</span>\n<span class="k">fn</span> <span class="f">main</span>()'));
  assert.ok(html.includes('<span class="t">String</span>'));
  assert.ok(html.includes('<span class="s">"a&lt;b"</span>'), "text is escaped");
  assert.ok(html.includes('<span class="f">println</span><span class="f">!</span>'));
  assert.ok(html.includes('<span class="n">1.5</span>'));
  assert.equal(decode(html.replace(/<[^>]+>/g, "")), '// rawr\nfn main() { let s: String = "a<b".into(); println!("{}", 1.5); }');
});

test("Rust: functions, macros, escapes, attributes, lifetimes, doc comments and type paths each get a class", () => {
  const html = highlightRust(String.raw`//! crate
/// item
// note
#[derive(Debug)]
struct A<'a>(&'a str);
fn f() { let v = Vec::new(); x.cost(); std::mem::drop("a\n"); true }`);
  assert.ok(html.startsWith('<span class="d">//! crate</span>\n<span class="d">/// item</span>\n<span class="c">// note</span>\n<span class="a">#[derive(Debug)]</span>'));
  assert.ok(html.includes(`&lt;<span class="a">'a</span>&gt;`), "a lifetime");
  assert.ok(html.includes('<span class="k">fn</span> <span class="f">f</span>()'), "a function's name");
  assert.ok(html.includes('<span class="t">Vec</span>::<span class="f">new</span>()'), "a type in a path, and a path call");
  assert.ok(html.includes('x.<span class="f">cost</span>()'), "a method call");
  assert.ok(html.includes('std::mem::<span class="f">drop</span>'), "a module keeps the text's colour");
  assert.ok(html.includes(String.raw`<span class="s">"a</span><span class="x">\n</span><span class="s">"</span>`), "an escape");
  assert.ok(html.includes('<span class="n">true</span>'));
  assert.equal(highlightRust("let x = Some(MAX);"), '<span class="k">let</span> x = <span class="t">Some</span>(MAX);', "a variant is a type's colour, a constant the text's");
  assert.equal(highlightRust("#[derive(Debug)] struct S;"), '<span class="a">#[derive(Debug)]</span> <span class="k">struct</span> <span class="t">S</span>;', "an attribute is one colour");
  assert.equal(highlightRust("/** a\n b */ /**/ //// c"), '<span class="d">/** a</span>\n<span class="d"> b */</span> <span class="c">/**/</span> <span class="c">//// c</span>');
});

test("TOML: tables are keywords, strings literals, numbers and booleans numbers", () => {
  assert.equal(highlightToml('[dependencies]\ntokio = { version = "1", n = 2 } # x'), '<span class="k">[dependencies]</span>\ntokio = { version = <span class="s">"1"</span>, n = <span class="n">2</span> } <span class="c"># x</span>');
});

test("a run block keeps its plain text for /play/run, with its highlighted copy beside it", () => {
  const html = highlightBlocks('<pre><code data-lang="rust" data-run="x.rs" id="src-x">let a = &amp;1;</code></pre><pre><code data-lang="rust">fn f() {}</code></pre>');
  assert.ok(html.startsWith('<pre><code data-lang="rust" data-run="x.rs" id="src-x">let a = &amp;1;</code><template data-hl><span class="k">let</span> a = &amp;<span class="n">1</span>;</template></pre>'));
  assert.ok(html.endsWith('<pre><code data-lang="rust"><span class="k">fn</span> <span class="f">f</span>() {}</code></pre>'));
  // A block that already holds markup is left alone.
  const marked = '<code data-lang="rust"><span class="k">?play</span></code>';
  assert.equal(highlightBlocks(marked), marked);
});

/** A run block with its strip, as the content files write it: the output empty. */
const block = (id: string, code: string, where = "") =>
  `<pre><code data-lang="rust" data-run="${id}.rs" id="src-${id}"${where}>${code}</code></pre>` +
  `<div class="run"><button type="button" class="runbtn" data-source="src-${id}" data-out="out-${id}" hidden></button><b>rawr run ${id}.rs</b><span id="out-${id}" aria-live="polite"></span></div>`;

/** A stand-in rawr-web that answers each source from `results`, and the server likewise. */
function standIn(results: Record<string, unknown>) {
  const ran: string[] = [];
  const runs: Runs = {
    web: async () => ({
      run_text(code: string) {
        ran.push(`web ${code}`);
        if (!(code in results)) throw new Error("unreachable");
        return JSON.stringify(results[code]);
      },
    }),
    server: async (code) => (ran.push(`server ${code}`), results[code] as Result),
  };
  return { runs, ran };
}

test("the build runs each snippet and writes its output as a Run shows it", async () => {
  const { runs, ran } = standIn({
    'println!("a<b");': { kind: "text", status: 0, output: [{ stream: "stdout", text: "a<b\n" }] },
    "bad": { kind: "error", text: "error[E0382]: borrow of moved value: `a`\n" },
    "fails": { kind: "text", status: 1, output: [{ stream: "stdout", text: "n = 42\n" }, { stream: "stderr", text: "Error: x\n" }] },
    "thread": { kind: "text", status: 0, output: [{ stream: "stdout", text: "4\n" }] },
    "draw": { kind: "text", status: 0, output: [], frame: { type: "image/png", data: "AAAA" } },
  });
  const html = await runOutputs(
    block("a", "println!(&quot;a&lt;b&quot;);") + block("b", "bad") + block("c", "fails") + block("d", "thread", ' data-where="server"') + block("e", "draw") + block("f", "println!(&quot;a&lt;b&quot;);"),
    runs,
  );
  const out = (id: string) => new RegExp(`<span id="out-${id}" aria-live="polite">(.*?)</span></div>`, "s").exec(html)?.[1];
  assert.equal(out("a"), "<span>a&lt;b</span>", "the text is escaped and loses its last newline");
  assert.equal(out("b"), '<span class="err">error[E0382]: borrow of moved value: `a`</span>', "a compile error carries the smoke test's class=\"err\"");
  assert.equal(out("c"), '<span>n = 42\n</span><span class="err">Error: x</span><span class="exit">\nexit status 1</span>');
  assert.equal(out("d"), "<span>4</span>", "a server block's output comes from the server");
  assert.equal(out("e"), '<img src="data:image/png;base64,AAAA" alt="What the program drew" style="display: block; max-width: 100%;">', "what an iced program drew");
  assert.equal(out("f"), out("a"));
  assert.deepEqual(ran, ['web println!("a<b");', "web bad", "web fails", "server thread", "web draw"], "each source runs once, and only a server block on the server");
});

test("a server block that waits for the next image says so while the live server can't compile it, then shows its run", async () => {
  const { runs } = standIn({
    "ratatui": { kind: "error", text: "error[E0432]: unresolved import `ratatui::buffer`\n" },
    "iced": { kind: "text", status: 0, output: [{ stream: "stdout", text: "drawn\n" }] },
  });
  const next = ' data-where="server" data-image="next"';
  const warnings: string[] = [];
  const warn = console.warn;
  console.warn = (message: string) => warnings.push(message);
  let html: string;
  try {
    html = await runOutputs(block("a", "ratatui", next) + block("b", "iced", next), runs);
  } finally {
    console.warn = warn;
  }
  const out = (id: string) => new RegExp(`<span id="out-${id}" aria-live="polite">(.*?)</span></div>`, "s").exec(html)?.[1];
  assert.equal(out("a"), NEXT_IMAGE, "no compile error, so the smoke test doesn't expect one");
  assert.match(out("a")!, /class="next"/);
  assert.equal(out("b"), "<span>drawn</span>", "once the live server runs it, its output is the run's");
  assert.equal(warnings.length, 2);
  assert.match(warnings[0], /a\.rs needs the server's next image/);
  assert.match(warnings[1], /b\.rs runs on the live server/);
  await assert.rejects(runOutputs(block("c", "iced", ' data-image="next"'), runs), /c\.rs waits for the server's next image, so it is marked data-where="server"/);
});

test("the build refuses an output written by hand, a snippet rawr-web can't run and a run a limit stopped", async () => {
  const { runs } = standIn({ ok: { kind: "text", status: 0, output: [] }, slow: { kind: "text", status: 0, output: [], limit: true } });
  await assert.rejects(runOutputs(block("a", "ok").replace("></span></div>", ">42</span></div>"), runs), /written by hand/);
  await assert.rejects(runOutputs(block("b", "spawn"), runs), /stopped rawr-web.*data-where="server"/);
  await assert.rejects(runOutputs(block("c", "slow"), runs), /a limit stopped its run/);
  await assert.rejects(runOutputs('<pre><code data-lang="rust" data-run="x.rs" id="src-x">ok</code></pre>', runs), /has no Run button/);
});

test("each heading links to itself, and a long page gets On this page after its lede", () => {
  const article = '<h1>T</h1>\n<p class="lede">L</p>\n<h2 id="a">A <code>x</code></h2><h2 id="b">B</h2><h3 id="c">C</h3><h2 id="d">D</h2>';
  const out = toc(anchors(article));
  assert.ok(out.includes('<h2 id="a"><a class="anchor" href="#a">A <code>x</code></a></h2>'));
  assert.ok(out.includes('<h3 id="c"><a class="anchor" href="#c">C</a></h3>'));
  assert.ok(out.includes('<p class="lede">L</p>\n<details class="toc"><summary>On this page</summary><ol><li><a href="#a">A x</a></li><li><a href="#b">B</a></li><li><a href="#d">D</a></li></ol></details>'));
  assert.equal(toc('<p class="lede">L</p><h2 id="a">A</h2>'), '<p class="lede">L</p><h2 id="a">A</h2>', "two sections or fewer: no list");
  assert.equal(anchors('<h2 id="a">See <a href="/x">x</a></h2>'), '<h2 id="a">See <a href="/x">x</a></h2>', "a heading that holds a link stays as it is");
});

test("a heading without an id gets one from its text, unique on the page", () => {
  const html = headingIds('<h2>Run Rust in your browser</h2><h2 class="k">Pre-alpha</h2><h3 id="run-rust-in-your-browser-2">Kept</h3><h2>Run Rust in <code>your</code> browser</h2><h3>Don&rsquo;t</h3>');
  assert.equal(
    html,
    '<h2 id="run-rust-in-your-browser">Run Rust in your browser</h2><h2 class="k" id="pre-alpha">Pre-alpha</h2><h3 id="run-rust-in-your-browser-2">Kept</h3><h2 id="run-rust-in-your-browser-3">Run Rust in <code>your</code> browser</h2><h3 id="don-t">Don&rsquo;t</h3>',
  );
  assert.ok(anchors(html).startsWith('<h2 id="run-rust-in-your-browser"><a class="anchor" href="#run-rust-in-your-browser">Run Rust in your browser</a></h2><h2 class="k" id="pre-alpha"><a class="anchor" href="#pre-alpha">'));
});

test("every page in the navigation has its file, and every file a place in the navigation", () => {
  const root = new URL("../src/content/docs/", import.meta.url);
  const files = readdirSync(root, { recursive: true }).map(String).filter((f) => f.endsWith(".html")).sort();
  assert.deepEqual(PAGES.map((page) => source(page.path)).sort(), files);
  for (const page of PAGES) {
    const { description, article } = parse(readFileSync(new URL(source(page.path), root), "utf8"), page.path);
    assert.ok(description.length > 20, page.path);
    assert.ok(article.includes(`<h1>${page.title}</h1>`), `${page.path}'s h1 is its title in the navigation`);
  }
});

test("the sidebar marks the page and lists its sections; the pager links its neighbours", () => {
  const run = PAGES.find((page) => page.path === "/docs/run")!;
  const side = sidebar(run, '<h2 id="a">A</h2><h2 id="b">B</h2>');
  assert.ok(side.startsWith('<nav class="side" id="docs-nav" popover aria-label="Docs">'), "the tree is the bar's drawer on narrow screens");
  assert.ok(side.includes('<a href="/docs/run" aria-current="page">Run</a><ol class="secs"><li><a href="#a">A</a></li><li><a href="#b">B</a></li></ol>'));
  assert.ok(side.includes('<p class="grp" data-here>Guides</p>'));
  assert.deepEqual([...side.matchAll(/<p class="grp"[^>]*>([^<]+)<\/p>/g)].map((m) => m[1]), ["Get started", "Guides", "Reference", "Tools"]);
  assert.equal(pager(run), '<nav class="pager" aria-label="Next and previous pages"><a class="prev" href="/docs/start">Previous<b>Start</b></a><a class="next" href="/docs/imports">Next<b>Imports</b></a></nav>');
});

test("breadcrumbs name the group and the parent page; the index has none", () => {
  const items = PAGES.find((page) => page.path === "/docs/reference/items")!;
  assert.equal(breadcrumbs(items), '<nav class="crumbs label" aria-label="Breadcrumb"><ol><li><a href="/docs">Docs</a></li><li>Reference</li><li><a href="/docs/reference/overview">Overview</a></li></ol></nav>');
  assert.equal(breadcrumbs(PAGES[0]), "");
});
