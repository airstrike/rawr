// The playground page at /play and /play/<id>. The site builds it as a
// static page (site/src/pages/play.astro, dist/play.html in the assets) with
// a React island that runs the snippet. For each request the Worker writes
// what only it knows into that page:
//   - the link preview: a share's own title, description and image;
//   - the data the island reads from #play-data: the snippet (a share's, or
//     the first example), the examples, Turnstile's site key and whether the
//     browser holds a pass, and the share's id;
//   - a share's theme, pinned on <html> so the page opens in it;
//   - Turnstile's script up front when the browser has no pass.
// The island (site/src/play/session.ts) keeps the address, the draft, the
// browser and server runs and sharing.

import { escape } from "./dashboard.ts";
import { ogTags, type Og, type Snippet } from "./play.ts";

export interface Example {
  name: string;
  code: string;
  /** Messages sent before the preview. */
  messages?: string[];
  /** The canvas size, WxH, when the default would cut the view off. */
  size?: string;
  /** The menu group: "Rust" or "iced". */
  group?: string;
}

export interface Page {
  snippet: Snippet;
  examples: Example[];
  /** Turnstile site key; no widget without one. */
  sitekey: string | null;
  /** The browser holds a valid pass: no widget unless the server refuses it. */
  pass?: boolean;
  /** The share this page was opened from. */
  shared: string | null;
  /** The link preview. */
  og: Og;
}

/** An example's name as the URL's hash names it: "Async tasks" is "async-tasks". */
export function slug(name: string): string {
  return name.toLowerCase().replace(/[^a-z0-9]+/g, "-").replace(/^-+|-+$/g, "");
}

export const TURNSTILE_JS = "https://challenges.cloudflare.com/turnstile/v0/api.js?render=explicit";

/** Where the page holds the island's data. */
export const DATA = '<script type="application/json" id="play-data">null</script>';

/** JSON safe inside a <script> element. */
export function scriptJson(value: unknown): string {
  return JSON.stringify(value).replace(/</g, "\\u003c").replace(/>/g, "\\u003e").replace(/&/g, "\\u0026");
}

/** The site's static playground page with this request's preview and data. */
export function inject(html: string, page: Page): string {
  const { snippet, examples, sitekey, shared } = page;
  const pass = Boolean(sitekey && page.pass);
  if (!html.includes(DATA)) throw new Error("the playground page has no #play-data");
  const data = { snippet, examples: examples.map((e) => ({ ...e, slug: slug(e.name) })), sitekey, pass, shared };
  // The site's preview tags give way to this request's.
  let out = html.replace(/<meta (?:property="og:[^"]*"|name="twitter:[^"]*"|name="description") content="[^"]*">\n?/g, "");
  const head = [ogTags(page.og, escape)];
  if (sitekey && !pass) head.push(`<script src="${TURNSTILE_JS}" async defer></script>`);
  out = out.replace("</head>", () => `${head.join("\n")}\n</head>`);
  if (shared) {
    const theme = snippet.theme === "light" ? "light" : "dark";
    out = out.replace(/<html lang="en">/, `<html lang="en" data-theme-pin="${theme}">`);
  }
  return out.replace(DATA, () => `<script type="application/json" id="play-data">${scriptJson(data)}</script>`);
}
