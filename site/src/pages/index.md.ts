// /index.md: the landing page as Markdown (lib/agents.ts), each snippet
// followed by what it prints, as the page shows it.
import main from "../content/index.html?raw";
import { landingMarkdown } from "../lib/agents.ts";
import { headingIds, runOutputs } from "../lib/docs.ts";
import { runs } from "../lib/runs.ts";

export const GET = async () =>
  new Response(landingMarkdown(headingIds(await runOutputs(main, runs))), { headers: { "content-type": "text/markdown; charset=utf-8" } });
