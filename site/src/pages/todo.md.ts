// /todo.md: the work plan as Markdown (lib/agents.ts), from the data /todo
// is drawn from. The private tabs are not in it.
import { todoMarkdown, type Plan } from "../lib/agents.ts";
import data from "../lib/todo.json";

export const GET = () => new Response(todoMarkdown(data as unknown as Plan), { headers: { "content-type": "text/markdown; charset=utf-8" } });
