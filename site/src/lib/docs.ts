// The docs' navigation and the build's passes over a page's HTML. A page is
// src/content/docs/<path>.html: its description in a <meta> line, then its
// <article>. The build gives it the site bar, the sidebar, the pager, a "#"
// link on each section heading, "On this page" for narrow screens,
// highlighted code, and the output of each runnable snippet, from running it.
// A new page is a file there and an entry in GROUPS.
import { highlight } from "./highlight.ts";
import { outputHtml, type Result } from "./output.ts";

export const SITE = "https://rawr.airstrike.fyi";

type Item = [path: string, title: string, children?: [string, string][]];

/** Each group's pages, in reading order. */
export const GROUPS: [string, Item[]][] = [
  ["Guides", [
    ["/docs/start", "Start"],
    ["/docs/run", "Run"],
    ["/docs/agents", "Agents"],
    ["/docs/imports", "Imports"],
    ["/docs/embedding", "Embedding"],
    ["/docs/apps", "Apps and libraries"],
    ["/docs/iced", "iced", [
      ["/docs/iced/reload", "Reload"],
      ["/docs/iced/previews", "Previews"],
    ]],
    ["/docs/ratatui", "ratatui"],
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

export interface Page {
  path: string;
  title: string;
  group: string | null;
  parent?: string;
}

export const INDEX: Page = { path: "/docs", title: "Docs", group: null };

/** Every docs page in reading order, the index first. */
export const PAGES: Page[] = [INDEX];
for (const [group, items] of GROUPS) {
  for (const [path, title, kids = []] of items) {
    PAGES.push({ path, title, group });
    for (const [kid, kidTitle] of kids) PAGES.push({ path: kid, title: kidTitle, group, parent: path });
  }
}

/** The pages that are a directory's index: /docs/iced is docs/iced/index.html. */
export const isIndex = (path: string) => ["/docs", "/docs/iced", "/docs/play"].includes(path);

/** The source file of a page, relative to src/content/docs. */
export const source = (path: string) => {
  const rel = path.slice("/docs".length).replace(/^\//, "");
  return isIndex(path) ? (rel ? `${rel}/index.html` : "index.html") : `${rel}.html`;
};

export const fullTitle = (page: Page) =>
  page === INDEX ? "rawr docs" : page.group === "API Reference" ? `${page.title} | rawr API Reference` : `${page.title} | rawr docs`;

/** The page's h2 sections, as [id, inner HTML]. */
const sections = (html: string) => [...html.matchAll(/<h2 id="([^"]+)">([\s\S]*?)<\/h2>/g)].map((m) => [m[1], m[2]] as const);

export function sidebar(page: Page, article: string): string {
  const out = ['<nav class="side" aria-label="Docs">'];
  const link = (path: string, title: string) => `<a href="${path}"${path === page.path ? ' aria-current="page"' : ""}>${title}</a>`;
  const secs = () => {
    const list = sections(article);
    return list.length > 1 ? `<ol class="secs">${list.map(([id, text]) => `<li><a href="#${id}">${text}</a></li>`).join("")}</ol>` : "";
  };
  for (const [group, items] of GROUPS) {
    const here = page.group === group ? " data-here" : "";
    out.push(`  <p class="grp"${here}>${group}</p>`, `  <ol${here}>`);
    for (const [path, title, kids = []] of items) {
      let inner = "";
      if (kids.length > 0) {
        inner = `<ol class="kids">${kids.map(([kid, kidTitle]) => `<li>${link(kid, kidTitle)}${kid === page.path ? secs() : ""}</li>`).join("")}</ol>`;
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

export function pager(page: Page): string {
  const at = PAGES.indexOf(page);
  const prev = PAGES[at - 1];
  const next = PAGES[at + 1];
  const parts = [];
  if (prev) parts.push(`<a class="prev" href="${prev.path}">Previous<b>${prev.title}</b></a>`);
  if (next) parts.push(`<a class="next" href="${next.path}">Next<b>${next.title}</b></a>`);
  return `<nav class="pager" aria-label="Next and previous pages">${parts.join("")}</nav>`;
}

const ENTITIES: Record<string, string> = { amp: "&", lt: "<", gt: ">", quot: '"', apos: "'", nbsp: " ", rsquo: "’", lsquo: "‘", rdquo: "”", ldquo: "“", mdash: "—", ndash: "–", hellip: "…", times: "×", rarr: "→", larr: "←", lsaquo: "‹", rsaquo: "›" };

/** Text of HTML with no markup, as a browser's textContent reads it. */
export function decode(html: string): string {
  return html.replace(/&(#x[0-9a-fA-F]+|#\d+|[A-Za-z]+);/g, (whole, name: string) => {
    if (name.startsWith("#x")) return String.fromCodePoint(parseInt(name.slice(2), 16));
    if (name.startsWith("#")) return String.fromCodePoint(parseInt(name.slice(1), 10));
    return ENTITIES[name] ?? whole;
  });
}

/**
 * Highlights each <code data-lang> block. A runnable block (data-run) keeps
 * its plain text, which is what /play/run's allow-list and the Run button
 * read; its highlighted copy follows it in a <template>, which the page swaps
 * in before the first paint. A block that already holds markup is left as is.
 */
export function highlightBlocks(html: string): string {
  return html.replace(/<code([^>]*\bdata-lang="([a-z]+)"[^>]*)>([^<]*)<\/code>/g, (whole, attributes: string, lang: string, text: string) => {
    const code = highlight(decode(text), lang);
    if (/\bdata-run\b/.test(attributes)) return `${whole}<template data-hl>${code}</template>`;
    return `<code${attributes}>${code}</code>`;
  });
}

/** rawr-web's API (web/src/lib.rs): a run's result as JSON text. */
export interface Web {
  run_text(code: string): string;
}

/** How the build runs a snippet, as a Run button would. */
export interface Runs {
  /** A fresh rawr-web, so no run sees another's state. */
  web(): Promise<Web>;
  /** A `data-where="server"` block's run on rawr's server. */
  server(code: string): Promise<Result>;
}

/** Each source's run, once per Runs: a snippet on two pages, or a page built twice, runs once. */
const ran = new WeakMap<Runs, Map<string, Promise<Result>>>();

function run(runs: Runs, block: { name: string; code: string; server: boolean }): Promise<Result> {
  let results = ran.get(runs);
  if (!results) ran.set(runs, (results = new Map()));
  const key = `${block.server ? "server" : "web"}:${block.code}`;
  let result = results.get(key);
  if (result) return result;
  result = block.server
    ? runs.server(block.code)
    : runs.web().then((web) => {
        try {
          return JSON.parse(web.run_text(block.code)) as Result;
        } catch (error) {
          throw new Error(`${block.name} stopped rawr-web (${error}). A snippet that needs rawr's server is marked data-where="server".`);
        }
      });
  results.set(key, result);
  return result;
}

/**
 * Fills each run strip's output with what its snippet prints. A snippet runs
 * in rawr-web, or on the server when it is marked `data-where="server"`, as
 * its Run button runs it. The source leaves each output empty: an output
 * written by hand, a button with no snippet, a run that rawr-web can't finish
 * and a run a limit stopped all fail the build.
 */
export async function runOutputs(html: string, runs: Runs): Promise<string> {
  const blocks = new Map<string, { name: string; code: string; server: boolean }>();
  for (const [, attributes, text] of html.matchAll(/<code([^>]*\bdata-run\b[^>]*)>([^<]*)<\/code>/g)) {
    const id = /\bid="([^"]+)"/.exec(attributes)?.[1];
    const name = /\bdata-run="([^"]*)"/.exec(attributes)?.[1] || "this snippet";
    if (!id) throw new Error(`the run block ${name} has no id`);
    blocks.set(id, { name, code: decode(text), server: /\bdata-where="server"/.test(attributes) });
  }
  const filled = new Map<string, string>();
  for (const [, source, out] of html.matchAll(/<button[^>]*\bdata-source="([^"]+)"[^>]*\bdata-out="([^"]+)"/g)) {
    const block = blocks.get(source);
    if (!block) throw new Error(`the Run button for ${out} names ${source}, which is no run block`);
    const result = await run(runs, block);
    if (result.limit) throw new Error(`${block.name}: a limit stopped its run, so it has no output to show`);
    filled.set(out, outputHtml(result));
  }
  for (const [id, block] of blocks) {
    if (!html.includes(`data-source="${id}"`)) throw new Error(`the run block ${block.name} has no Run button`);
  }
  return html.replace(/(<span id="([^"]+)"[^>]*>)([\s\S]*?)(<\/span>)/g, (whole, open: string, id: string, inner: string, close: string) => {
    const output = filled.get(id);
    if (output === undefined) return whole;
    if (inner !== "") throw new Error(`${id} holds an output written by hand; the build writes it from the run`);
    return open + output + close;
  });
}

/** A "#" link before each section heading that has an id. */
export function anchors(html: string): string {
  return html.replace(/<(h[23]) id="([^"]+)">/g, (_, tag, id) => `<${tag} id="${id}"><a class="anchor" href="#${id}" aria-label="Link to this section">#</a>`);
}

/** "On this page": the page's sections after its lede, shown on narrow screens. */
export function toc(html: string): string {
  const list = sections(html);
  const lede = /<p class="lede">[\s\S]*?<\/p>/.exec(html);
  if (list.length <= 2 || !lede) return html;
  const items = list.map(([id, text]) => `<li><a href="#${id}">${text.replace(/<a class="anchor"[^>]*>#<\/a>/, "").replace(/<[^>]+>/g, "")}</a></li>`).join("");
  const at = lede.index + lede[0].length;
  return `${html.slice(0, at)}\n<details class="toc"><summary>On this page</summary><ol>${items}</ol></details>${html.slice(at)}`;
}

/** A docs page's source: its description and its article. */
export function parse(source: string, file: string): { description: string; article: string } {
  const description = /^<meta name="description" content="([^"]*)">/.exec(source)?.[1];
  const article = /<article>([\s\S]*)<\/article>/.exec(source)?.[1];
  if (description === undefined || article === undefined) throw new Error(`${file}: a docs page is a description <meta> line and an <article>`);
  return { description, article };
}
