// Writes the shared parts of every docs page from the tree below: the title
// tags, the site bar, the sidebar, the h1, the pager, and the Run button's
// stylesheet and script on pages with a runnable block. Run it after adding
// a page or a section:
//   node site/docs-nav.mjs
// A new page is a copy of an existing one with its own <article>.
import { readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";

const root = join(import.meta.dirname, "public");
const SITE = "https://rawr.airstrike.fyi";

/** [path, title, children]. A group's pages are in reading order. */
const GROUPS = [
  ["Guides", [
    ["/docs/start", "Start"],
    ["/docs/run", "Run"],
    ["/docs/imports", "Imports"],
    ["/docs/embedding", "Embedding"],
    ["/docs/iced", "iced", [
      ["/docs/iced/reload", "Reload"],
      ["/docs/iced/previews", "Previews"],
    ]],
    ["/docs/play", "Play", [
      ["/docs/play/discord", "Discord"],
    ]],
    ["/docs/troubleshooting", "Troubleshooting"],
    ["/docs/how-it-works", "How it works"],
  ]],
  ["API Reference", [
    ["/docs/reference/overview", "Overview", [
      ["/docs/reference/items", "Items"],
      ["/docs/reference/types", "Types"],
      ["/docs/reference/generics", "Generics"],
      ["/docs/reference/patterns", "Patterns"],
      ["/docs/reference/moves", "Moves"],
      ["/docs/reference/closures", "Closures"],
      ["/docs/reference/macros", "Macros"],
      ["/docs/reference/std", "std"],
    ]],
    ["/docs/reference/iced", "iced"],
    ["/docs/reference/play", "Play"],
    ["/docs/reference/commands", "Commands"],
    ["/docs/reference/requirements", "Requirements"],
  ]],
];

const INDEX = { path: "/docs", title: "Docs", group: null };
const pages = [INDEX];
for (const [group, items] of GROUPS) {
  for (const [path, title, kids = []] of items) {
    pages.push({ path, title, group });
    for (const [kid, kidTitle] of kids) pages.push({ path: kid, title: kidTitle, group, parent: path });
  }
}

const file = (path) => {
  const rel = path.slice(1);
  return ["docs", "docs/iced", "docs/play"].includes(rel) ? join(root, rel, "index.html") : join(root, `${rel}.html`);
};

const BAR = `<header class="bar">
  <a class="brand" href="/" aria-label="rawr home"><span class="mark" aria-hidden="true"></span><b>rawr</b><span class="cap">pre-alpha</span></a>
  <nav class="links cap" aria-label="Site">
    <a href="/docs" aria-current="true">Docs</a>
    <a href="/play">Playground</a>
    <a class="wide" href="https://github.com/airstrike/rawr">GitHub</a>
  </nav>
</header>`;

/** The page's h2 sections, as [id, inner HTML]. */
const sections = (html) => [...html.matchAll(/<h2 id="([^"]+)">([\s\S]*?)<\/h2>/g)].map((m) => [m[1], m[2]]);

function sidebar(page, html) {
  const out = ['<nav class="side" aria-label="Docs">'];
  const link = (path, title) =>
    `<a href="${path}"${path === page.path ? ' aria-current="page"' : ""}>${title}</a>`;
  const secs = () => {
    const list = sections(html);
    return list.length > 1
      ? `<ol class="secs">${list.map(([id, text]) => `<li><a href="#${id}">${text}</a></li>`).join("")}</ol>`
      : "";
  };
  for (const [group, items] of GROUPS) {
    const here = page.group === group ? " data-here" : "";
    out.push(`  <p class="grp"${here}>${group}</p>`, `  <ol${here}>`);
    for (const [path, title, kids = []] of items) {
      let inner = "";
      if (kids.length > 0) {
        inner = `<ol class="kids">${kids.map(([kid, kidTitle]) => `<li>${link(kid, kidTitle)}${kid === page.path ? secs() : ""}</li>`).join("")}</ol>`;
        if (path === page.path) inner = secs() + inner;
      } else if (path === page.path) {
        inner = secs();
      }
      out.push(`    <li>${link(path, title)}${inner}</li>`);
    }
    out.push("  </ol>");
  }
  out.push("</nav>");
  return out.join("\n");
}

function pager(page) {
  const at = pages.indexOf(page);
  const prev = pages[at - 1];
  const next = pages[at + 1];
  const parts = [];
  if (prev) parts.push(`<a class="prev" href="${prev.path}">Previous<b>${prev.title}</b></a>`);
  if (next) parts.push(`<a class="next" href="${next.path}">Next<b>${next.title}</b></a>`);
  return `<nav class="pager" aria-label="Next and previous pages">${parts.join("")}</nav>`;
}

const replace = (html, pattern, value, path, what) => {
  if (!pattern.test(html)) throw new Error(`${path}: no ${what}`);
  return html.replace(pattern, () => value);
};

for (const page of pages) {
  const path = file(page.path);
  let html = readFileSync(path, "utf8");
  const full = page === INDEX ? "rawr docs" : page.group === "API Reference" ? `${page.title} | rawr API Reference` : `${page.title} | rawr docs`;
  html = replace(html, /<title>[^<]*<\/title>/, `<title>${full}</title>`, path, "title");
  html = replace(html, /<meta property="og:title" content="[^"]*">/, `<meta property="og:title" content="${full}">`, path, "og:title");
  html = replace(html, /<meta property="og:url" content="[^"]*">/, `<meta property="og:url" content="${SITE}${page.path}">`, path, "og:url");
  html = replace(html, /<link rel="canonical" href="[^"]*">/, `<link rel="canonical" href="${SITE}${page.path}">`, path, "canonical");
  html = replace(html, /<header class="bar">[\s\S]*?<\/header>/, BAR, path, "site bar");
  html = replace(html, /<h1>[\s\S]*?<\/h1>/, `<h1>${page.title}</h1>`, path, "h1");
  // No captions above headings.
  html = html.replace(/\n?<p class="cap"><span class="slash">\/\/<\/span>[^<]*<\/p>/g, "");
  html = replace(html, /<nav class="side"[\s\S]*?<\/nav>/, sidebar(page, html), path, "sidebar");
  if (!/<nav class="pager"/.test(html)) html = html.replace("</article>", "<nav class=\"pager\"></nav>\n</article>");
  html = replace(html, /<nav class="pager"[\s\S]*?<\/nav>/, pager(page), path, "pager");
  // The Run button's files, on the pages that have a runnable block.
  const runs = /<code[^>]*\bdata-run\b/.test(html);
  html = html.replace(/<link rel="stylesheet" href="\/run.css">\n/, "").replace(/<script src="\/run.js"><\/script>\n/, "");
  if (runs) {
    html = html
      .replace('<link rel="stylesheet" href="/docs/docs.css">\n', '<link rel="stylesheet" href="/docs/docs.css">\n<link rel="stylesheet" href="/run.css">\n')
      .replace('<script src="/docs/docs.js"></script>\n', '<script src="/docs/docs.js"></script>\n<script src="/run.js"></script>\n');
  }
  writeFileSync(path, html);
}
console.log(`docs-nav: ${pages.length} pages`);
