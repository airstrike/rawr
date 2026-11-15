// HTML to Markdown for the site's own pages, so an agent can read each page
// as text (/docs/run.md beside /docs/run). It reads the HTML the pages are
// built from, which is hand-written and well formed, and knows the site's
// blocks: a file's name above its code, a run strip with its output, a
// diagnostic. Code blocks, tables and links keep their content. Controls
// (buttons, inputs, labels) and anything aria-hidden are left out.
import { decode } from "./docs.ts";

type Node = string | Element;

interface Element {
  tag: string;
  attrs: Record<string, string>;
  children: Node[];
}

const VOID = new Set(["area", "br", "col", "embed", "hr", "img", "input", "link", "meta", "source", "wbr"]);

const TOKEN = /<!--[\s\S]*?-->|<\/([a-zA-Z][\w-]*)\s*>|<([a-zA-Z][\w-]*)((?:\s+[^\s=>/]+(?:\s*=\s*(?:"[^"]*"|'[^']*'|[^\s>]+))?)*)\s*(\/?)>|[^<]+|</g;
const ATTR = /([^\s=]+)(?:\s*=\s*(?:"([^"]*)"|'([^']*)'|([^\s>]+)))?/g;

/** The element tree of an HTML fragment. Text stays as written, entities and all. */
export function parseHtml(html: string): Element {
  const root: Element = { tag: "#root", attrs: {}, children: [] };
  const stack = [root];
  for (const m of html.matchAll(TOKEN)) {
    const top = stack[stack.length - 1];
    const [whole, close, open, rawAttrs = "", selfClose] = m;
    if (whole.startsWith("<!--")) continue;
    if (close) {
      const at = stack.map((e) => e.tag).lastIndexOf(close.toLowerCase());
      if (at > 0) stack.length = at;
      continue;
    }
    if (open) {
      const attrs: Record<string, string> = {};
      for (const a of rawAttrs.matchAll(ATTR)) attrs[a[1].toLowerCase()] = decode(a[2] ?? a[3] ?? a[4] ?? "");
      const el: Element = { tag: open.toLowerCase(), attrs, children: [] };
      top.children.push(el);
      if (!VOID.has(el.tag) && !selfClose) stack.push(el);
      continue;
    }
    top.children.push(whole);
  }
  return root;
}

/** What a page's links point at: a site path becomes its Markdown page where it has one. */
export type LinkMap = (href: string) => string;

const BLOCK = new Set(["address", "article", "aside", "blockquote", "details", "div", "dl", "dt", "dd", "figure", "footer", "h1", "h2", "h3", "h4", "h5", "h6", "header", "hr", "li", "main", "nav", "ol", "p", "pre", "section", "summary", "table", "ul"]);
const SKIP = new Set(["button", "input", "label", "template", "script", "style", "meta", "link", "select", "textarea", "svg"]);
const SKIP_CLASS = ["toc", "tabs", "divider", "hint", "copy", "crumbs", "pager"];

const classes = (el: Element) => (el.attrs.class ?? "").split(/\s+/).filter(Boolean);
const has = (el: Element, name: string) => classes(el).includes(name);
const skipped = (node: Node) =>
  typeof node !== "string" && (SKIP.has(node.tag) || node.attrs["aria-hidden"] === "true" || node.attrs.hidden !== undefined || classes(node).some((c) => SKIP_CLASS.includes(c)));
const isBlock = (node: Node) => typeof node !== "string" && BLOCK.has(node.tag);
const elements = (el: Element, tag?: string) => el.children.filter((c): c is Element => typeof c !== "string" && (!tag || c.tag === tag));

/** An element's text, as a browser's textContent reads it, without the parts the page hides. */
function text(node: Node): string {
  if (typeof node === "string") return decode(node);
  if (node.tag === "template" || node.tag === "button") return "";
  if (node.tag === "br") return "\n";
  return node.children.map(text).join("");
}

/** A code span that holds `code`, with a fence longer than any backtick run inside it. */
function codeSpan(code: string): string {
  const one = code.replace(/\s*\n\s*/g, " ");
  if (!one) return "";
  const longest = Math.max(0, ...[...one.matchAll(/`+/g)].map((m) => m[0].length));
  const tick = "`".repeat(longest + 1);
  const pad = one.startsWith("`") || one.endsWith("`") ? " " : "";
  return `${tick}${pad}${one}${pad}${tick}`;
}

/** A fenced block of `code`, with a fence longer than any backtick run inside it. */
function fence(code: string, lang = ""): string {
  const longest = Math.max(2, ...[...code.matchAll(/^`+/gm)].map((m) => m[0].length));
  const tick = "`".repeat(longest + 1);
  return `${tick}${lang}\n${code.replace(/\n+$/, "")}\n${tick}`;
}

/** Wraps `inner` in a mark such as `**`, keeping its outer spaces outside. */
function wrap(inner: string, mark: string): string {
  const core = inner.trim();
  if (!core) return inner;
  const lead = inner.startsWith(" ") ? " " : "";
  const trail = inner.endsWith(" ") ? " " : "";
  return `${lead}${mark}${core}${mark}${trail}`;
}

function inline(node: Node, links: LinkMap): string {
  if (typeof node === "string") return decode(node).replace(/\s+/g, " ");
  if (skipped(node)) return "";
  const kids = () => node.children.map((c) => inline(c, links)).join("");
  switch (node.tag) {
    case "code":
      return codeSpan(text(node));
    case "a": {
      const label = kids().trim();
      const href = node.attrs.href;
      if (!label) return "";
      return href ? `[${label}](${links(href)})` : label;
    }
    case "b":
    case "strong":
      return wrap(kids(), "**");
    case "i":
    case "em":
      return wrap(kids(), "*");
    case "br":
      return "  \n";
    case "img": {
      const src = node.attrs.src ?? "";
      const alt = node.attrs.alt ?? "image";
      return src.startsWith("data:") ? `(${alt})` : `![${alt}](${links(src)})`;
    }
    default:
      return kids();
  }
}

/** Escapes what would end a table cell. */
const cell = (s: string) => s.replace(/\|/g, "\\|").replace(/\s*\n\s*/g, " ").trim();

function table(el: Element, links: LinkMap): string {
  const rows: Element[] = [];
  const walk = (e: Element) => {
    for (const c of elements(e)) c.tag === "tr" ? rows.push(c) : ["thead", "tbody", "tfoot"].includes(c.tag) && walk(c);
  };
  walk(el);
  if (!rows.length) return "";
  const cells = rows.map((r) =>
    elements(r).filter((c) => c.tag === "td" || c.tag === "th").map((c) => cell(blocks(c, links).join(" <br> "))),
  );
  const width = Math.max(...cells.map((r) => r.length));
  const line = (r: string[]) => `| ${[...r, ...Array(width - r.length).fill("")].join(" | ")} |`;
  return [line(cells[0]), `|${" --- |".repeat(width)}`, ...cells.slice(1).map(line)].join("\n");
}

function list(el: Element, links: LinkMap): string {
  const ordered = el.tag === "ol";
  return elements(el, "li")
    .map((li, i) => {
      const mark = ordered ? `${i + 1}. ` : "- ";
      const body = blocks(li, links).join("\n\n");
      const indent = " ".repeat(mark.length);
      return mark + body.split("\n").map((l, j) => (j && l ? indent + l : l)).join("\n");
    })
    .join("\n");
}

/** A run strip: the command it shows, then the output the build wrote into it. */
function runStrip(el: Element, links: LinkMap): string[] {
  const label = elements(el, "b").map((b) => text(b).trim()).join(" ");
  const note = elements(el, "small").map((s) => text(s).trim()).join(" ");
  const out = elements(el, "span").at(-1);
  if (!out) return [];
  const pictures = (function find(e: Element): Element[] {
    return elements(e).flatMap((c) => (c.tag === "img" ? [c] : find(c)));
  })(out);
  const said = text(out).replace(/\n+$/, "");
  const parts: string[] = [];
  const title = [label && `Output of ${codeSpan(label)}`, note].filter(Boolean).join(", ") || "Output";
  const head = title[0].toUpperCase() + title.slice(1);
  if (pictures.length) parts.push(`${head}: a window, which the HTML page shows as a picture.`);
  if (said.trim()) parts.push(`${pictures.length ? "It prints" : head}:`, fence(said, "text"));
  return parts;
}

/** The blocks of an element's content, each one Markdown block. */
function blocks(el: Element, links: LinkMap): string[] {
  const out: string[] = [];
  let run: Node[] = [];
  const flush = () => {
    const para = run.map((n) => inline(n, links)).join("").replace(/[ \t]+\n/g, "\n").replace(/ {2,}/g, " ").trim();
    if (para) out.push(para);
    run = [];
  };
  for (const node of el.children) {
    if (skipped(node)) continue;
    if (!isBlock(node)) {
      run.push(node);
      continue;
    }
    flush();
    out.push(...block(node as Element, links));
  }
  flush();
  return out;
}

function block(el: Element, links: LinkMap): string[] {
  const heading = /^h([1-6])$/.exec(el.tag);
  if (heading) {
    const title = el.children.map((c) => inline(c, links)).join("").replace(/\s+/g, " ").trim();
    return title ? [`${"#".repeat(+heading[1])} ${title}`] : [];
  }
  switch (el.tag) {
    case "pre": {
      const code = elements(el, "code")[0];
      return [fence(text(el), code?.attrs["data-lang"] ?? "text")];
    }
    case "ul":
    case "ol":
      return [list(el, links)];
    case "table":
      return [table(el, links)];
    case "hr":
      return ["---"];
    case "blockquote":
      return [blocks(el, links).join("\n\n").split("\n").map((l) => `> ${l}`.trimEnd()).join("\n")];
    case "summary":
      return [wrap(blocks(el, links).join(" "), "**")];
  }
  if (has(el, "file")) {
    const name = elements(el, "b").map((b) => text(b).trim()).join(" ");
    const note = elements(el, "span").map((s) => text(s).trim()).filter(Boolean).join(", ");
    return name ? [`${codeSpan(name)}${note ? ` (${note})` : ""}:`] : [];
  }
  if (has(el, "run")) return runStrip(el, links);
  if (has(el, "cards") || has(el, "paths")) {
    // A card is a link whose <span> says what is behind it; an <i> numbers it.
    return [elements(el, "a").map((a) => {
      const title = a.children.filter((c) => typeof c === "string" || (c.tag !== "span" && c.tag !== "i")).map((c) => (typeof c !== "string" && c.tag === "b" ? text(c) : inline(c, links))).join("").trim();
      const about = elements(a, "span").map((s) => inline(s, links)).join(" ").trim();
      return `- [${title}](${links(a.attrs.href ?? "")})${about ? `: ${about}` : ""}`;
    }).join("\n")];
  }
  return blocks(el, links);
}

/** Markdown for an HTML fragment. `links` maps each href. */
export function toMarkdown(html: string, links: LinkMap = (href) => href): string {
  return blocks(parseHtml(html), links).join("\n\n").replace(/\n{3,}/g, "\n\n").trim() + "\n";
}
