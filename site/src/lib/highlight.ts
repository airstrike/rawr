// Rust and TOML highlighting, the same at build time, in the docs' edited
// snippets and in the playground's editor. Rust goes through lezer's Rust
// grammar, which CodeMirror's Rust mode also uses, and every token gets one
// of the site's syntax classes, coloured by tokens.css's --syn-* properties
// (global.css):
//
//   k  keywords and self          t  type names
//   f  functions and macros       s  strings and chars
//   n  numbers and booleans       x  escapes inside a string
//   a  attributes, lifetimes and loop labels
//   c  comments                   d  doc comments (/// //! /** /*!)
//   p  a lowercase path segment (std::), in the text's colour
//
// Two classes the grammar's tags can't tell, so they come from a token's text
// (refine), here and in the editor: a doc comment, and a capitalised path
// segment (String::from, Vec::new), which is a type.
import { highlightCode, styleTags, tagHighlighter, tags as t } from "@lezer/highlight";
import { parser as rustParser } from "@lezer/rust";

/** Tags the grammar leaves out: a string's escapes, and a path call's name. */
export const rustTags = styleTags({
  Escape: t.escape,
  "CallExpression/ScopedIdentifier/Identifier": t.function(t.variableName),
});

const parser = rustParser.configure({ props: [rustTags] });

export const highlighter = tagHighlighter([
  { tag: [t.keyword, t.self], class: "k" },
  { tag: t.typeName, class: "t" },
  { tag: [t.function(t.variableName), t.function(t.propertyName), t.macroName], class: "f" },
  { tag: [t.string, t.character], class: "s" },
  { tag: [t.number, t.bool], class: "n" },
  { tag: t.escape, class: "x" },
  { tag: [t.meta, t.special(t.variableName), t.labelName], class: "a" },
  { tag: t.comment, class: "c" },
  // A path segment keeps the text's colour; its own span lets refine find it.
  { tag: t.namespace, class: "p" },
]);

/**
 * The class a node's text gives it over its tags: d for a doc comment, as
 * rustdoc reads one, and t for a path segment that names a type.
 */
export function refine(node: string, text: string): string | null {
  if (node === "LineComment" || node === "BlockComment") return /^(\/\/\/(?!\/)|\/\/!|\/\*\*(?![*/])|\/\*!)/.test(text) ? "d" : null;
  if (node === "ScopeIdentifier") return /^[A-Z]/.test(text) ? "t" : null;
  return null;
}

const escape = (text: string) => text.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;");
const span = (cls: string, text: string) => (cls ? `<span class="${cls}">${escape(text)}</span>` : escape(text));

/** Rust source as HTML: escaped text, with a span for each highlighted token. */
export function highlightRust(code: string): string {
  const tree = parser.parse(code);
  // Each refined node's class and end, by where it starts.
  const refined = new Map<number, [string, number]>();
  tree.iterate({
    enter: (node) => {
      const cls = refine(node.name, code.slice(node.from, node.to));
      if (cls) refined.set(node.from, [cls, node.to]);
    },
  });
  let html = "";
  let at = 0;
  let over: [string, number] = ["", -1];
  highlightCode(
    code,
    tree,
    highlighter,
    (text, classes) => {
      over = refined.get(at) ?? over;
      html += span(at < over[1] ? over[0] : classes, text);
      at += text.length;
    },
    () => {
      html += "\n";
      at += 1;
    },
  );
  return html;
}

/** TOML as HTML: a table header is a keyword, strings literals, numbers and booleans numbers. */
export function highlightToml(code: string): string {
  const token = /(#[^\n]*)|("(?:\\.|[^"\\\n])*"|'[^'\n]*')|(^[ \t]*\[[^\]\n]*\]+)|(\b(?:true|false)\b|\b\d[\d_.:TZ-]*\b)/gm;
  let html = "";
  let last = 0;
  for (const m of code.matchAll(token)) {
    html += escape(code.slice(last, m.index));
    last = m.index + m[0].length;
    const [, comment, string, table] = m;
    html += span(comment ? "c" : string ? "s" : table ? "k" : "n", m[0]);
  }
  return html + escape(code.slice(last));
}

export function highlight(code: string, lang: string): string {
  return lang === "toml" ? highlightToml(code) : highlightRust(code);
}
