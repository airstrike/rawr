// /docs.md: the docs home as Markdown (lib/agents.ts).
import { INDEX } from "../lib/docs.ts";
import { markdown } from "../lib/sources.ts";

export const GET = async () => new Response(await markdown(INDEX), { headers: { "content-type": "text/markdown; charset=utf-8" } });
