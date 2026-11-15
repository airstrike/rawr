// /docs/<slug>.md: each docs page as Markdown, beside its HTML page
// (lib/agents.ts). /docs/play.md is the page that is docs/play/index.html.
import type { APIRoute } from "astro";
import { INDEX, PAGES, type Page } from "../../lib/docs.ts";
import { markdown } from "../../lib/sources.ts";

export function getStaticPaths() {
  return PAGES.filter((page) => page !== INDEX).map((page) => ({ params: { slug: page.path.slice("/docs/".length) }, props: { page } }));
}

export const GET: APIRoute = async ({ props }) =>
  new Response(await markdown((props as { page: Page }).page), { headers: { "content-type": "text/markdown; charset=utf-8" } });
