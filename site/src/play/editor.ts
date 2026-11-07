// The playground's editor: CodeMirror 6 with Rust, highlighted in the site's
// syntax classes (lib/highlight.ts), so its colours are the docs' in either
// theme. Its look comes from the theme's CSS variables, so a change of theme
// needs nothing here.
import { defaultKeymap, history, historyKeymap, indentWithTab } from "@codemirror/commands";
import { rustLanguage } from "@codemirror/lang-rust";
import { LanguageSupport, bracketMatching, indentUnit, syntaxHighlighting, syntaxTree } from "@codemirror/language";
import { EditorState, RangeSetBuilder } from "@codemirror/state";
import { Decoration, type DecorationSet, EditorView, ViewPlugin, type ViewUpdate, drawSelection, keymap } from "@codemirror/view";
import { eachRefined, highlighter, rustTags } from "../lib/highlight.ts";

// The static highlighter's grammar: lezer's Rust with highlight.ts's tags.
const rust = new LanguageSupport(rustLanguage.configure({ props: [rustTags] }));

// The classes a token's text gives it (highlight.ts's refine), as marks over what is in view.
const marks = new Map<string, Decoration>();
function refined(view: EditorView): DecorationSet {
  const builder = new RangeSetBuilder<Decoration>();
  // A node across two visible ranges is met twice; it is marked once.
  let last = -1;
  for (const { from, to } of view.visibleRanges) {
    eachRefined(
      syntaxTree(view.state),
      (from, to) => view.state.sliceDoc(from, to),
      (from, to, cls) => {
        if (from < last) return;
        last = from;
        if (!marks.has(cls)) marks.set(cls, Decoration.mark({ class: cls }));
        builder.add(from, to, marks.get(cls)!);
      },
      from,
      to,
    );
  }
  return builder.finish();
}
const refinement = ViewPlugin.fromClass(
  class {
    decorations: DecorationSet;
    constructor(view: EditorView) {
      this.decorations = refined(view);
    }
    update(update: ViewUpdate) {
      if (update.docChanged || update.viewportChanged || syntaxTree(update.startState) !== syntaxTree(update.state)) this.decorations = refined(update.view);
    }
  },
  { decorations: (plugin) => plugin.decorations },
);

const look = EditorView.theme({
  "&": { height: "100%", backgroundColor: "var(--wash)", color: "var(--ink)", border: "1px solid var(--grid)", fontSize: "13px" },
  "&.cm-focused": { outline: "none", borderColor: "var(--ink)" },
  ".cm-scroller": { fontFamily: "var(--mono)", lineHeight: "1.55", overflow: "auto" },
  ".cm-content": { padding: "12px 0", caretColor: "var(--red)" },
  ".cm-line": { padding: "0 12px" },
  ".cm-cursor, .cm-dropCursor": { borderLeftColor: "var(--red)", borderLeftWidth: "2px" },
  "&.cm-focused > .cm-scroller > .cm-selectionLayer .cm-selectionBackground, .cm-selectionBackground, .cm-content ::selection": {
    backgroundColor: "color-mix(in srgb, var(--red) 28%, transparent)",
  },
  ".cm-matchingBracket, &.cm-focused .cm-matchingBracket": { backgroundColor: "transparent", outline: "1px solid var(--quiet)" },
});

export interface EditorOptions {
  parent: HTMLElement;
  doc: string;
  onChange(doc: string): void;
  onRun(): void;
}

export function createEditor({ parent, doc, onChange, onRun }: EditorOptions): EditorView {
  return new EditorView({
    parent,
    state: EditorState.create({
      doc,
      extensions: [
        history(),
        drawSelection(),
        bracketMatching(),
        EditorState.tabSize.of(4),
        indentUnit.of("    "),
        keymap.of([{ key: "Mod-Enter", run: () => (onRun(), true) }, ...defaultKeymap, ...historyKeymap, indentWithTab]),
        rust,
        syntaxHighlighting(highlighter),
        refinement,
        look,
        EditorView.contentAttributes.of({ "aria-label": "Code", autocapitalize: "off", autocorrect: "off", spellcheck: "false" }),
        EditorView.updateListener.of((update) => {
          if (update.docChanged) onChange(update.state.doc.toString());
        }),
      ],
    }),
  });
}

/** Replaces the whole text, as picking an example does. */
export function setDoc(view: EditorView, doc: string): void {
  if (view.state.doc.toString() === doc) return;
  view.dispatch({ changes: { from: 0, to: view.state.doc.length, insert: doc } });
}
