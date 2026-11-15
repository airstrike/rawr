// /play.md: what the playground is, as Markdown (lib/agents.ts), from the
// Playground guide and the Play reference's limits.
import { playMarkdown } from "../lib/agents.ts";
import { PAGES } from "../lib/docs.ts";
import { filled } from "../lib/sources.ts";

const page = (path: string) => PAGES.find((p) => p.path === path)!;

export const GET = async () =>
  new Response(playMarkdown(await filled(page("/docs/play")), await filled(page("/docs/reference/play"))), {
    headers: { "content-type": "text/markdown; charset=utf-8" },
  });
