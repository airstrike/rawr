// The playground page at /play and /play/<id>. The site builds it as a
// static page (site/src/pages/play.astro, dist/play.html in the assets) with
// a React island that runs the snippet in the browser. For each request the
// Worker writes what only it knows into that page:
//   - the link preview: a share's own title, description and image;
//   - the data the island reads from #play-data: the snippet (a share's, or
//     the first example), the examples and the share's id.
// The island (site/src/play/session.ts) keeps the address, the draft, the
// browser's runs and sharing.

import { escape } from "./dashboard.ts";
import { ogTags, type Og, type Snippet } from "./play.ts";

export interface Example {
  name: string;
  code: string;
  /** The menu group: "Rust" or "Rustlings". */
  group?: string;
}

export interface Page {
  snippet: Snippet;
  examples: Example[];
  /** The share this page was opened from. */
  shared: string | null;
  /** The link preview. */
  og: Og;
}

/** An example's name as the URL's hash names it: "Async tasks" is "async-tasks". */
export function slug(name: string): string {
  return name.toLowerCase().replace(/[^a-z0-9]+/g, "-").replace(/^-+|-+$/g, "");
}

/** Where the page holds the island's data. */
export const DATA = '<script type="application/json" id="play-data">null</script>';

/** JSON safe inside a <script> element. */
export function scriptJson(value: unknown): string {
  return JSON.stringify(value).replace(/</g, "\\u003c").replace(/>/g, "\\u003e").replace(/&/g, "\\u0026");
}

/** The site's static playground page with this request's preview and data. */
export function inject(html: string, page: Page): string {
  const { snippet, examples, shared } = page;
  if (!html.includes(DATA)) throw new Error("the playground page has no #play-data");
  const data = { snippet, examples: examples.map((e) => ({ ...e, slug: slug(e.name) })), shared };
  // The site's preview tags give way to this request's.
  let out = html.replace(/<meta (?:property="og:[^"]*"|name="twitter:[^"]*"|name="description") content="[^"]*">\n?/g, "");
  out = out.replace("</head>", () => `${ogTags(page.og, escape)}\n</head>`);
  return out.replace(DATA, () => `<script type="application/json" id="play-data">${scriptJson(data)}</script>`);
}
