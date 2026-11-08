// A run's result as the strip under a snippet shows it. The Run button
// (scripts/run.ts) and the build (docs.ts's runOutputs) both draw it with
// outputHtml, so the output the page is built with is the one a Run shows.

/** One stream's run of a program's output. */
export interface Chunk {
  stream: "stdout" | "stderr";
  text: string;
}

/**
 * What rawr-web's run_text gives and what the server answers: a program's
 * output and status, with what an iced program drew, or a compile error.
 */
export interface Result {
  kind?: string;
  /** A compile error's report. */
  text?: string;
  status?: number;
  output?: Chunk[];
  frame?: { type: string; data: string };
  /** True when one of the playground's limits stopped the run. */
  limit?: boolean;
}

export const escapeHtml = (text: string) =>
  text.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;").replace(/"/g, "&quot;");

const span = (text: string, cls: string) => `<span${cls ? ` class="${cls}"` : ""}>${escapeHtml(text)}</span>`;

/**
 * The strip's content for a result. A compile error is one red span. A run
 * is its picture, if it drew one, then each chunk of output as a span, red
 * for stderr. The last chunk loses its trailing newline. A status other than
 * 0 follows as "exit status N".
 */
export function outputHtml(result: Result): string {
  if (result.kind === "error") return span((result.text ?? "").replace(/\n$/, ""), "err");
  const parts: string[] = [];
  if (result.frame) {
    const src = `data:${result.frame.type};base64,${result.frame.data}`;
    parts.push(`<img src="${escapeHtml(src)}" alt="What the program drew" style="display: block; max-width: 100%;">`);
  }
  const chunks = result.output ?? [];
  chunks.forEach((chunk, i) => {
    const text = i === chunks.length - 1 ? chunk.text.replace(/\n$/, "") : chunk.text;
    parts.push(span(text, chunk.stream === "stderr" ? "err" : ""));
  });
  if (result.status !== 0) parts.push(span((parts.length > 0 ? "\n" : "") + "exit status " + result.status, "exit"));
  return parts.join("");
}
