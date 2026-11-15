// /llms.txt: the index an agent reads first (llmstxt.org). The text is
// src/content/llms.txt; its docs lines come from the navigation and each
// page's description (lib/agents.ts).
import template from "../content/llms.txt?raw";
import { llmsTxt } from "../lib/agents.ts";
import { descriptions } from "../lib/sources.ts";

export const GET = () => new Response(llmsTxt(template, descriptions()), { headers: { "content-type": "text/plain; charset=utf-8" } });
