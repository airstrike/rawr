// The docs' sources for the build's text endpoints (lib/agents.ts): each
// page's file in src/content/docs, its description, and its article with the
// output of each snippet, from the runs its HTML page made. Vite only.
import { article, describe, docsMarkdown } from "./agents.ts";
import { PAGES, runOutputs, source, type Page } from "./docs.ts";
import { runs } from "./runs.ts";

const files = import.meta.glob<string>("../content/docs/**/*.html", { query: "?raw", import: "default", eager: true });

/** A docs page's source file. */
export function file(page: Page): { name: string; text: string } {
  const name = `../content/docs/${source(page.path)}`;
  const text = files[name];
  if (text === undefined) throw new Error(`no ${name} for ${page.path}`);
  return { name, text };
}

/** A docs page's article, each snippet followed by its output, as its HTML page has it. */
export async function filled(page: Page): Promise<string> {
  const { name, text } = file(page);
  return runOutputs(article(text, name), runs);
}

/** A docs page as Markdown. */
export async function markdown(page: Page, options?: { footer?: boolean }): Promise<string> {
  return docsMarkdown(page, await filled(page), options);
}

/** Each docs page's description, by path. */
export const descriptions = () =>
  new Map(PAGES.map((page) => {
    const { name, text } = file(page);
    return [page.path, describe(text, name)];
  }));
