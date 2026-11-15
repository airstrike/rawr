// node --test test/agents.test.ts
// The site as text for agents: the Markdown copy of each page, llms.txt and
// llms-full.txt. Once the site is built, the files in dist/ are checked too.
import assert from "node:assert/strict";
import { existsSync, readFileSync } from "node:fs";
import { test } from "node:test";
import { MD_FILES, PLAY_PAGES, article, describe, docsMarkdown, landingMarkdown, llmsFull, llmsTxt, mdLink, mdPath, playMarkdown, siteLinks, todoMarkdown, type Plan } from "../src/lib/agents.ts";
import { PAGES, source } from "../src/lib/docs.ts";
import { toMarkdown } from "../src/lib/markdown.ts";

const content = new URL("../src/content/", import.meta.url);
const read = (rel: string) => readFileSync(new URL(rel, content), "utf8");
const doc = (path: string) => {
  const file = `docs/${source(path)}`;
  return { text: read(file), file };
};
const descriptions = new Map(PAGES.map((page) => [page.path, describe(doc(page.path).text, page.path)]));
const llms = llmsTxt(read("llms.txt"), descriptions);
const dist = new URL("../dist/", import.meta.url);

test("code blocks, tables, lists and links keep their content", () => {
  const md = toMarkdown(
    '<h2 id="a">A <code>x</code></h2><p>See <a href="/docs/run#marker">Run</a> and <b>bold</b>, <code>a`b</code>.</p>' +
      '<div class="block"><div class="file"><b>src/main.rs</b></div><pre><code data-lang="rust">fn main() {\n    let a = &amp;1;\n}</code></pre>' +
      '<div class="run"><button type="button" class="runbtn" hidden></button><b>rawr run</b><span id="out-x"><span>1\n2</span></span></div></div>' +
      '<table><tr><th>A</th><th>B</th></tr><tr><td><code>a|b</code></td><td>two <a href="https://example.com">x</a></td></tr></table>' +
      "<ul><li>one</li><li>two<ul><li>three</li></ul></li></ul><pre><code>```\nfence\n```</code></pre>",
    mdLink,
  );
  assert.equal(
    md,
    [
      "## A `x`",
      "See [Run](https://rawr.airstrike.fyi/docs/run.md#marker) and **bold**, ``a`b``.",
      "`src/main.rs`:",
      "```rust\nfn main() {\n    let a = &1;\n}\n```",
      "Output of `rawr run`:",
      "```text\n1\n2\n```",
      "| A | B |\n| --- | --- |\n| `a\\|b` | two [x](https://example.com) |",
      "- one\n- two\n\n  - three",
      "````text\n```\nfence\n```\n````",
    ].join("\n\n") + "\n",
  );
});

test("a link to a page goes to its Markdown copy; other links stay absolute", () => {
  assert.equal(mdLink("/"), "https://rawr.airstrike.fyi/index.md");
  assert.equal(mdLink("/docs"), "https://rawr.airstrike.fyi/docs.md");
  assert.equal(mdLink("/docs/play#share"), "https://rawr.airstrike.fyi/docs/play.md#share");
  assert.equal(mdLink("/play?run=server"), "https://rawr.airstrike.fyi/play.md?run=server");
  assert.equal(mdLink("/install.sh"), "https://rawr.airstrike.fyi/install.sh");
  assert.equal(mdLink("#here"), "#here");
  assert.equal(mdLink("https://github.com/airstrike/rawr"), "https://github.com/airstrike/rawr");
});

