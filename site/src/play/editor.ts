// The playground's editor: CodeMirror 6 with Rust, highlighted in the site's
// syntax classes (lib/highlight.ts), so its colours are the docs' in either
// theme. Its look comes from the theme's CSS variables, so a change of theme
// needs nothing here.
//
// While the debugger is on (setDebugging), the editor shows line numbers: a
// click on one sets or clears a breakpoint there, each breakpoint's number
// is marked, and the line the program is paused at is highlighted. The
// debugger's console prompt (createConsole) is a small editor of the same
// Rust.
import { defaultKeymap, history, historyKeymap, indentWithTab, insertNewlineAndIndent } from "@codemirror/commands";
import { rustLanguage } from "@codemirror/lang-rust";
import { LanguageSupport, bracketMatching, indentUnit, syntaxHighlighting, syntaxTree } from "@codemirror/language";
import { Compartment, EditorState, RangeSet, RangeSetBuilder, StateEffect, StateField } from "@codemirror/state";
import { Decoration, type DecorationSet, EditorView, GutterMarker, ViewPlugin, type ViewUpdate, drawSelection, gutterLineClass, keymap, lineNumbers } from "@codemirror/view";
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
  ".cm-gutters": { backgroundColor: "var(--wash)", color: "var(--quiet)", borderRight: "1px solid var(--grid)" },
  ".cm-lineNumbers .cm-gutterElement": { padding: "0 6px 0 14px", cursor: "pointer", position: "relative" },
  ".cm-lineNumbers .cm-gutterElement.cm-breakpoint": { color: "var(--ink)" },
  ".cm-lineNumbers .cm-gutterElement.cm-breakpoint::before": {
    content: '""', position: "absolute", left: "3px", top: "50%", width: "7px", height: "7px", marginTop: "-3.5px", borderRadius: "50%", backgroundColor: "var(--red)",
  },
  ".cm-paused": { backgroundColor: "color-mix(in srgb, var(--red) 16%, transparent)" },
});

/** The debugger's lines: its breakpoints, and where the program is paused. */
interface Marks {
  breakpoints: number[];
  paused: number | null;
}

const marked = StateEffect.define<Marks>();
const marksField = StateField.define<Marks>({
  create: () => ({ breakpoints: [], paused: null }),
  update: (marks, transaction) => transaction.effects.reduce((marks, effect) => (effect.is(marked) ? effect.value : marks), marks),
});

class Breakpoint extends GutterMarker {
  elementClass = "cm-breakpoint";
}
const breakpoint = new Breakpoint();
const pausedLine = Decoration.line({ class: "cm-paused" });

/** A line's start, or null past the document's end. */
function lineStart(state: EditorState, line: number): number | null {
  return line >= 1 && line <= state.doc.lines ? state.doc.line(line).from : null;
}

/** The debugger's gutter and highlight, which call `toggle` with a clicked line's number. */
function debugging(toggle: (line: number) => void) {
  return [
    lineNumbers({
      domEventHandlers: {
        mousedown: (view, block) => {
          toggle(view.state.doc.lineAt(block.from).number);
          return true;
        },
      },
    }),
    gutterLineClass.compute([marksField], (state) => {
      const starts = state.field(marksField).breakpoints.map((line) => lineStart(state, line)).filter((at) => at !== null);
      return RangeSet.of([...new Set(starts)].sort((a, b) => a - b).map((at) => breakpoint.range(at)));
    }),
    EditorView.decorations.compute([marksField], (state) => {
      const at = lineStart(state, state.field(marksField).paused ?? 0);
      return at === null ? Decoration.none : Decoration.set([pausedLine.range(at)]);
    }),
  ];
}

const debugCompartment = new Compartment();

/** Turn the debugger's gutter on, its clicks going to `toggle`, or off with null. */
export function setDebugging(view: EditorView, toggle: ((line: number) => void) | null): void {
  view.dispatch({ effects: debugCompartment.reconfigure(toggle ? debugging(toggle) : []) });
}

/** Mark `breakpoints` and the line the program is `paused` at. */
export function setMarks(view: EditorView, breakpoints: number[], paused: number | null): void {
  view.dispatch({ effects: marked.of({ breakpoints, paused }) });
}

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
        marksField,
        debugCompartment.of([]),
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

const consoleLook = EditorView.theme({
  "&": { backgroundColor: "transparent", color: "var(--ink)", fontSize: "12px" },
  "&.cm-focused": { outline: "none" },
  ".cm-scroller": { fontFamily: "var(--mono)", lineHeight: "1.4" },
  ".cm-content": { padding: "0", caretColor: "var(--red)" },
  ".cm-line": { padding: "0" },
  ".cm-cursor, .cm-dropCursor": { borderLeftColor: "var(--red)", borderLeftWidth: "2px" },
  "&.cm-focused > .cm-scroller > .cm-selectionLayer .cm-selectionBackground, .cm-selectionBackground, .cm-content ::selection": {
    backgroundColor: "color-mix(in srgb, var(--red) 28%, transparent)",
  },
});

export interface ConsoleOptions {
  parent: HTMLElement;
  /** Takes the prompt's text when Enter submits it; the prompt then empties. */
  onSubmit(code: string): void;
}

/**
 * The debugger console's prompt: the editor's Rust and highlighting, one
 * line that grows as lines are added. Enter submits it and Shift+Enter
 * adds a line.
 */
export function createConsole({ parent, onSubmit }: ConsoleOptions): EditorView {
  const submit = (view: EditorView) => {
    const code = view.state.doc.toString();
    if (!code.trim()) return true;
    onSubmit(code);
    view.dispatch({ changes: { from: 0, to: view.state.doc.length, insert: "" } });
    return true;
  };
  return new EditorView({
    parent,
    state: EditorState.create({
      extensions: [
        history(),
        drawSelection(),
        bracketMatching(),
        EditorState.tabSize.of(4),
        indentUnit.of("    "),
        keymap.of([{ key: "Enter", run: submit, shift: insertNewlineAndIndent }, ...defaultKeymap, ...historyKeymap]),
        rust,
        syntaxHighlighting(highlighter),
        refinement,
        consoleLook,
        EditorView.contentAttributes.of({ "aria-label": "Console", autocapitalize: "off", autocorrect: "off", spellcheck: "false" }),
      ],
    }),
  });
}
