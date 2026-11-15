// /llms-full.txt: every docs page's Markdown in one file (lib/agents.ts).
import { llmsFull } from "../lib/agents.ts";
import { PAGES } from "../lib/docs.ts";
import { markdown } from "../lib/sources.ts";

export const GET = async () => {
  const pages = [];
  for (const page of PAGES) pages.push({ page, markdown: await markdown(page, { footer: false }) });
  return new Response(llmsFull(pages), { headers: { "content-type": "text/plain; charset=utf-8" } });
};
