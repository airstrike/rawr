// Rust and TOML highlighting, the same at build time, in the docs' edited
// snippets and in the playground's editor. Rust goes through lezer's Rust
// grammar, which CodeMirror's Rust mode also uses, and every token gets one
// of the site's syntax classes (global.css): k for keywords, s for literals,
// t for type and macro names, c for comments and attributes.
import { highlightCode, tagHighlighter, tags as t } from "@lezer/highlight";
import { parser } from "@lezer/rust";

export const highlighter = tagHighlighter([
  { tag: [t.keyword, t.self, t.bool], class: "k" },
  { tag: [t.string, t.character, t.number, t.literal], class: "s" },
  { tag: [t.typeName, t.macroName], class: "t" },
  { tag: [t.comment, t.meta], class: "c" },
]);

const escape = (text: string) => text.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;");
const span = (cls: string, text: string) => (cls ? `<span class="${cls}">${escape(text)}</span>` : escape(text));

/** Rust source as HTML: escaped text, with a span for each highlighted token. */
export function highlightRust(code: string): string {
  let html = "";
  highlightCode(code, parser.parse(code), highlighter, (text, classes) => (html += span(classes, text)), () => (html += "\n"));
  return html;
}

/** TOML as HTML: a table header is a keyword, strings and numbers are literals. */
export function highlightToml(code: string): string {
  const token = /(#[^\n]*)|("(?:\\.|[^"\\\n])*"|'[^'\n]*')|(^[ \t]*\[[^\]\n]*\]+)|(\b(?:true|false)\b)|(\b\d[\d_.:TZ-]*\b)/gm;
  let html = "";
  let last = 0;
  for (const m of code.matchAll(token)) {
    html += escape(code.slice(last, m.index));
    last = m.index + m[0].length;
    const [, comment, string, table, bool] = m;
    html += span(comment ? "c" : string ? "s" : table || bool ? "k" : "s", m[0]);
  }
  return html + escape(code.slice(last));
}

export function highlight(code: string, lang: string): string {
  return lang === "toml" ? highlightToml(code) : highlightRust(code);
}
