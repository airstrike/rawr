// node --test test/docs.test.ts
// The build's passes over a page: highlighting, run blocks, headings, the navigation.
import assert from "node:assert/strict";
import { readdirSync, readFileSync } from "node:fs";
import { test } from "node:test";
import { PAGES, anchors, decode, highlightBlocks, pager, parse, sidebar, source, toc } from "../src/lib/docs.ts";
import { highlightRust, highlightToml } from "../src/lib/highlight.ts";

test("Rust gets the site's syntax classes", () => {
  const html = highlightRust('// rawr\nfn main() { let s: String = "a<b".into(); println!("{}", 1.5); }');
  assert.ok(html.startsWith('<span class="c">// rawr</span>\n<span class="k">fn</span> main()'));
  assert.ok(html.includes('<span class="t">String</span>'));
  assert.ok(html.includes('<span class="s">"a&lt;b"</span>'), "text is escaped");
  assert.ok(html.includes('<span class="t">println</span><span class="t">!</span>'));
  assert.ok(html.includes('<span class="s">1.5</span>'));
  assert.equal(decode(html.replace(/<[^>]+>/g, "")), '// rawr\nfn main() { let s: String = "a<b".into(); println!("{}", 1.5); }');
});

test("TOML: tables are keywords, strings and numbers literals", () => {
  assert.equal(highlightToml('[dependencies]\ntokio = { version = "1" } # x'), '<span class="k">[dependencies]</span>\ntokio = { version = <span class="s">"1"</span> } <span class="c"># x</span>');
});

test("a run block keeps its plain text for /play/run, with its highlighted copy beside it", () => {
  const html = highlightBlocks('<pre><code data-lang="rust" data-run="x.rs" id="src-x">let a = &amp;1;</code></pre><pre><code data-lang="rust">fn f() {}</code></pre>');
  assert.ok(html.startsWith('<pre><code data-lang="rust" data-run="x.rs" id="src-x">let a = &amp;1;</code><template data-hl><span class="k">let</span> a = &amp;<span class="s">1</span>;</template></pre>'));
  assert.ok(html.endsWith('<pre><code data-lang="rust"><span class="k">fn</span> f() {}</code></pre>'));
  // A block that already holds markup is left alone.
  const marked = '<code data-lang="rust"><span class="k">?play</span></code>';
  assert.equal(highlightBlocks(marked), marked);
});

test("headings get a # link, and a long page gets On this page after its lede", () => {
  const article = '<h1>T</h1>\n<p class="lede">L</p>\n<h2 id="a">A <code>x</code></h2><h2 id="b">B</h2><h3 id="c">C</h3><h2 id="d">D</h2>';
  const out = toc(anchors(article));
  assert.ok(out.includes('<h2 id="a"><a class="anchor" href="#a" aria-label="Link to this section">#</a>A <code>x</code></h2>'));
  assert.ok(out.includes('<h3 id="c"><a class="anchor" href="#c"'));
  assert.ok(out.includes('<p class="lede">L</p>\n<details class="toc"><summary>On this page</summary><ol><li><a href="#a">A x</a></li><li><a href="#b">B</a></li><li><a href="#d">D</a></li></ol></details>'));
  assert.equal(toc('<p class="lede">L</p><h2 id="a">A</h2>'), '<p class="lede">L</p><h2 id="a">A</h2>', "two sections or fewer: no list");
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
  assert.ok(side.includes('<a href="/docs/run" aria-current="page">Run</a><ol class="secs"><li><a href="#a">A</a></li><li><a href="#b">B</a></li></ol>'));
  assert.ok(side.includes('<p class="grp" data-here>Guides</p>'));
  assert.equal(pager(run), '<nav class="pager" aria-label="Next and previous pages"><a class="prev" href="/docs/start">Previous<b>Start</b></a><a class="next" href="/docs/agents">Next<b>Agents</b></a></nav>');
});
