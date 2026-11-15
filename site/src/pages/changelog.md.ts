// /changelog.md: the repository's CHANGELOG.md, which plan/plan.mjs writes
// from plan/changelog.json, the source of /todo's changelog tab.
import changelog from "../../../CHANGELOG.md?raw";
import { changelogMarkdown } from "../lib/agents.ts";
import data from "../lib/todo.json";

export const GET = () => new Response(changelogMarkdown(changelog, data.repo), { headers: { "content-type": "text/markdown; charset=utf-8" } });