test("every docs page has a Markdown copy, with its title and no markup left", () => {
  for (const page of PAGES) {
    assert.ok(MD_FILES.includes(mdPath(page.path)), page.path);
    const { text, file } = doc(page.path);
    const md = docsMarkdown(page, article(text, file));
    assert.ok(md.startsWith(`# ${toMarkdown(`<h1>${page.title}</h1>`).trim().slice(2)}\n`), `${page.path} starts with its title`);
    const prose = md.replace(/(`{3,})[^\n]*\n[\s\S]*?\n\1/g, "").replace(/(`+)[^`]*?\1/g, "");
    assert.doesNotMatch(prose, /<\/?(p|div|span|code|pre|table|td|li|ul|a|b|button)\b[^>]*>/, `${page.path} has HTML left in its prose`);
    assert.doesNotMatch(prose, /&(amp|lt|gt|rsquo|quot);/, `${page.path} has an entity left`);
  }
});

test("the landing page, /play and /todo have their copies", () => {
  const landing = landingMarkdown(read("index.html"));
  assert.ok(landing.startsWith("# rawr\n"));
  assert.match(landing, /curl -LsSf https:\/\/rawr\.airstrike\.fyi\/install\.sh \| sh/);
  const play = playMarkdown(article(doc("/docs/play").text, "play"), article(doc("/docs/reference/play").text, "play reference"));
  assert.match(play, /^# rawr playground\n/);
  assert.match(play, /## Limits\n/);
  assert.match(play, /WebAssembly/);
  const plan = JSON.parse(readFileSync(new URL("../src/lib/todo.json", import.meta.url), "utf8")) as Plan;
  const todo = todoMarkdown(plan);
  assert.ok(todo.includes(`Version: ${plan.version}.`));
  assert.match(todo, /## Open groups\n/);
  const open = plan.items.find((i) => i.state === "open");
  if (open) assert.ok(todo.includes(`#${open.issue} `), "an open issue is listed");
});

test("llms.txt is an llmstxt.org index whose site links are files the build writes", () => {
  assert.match(llms, /^# rawr\n\n> /);
  for (const section of ["Docs", "Playground", "Work plan", "Source", "Author", "Optional"]) assert.match(llms, new RegExp(`^## ${section}$`, "m"));
  assert.doesNotMatch(llms, /\{\{/);
  const written = new Set([...MD_FILES, "/llms.txt", "/llms-full.txt", "/install.sh", "/install.ps1", "/bot/invite"]);
  for (const path of siteLinks(llms)) assert.ok(written.has(path), `llms.txt links ${path}, which the build doesn't write`);
  for (const page of PAGES) assert.ok(llms.includes(`(https://rawr.airstrike.fyi${mdPath(page.path)})`), `llms.txt lists ${page.path}`);
  for (const path of PLAY_PAGES) assert.ok(llms.split("## Playground")[1].includes(mdPath(path)), `${path} is under Playground`);
});

test("llms-full.txt holds every docs page", () => {
  const pages = PAGES.map((page) => ({ page, markdown: docsMarkdown(page, article(doc(page.path).text, page.path), { footer: false }) }));
  const full = llmsFull(pages);
  assert.ok(full.length > 100_000);
  for (const page of PAGES) assert.ok(full.includes(`<!-- https://rawr.airstrike.fyi${page.path} -->`), page.path);
});

test("the build wrote each text file, and each page names its Markdown copy", { skip: !existsSync(new URL("llms.txt", dist)) && "the site isn't built" }, () => {
  for (const path of MD_FILES) {
    const file = new URL(path.slice(1), dist);
    assert.ok(existsSync(file), `dist${path}`);
    assert.ok(readFileSync(file, "utf8").length > 200, `dist${path} has content`);
  }
  const built = readFileSync(new URL("llms.txt", dist), "utf8");
  for (const path of siteLinks(built)) {
    const rel = path.slice(1);
    assert.ok(existsSync(new URL(rel, dist)) || existsSync(new URL(`${rel}/index.html`, dist)) || path === "/bot/invite", `llms.txt links ${path}, which dist/ lacks`);
  }
  assert.ok(readFileSync(new URL("llms-full.txt", dist), "utf8").length > 100_000, "llms-full.txt is the whole docs");
  for (const page of PAGES) {
    const html = page.path === "/docs" || page.path === "/docs/play" ? `${page.path.slice(1)}/index.html` : `${page.path.slice(1)}.html`;
    assert.ok(readFileSync(new URL(html, dist), "utf8").includes(`<link rel="alternate" type="text/markdown" href="${mdPath(page.path)}">`), `${html} names its copy`);
  }
  for (const [html, md] of [["index.html", "/index.md"], ["play.html", "/play.md"], ["todo.html", "/todo.md"]]) {
    assert.ok(readFileSync(new URL(html, dist), "utf8").includes(`<link rel="alternate" type="text/markdown" href="${md}">`), `${html} names its copy`);
  }
});
