// The site as text for agents: a Markdown page beside each HTML page
// (/docs/run.md, /index.md, /play.md, /todo.md), the changelog as
// /changelog.md, /llms.txt, the index an agent reads first, and
// /llms-full.txt, every docs page in one file. Each is built from the source
// its HTML page is built from, so the two never drift. The endpoints under
// src/pages (*.md.ts, llms.txt.ts, llms-full.txt.ts) call these.
import { INDEX, PAGES, SITE, decode, headingIds, parse, type Page } from "./docs.ts";
import { toMarkdown } from "./markdown.ts";
import type { Group, Item, Row } from "./todo.ts";

/** The site's pages that have a Markdown copy, by HTML path, beside the docs. */
export const EXTRA = ["/", "/play", "/todo"] as const;

/** The Markdown copy of an HTML path: /docs/run is /docs/run.md, / is /index.md. */
export const mdPath = (path: string) => (path === "/" ? "/index.md" : `${path}.md`);

/** Every Markdown file the build writes, by its path. */
export const MD_FILES: string[] = [...PAGES.map((page) => mdPath(page.path)), ...EXTRA.map(mdPath), "/changelog.md"];

const HAS_MD = new Set<string>([...PAGES.map((page) => page.path), ...EXTRA]);

/** A link on a page, as its Markdown copy writes it: absolute, and to the Markdown copy of a page that has one. */
export function mdLink(href: string): string {
  if (!href.startsWith("/") || href.startsWith("//")) return href;
  const [, path, rest] = /^([^?#]*)(.*)$/.exec(href)!;
  return HAS_MD.has(path) ? `${SITE}${mdPath(path)}${rest}` : `${SITE}${href}`;
}

/** A page's description, as text. */
export const describe = (source: string, file: string) => decode(parse(source, file).description);

/** A docs page's article, with the ids its HTML page gives its headings. */
export const article = (source: string, file: string) => headingIds(parse(source, file).article);

const footer = (path: string) => `\n---\n\nThis is the Markdown copy of ${SITE}${path === "/" ? "" : path}. Every page is listed in ${SITE}/llms.txt.\n`;

/**
 * A docs page as Markdown, from its article. Pass the article after the
 * build's runOutputs, so each snippet is followed by what it prints.
 */
export function docsMarkdown(page: Page, html: string, { footer: withFooter = true } = {}): string {
  const md = toMarkdown(html, mdLink);
  return withFooter ? md + footer(page.path) : md;
}

/** The section of an article that starts at the h2 with id `id`, up to the next h2. */
export function section(html: string, id: string): string {
  const at = html.indexOf(`<h2 id="${id}"`);
  if (at < 0) throw new Error(`no h2 with id ${id}`);
  const next = html.indexOf("<h2", at + 3);
  return html.slice(at, next < 0 ? undefined : next);
}

/** The landing page as Markdown. Its title is two spans that read as one line. */
export function landingMarkdown(html: string): string {
  const main = html.replace(/<h1>[\s\S]*?<\/h1>/, "<h1>rawr</h1><p>rawr is rust augmented with rust.</p>");
  return toMarkdown(main, mdLink) + footer("/");
}

/** /play as Markdown: what the playground is, then the Playground guide and the limits from the Play reference. */
export function playMarkdown(guide: string, reference: string): string {
  const intro = `# rawr playground

> ${SITE}/play runs Rust snippets in the browser, and only there. rawr itself is compiled to WebAssembly and loads into the page, so a program runs with no server. The ?play bot in Discord runs snippets on its own server.

- Open: ${SITE}/play (an example: ${SITE}/play#<example>).
- Share: the Share button saves the code at ${SITE}/play/<id>.
- A plain snippet uses rawr's std and no crates. A snippet that uses iced, tokio or ratatui runs on the playground's prebuilt runner, which is built for the browser. Its window is drawn in the output pane and takes clicks and keys.
- A snippet may start with a cargo-script frontmatter block. The page runs it when each dependency it names is one of the prebuilt runner's crates. The ?play bot also builds the approved crates that the Play reference lists.
- In Discord, a snippet runs in a sandbox with no files, environment, process or network.
- Discord: ${SITE}${mdPath("/docs/play/discord")}. The sandbox in full: ${SITE}${mdPath("/docs/reference/play")}.
`;
  const body = toMarkdown(guide.replace(/<h1>[\s\S]*?<\/h1>/, ""), mdLink);
  const limits = toMarkdown(section(reference, "limits"), mdLink);
  return `${intro}\n${body}\n${limits}${footer("/play")}`;
}

/** The plan's data, as plan/plan.mjs writes it into site/src/lib/todo.json. */
export interface Plan {
  repo: string;
  updated: string;
  version: string;
  examples: { running: number; total: number; note: string };
  groups: Group[];
  items: Item[];
  changelog: { unreleased: { notes: string[]; entries: { text: string; issues: number[] }[] } };
}

const STATE = { done: "done", now: "in progress", next: "next" } as const;

/** /todo as Markdown: what the plan is, the version, the open work on the chart, and the open issues. */
export function todoMarkdown(plan: Plan): string {
  const issues = `https://github.com/${plan.repo}/issues`;
  const open = plan.items.filter((i) => i.state === "open");
  const out = [
    `# rawr work plan

> rawr's work plan: a chart of the work, the changelog and every issue. Its HTML page is ${SITE}/todo. Each item is a GitHub issue in ${plan.repo}, and #N is ${issues}/N.

- Version: ${plan.version}. Updated: ${plan.updated}.
- iced examples: ${plan.examples.running} of iced master's ${plan.examples.total} examples run under \`rawr run\`. ${plan.examples.note}
- Issues: ${plan.items.length} in all: ${open.length} open, ${plan.items.filter((i) => i.state === "closed").length} closed, ${plan.items.filter((i) => i.state === "retired").length} retired (closed as not planned: superseded, duplicate or no longer applicable).
- Changelog: ${SITE}/changelog.md. Each merge to main bumps the patch version, and a minor version marks a milestone. A shipped issue is in the GitHub milestone of the minor series it shipped in, such as 0.3 (${issues.replace(/issues$/, "milestones")}).

## The chart

The chart groups the work by milestone, then by group, then by row. A row is a piece of work and links its issues. A bar's length is the work's complexity, how much code changes, never time. A row is done, in progress or next.
`,
  ];
  const done = plan.groups.filter((g) => g.rows.every((r) => r.state === "done"));
  const active = plan.groups.filter((g) => !done.includes(g));
  out.push(`Groups whose rows are all done: ${done.map((g) => `${g.title} (${g.subtitle})`).join("; ")}.\n`);
  out.push("## Open groups\n");
  for (const group of active) {
    out.push(`### ${group.title}\n\n${group.subtitle}.\n`);
    const rows = group.rows.filter((r) => r.state !== "done");
    const finished = group.rows.length - rows.length;
    out.push(rows.map((row) => rowLine(row)).join("\n") + (finished ? `\n- ${finished} done ${finished === 1 ? "row" : "rows"}.` : "") + "\n");
  }
  out.push("## Unreleased\n");
  out.push([...plan.changelog.unreleased.notes, ...plan.changelog.unreleased.entries.map((e) => e.text + (e.issues.length ? ` (${e.issues.map((n) => `#${n}`).join(", ")})` : ""))].map((t) => `- ${t}`).join("\n") + "\n");
  out.push(`## Open issues\n\nBy category, as the TODO tab lists them. Each is ${issues}/N.\n`);
  const by = new Map<string, Item[]>();
  for (const item of open) by.set(item.category, [...(by.get(item.category) ?? []), item]);
  for (const [category, items] of by) out.push(`### ${category}\n\n${items.map((i) => `- #${i.issue} ${i.title}`).join("\n")}\n`);
  return out.join("\n") + footer("/todo");
}

function rowLine(row: Row): string {
  const open = row.issues.filter((i) => i.state === "open").map((i) => `#${i.number}`);
  const closed = row.issues.length - open.length;
  const parts = [`- **${row.name}** (${STATE[row.state]}${row.milestone ? `, milestone ${row.milestone}` : ""}).`];
  if (row.note) parts.push(row.note.replace(/\.?$/, "."));
  if (open.length) parts.push(`Open: ${open.join(", ")}.`);
  if (closed) parts.push(`${closed} closed.`);
  return parts.join(" ");
}

/** CHANGELOG.md as the site serves it, with a line on where its issue numbers lead. */
export function changelogMarkdown(changelog: string, repo: string): string {
  const line = `Each \`#N\` is the issue https://github.com/${repo}/issues/N. The work plan is ${SITE}/todo.md.`;
  return changelog.replace(/^(# [^\n]*\n)/, `$1\n${line}\n`);
}

/** A page's line in llms.txt: its link and its description. */
const entry = (page: Page, description: string) =>
  `- [${page === INDEX ? "Docs home" : page.group === "Reference" ? `Reference: ${page.title}` : page.title}](${SITE}${mdPath(page.path)}): ${description}`;

/** The docs pages llms.txt lists under Playground; every other page goes under Docs. */
export const PLAY_PAGES = ["/docs/play", "/docs/play/discord", "/docs/reference/play"];

/**
 * llms.txt: src/content/llms.txt, with its {{docs}} and {{play}} lines
 * filled from the docs' navigation and each page's description.
 */
export function llmsTxt(template: string, descriptions: Map<string, string>): string {
  const line = (page: Page) => {
    const description = descriptions.get(page.path);
    if (!description) throw new Error(`no description for ${page.path}`);
    return entry(page, description);
  };
  // One flat list in reading order, as llmstxt.org's sections are.
  const docs = PAGES.filter((page) => !PLAY_PAGES.includes(page.path)).map(line);
  const play = PLAY_PAGES.map((path) => line(PAGES.find((page) => page.path === path)!));
  return template.replace("{{docs}}", docs.join("\n")).replace("{{play}}", play.join("\n"));
}

/** llms-full.txt: every docs page's Markdown in reading order, each under its URL. */
export function llmsFull(pages: { page: Page; markdown: string }[]): string {
  const head = `# rawr docs, in full

> Every page of ${SITE}/docs as Markdown, in reading order. ${SITE}/llms.txt is the short index.
`;
  return [head, ...pages.map(({ page, markdown }) => `<!-- ${SITE}${page.path} -->\n\n${markdown.trim()}\n`)].join("\n---\n\n");
}

/** The links llms.txt makes to the site, as paths. */
export const siteLinks = (text: string) =>
  [...text.matchAll(/\]\((https:\/\/rawr\.airstrike\.fyi[^)\s]*)\)/g)].map((m) => new URL(m[1]).pathname);
