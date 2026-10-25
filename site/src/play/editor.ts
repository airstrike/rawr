// The playground's editor: CodeMirror 6 with Rust, highlighted in the site's
// syntax classes (lib/highlight.ts), so its colours are the docs' in either
// theme. Its look comes from the theme's CSS variables, so a change of theme
// needs nothing here.
import { defaultKeymap, history, historyKeymap, indentWithTab } from "@codemirror/commands";
import { rust } from "@codemirror/lang-rust";
import { bracketMatching, indentUnit, syntaxHighlighting } from "@codemirror/language";
import { EditorState } from "@codemirror/state";
import { EditorView, drawSelection, keymap } from "@codemirror/view";
import { highlighter } from "../lib/highlight.ts";

const look = EditorView.theme({
  "&": { height: "100%", backgroundColor: "var(--inset)", color: "var(--code)", border: "1px solid var(--line)", fontSize: "13px" },
  "&.cm-focused": { outline: "none", borderColor: "var(--dim)" },
  ".cm-scroller": { fontFamily: "var(--mono)", lineHeight: "1.55", overflow: "auto" },
  ".cm-content": { padding: "12px 0", caretColor: "var(--pink)" },
  ".cm-line": { padding: "0 12px" },
  ".cm-cursor, .cm-dropCursor": { borderLeftColor: "var(--pink)", borderLeftWidth: "2px" },
  "&.cm-focused > .cm-scroller > .cm-selectionLayer .cm-selectionBackground, .cm-selectionBackground, .cm-content ::selection": {
    backgroundColor: "color-mix(in srgb, var(--pink) 28%, transparent)",
  },
  ".cm-matchingBracket, &.cm-focused .cm-matchingBracket": { backgroundColor: "transparent", outline: "1px solid var(--dim)" },
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
        rust(),
        syntaxHighlighting(highlighter),
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
