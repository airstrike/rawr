// The playground's island: the controls, the editor and the output, drawn
// from a Session (session.ts). The page is static; the Worker writes each
// request's data (the share and the examples) into #play-data. A snippet
// runs in the browser only, so the controls have one Run button.
// Before hydration the server-rendered controls show; the editor mounts
// once the island runs. The debugger icon runs the program under the
// debugger build of the universe, opens its panel right of the output and
// its console under the code (debugger.ts, D1071). A browser without JSPI
// can't pause a program, so there the icon is greyed and says why. On a wide screen,
// split bars between the panes and over the console set their sizes,
// the viewer's own (panes.tsx).
import type { EditorView } from "@codemirror/view";
import { Fragment, useEffect, useRef, useState, useSyncExternalStore, type CSSProperties } from "react";
import { Button } from "../components/ui/button.tsx";
import { Select, SelectContent, SelectGroup, SelectItem, SelectLabel, SelectSeparator, SelectTrigger, SelectValue } from "../components/ui/select.tsx";
import { onTheme, theme as pageTheme, type Theme } from "../lib/theme.ts";
import { DEBUGGER, universe } from "../scripts/universe.ts";
import { canPause, Debugger, ROOT, type DebugState } from "./debugger.ts";
import { createConsole, createEditor, setDebugging, setDoc, setMarks } from "./editor.ts";
import { clamp, PartsOf, SplitBar, Twisty, useOpen, useStoredSize, ValueTable, type Row } from "./panes.tsx";
import { Session, WEB_MODULE, type Example, type Output, type PlayData, type State, type Web } from "./session.ts";

/** The page's data, or a bare page's when the Worker wrote none (a static preview). */
function readData(): PlayData {
  try {
    const data = JSON.parse(document.getElementById("play-data")?.textContent ?? "null");
    if (data) return data;
  } catch {}
  return { snippet: { code: "" }, examples: [], shared: null };
}

/** The copies of rawr-web loaded so far. */
let copies = 0;

function browserDeps() {
  return {
    fetch: window.fetch.bind(window),
    location: window.location,
    replaceState: (url: string) => history.replaceState(null, "", url),
    storage: (() => {
      try {
        return window.localStorage;
      } catch {
        return null;
      }
    })(),
    loadWeb: async () => {
      // A module is imported once per address, so a copy after the first
      // has its own query: its own instance, from the same cached files.
      const m = await import(/* @vite-ignore */ WEB_MODULE + (copies++ ? "?" + copies : ""));
      await m.default();
      return m as Web;
    },
    // The browser's prebuilt runner, for snippets that use crates.
    universe: universe({
      document,
      fetch: window.fetch.bind(window),
      compile: (response) => WebAssembly.compileStreaming(response),
    }),
    // Its debugger build, fetched the first time the debugger is turned on.
    debugging: {
      universe: universe({
        document,
        fetch: window.fetch.bind(window),
        compile: (response) => WebAssembly.compileStreaming(response),
        build: DEBUGGER,
      }),
      debugger: new Debugger(),
      pausable: canPause(),
    },
    now: () => performance.now(),
    clipboard: (text: string) => navigator.clipboard.writeText(text),
    setTimeout: (f: () => void, ms: number) => window.setTimeout(f, ms),
    clearTimeout: (id: unknown) => window.clearTimeout(id as number),
  };
}

/**
 * The wide layout's least sizes, in CSS pixels: a drag of a split bar stops
 * short of them, as elevato's divider stops short of its cards'.
 */
const CODE_MIN = 260;
const OUTPUT_MIN = 320;
const DEBUGGER_MIN = 220;
const EDITOR_MIN = 120;
const CONSOLE_MIN = 64;
/** The line between two columns, which its split bar straddles. */
const LINE = 1;

const NONE: State = {
  code: "", theme: "dark", output: { kind: "note" }, status: "ready", mark: "", debugging: false,
};

export default function Playground() {
  const [session, setSession] = useState<Session | null>(null);
  const state = useSyncExternalStore(
    (listener) => session?.subscribe(listener) ?? (() => {}),
    () => session?.state ?? NONE,
    () => NONE,
  );
  const host = useRef<HTMLDivElement>(null);
  const frames = useRef<HTMLDivElement>(null);
  const editor = useRef<EditorView | null>(null);
  const debug = session?.deps.debugging?.debugger;
  const debugState = useSyncExternalStore(
    (listener) => debug?.subscribe(listener) ?? (() => {}),
    () => debug?.state ?? null,
    () => null,
  );
  const main = useRef<HTMLElement>(null);
  const debugging = state.debugging && !!debug && !!debugState;
  // The split bars' sizes, the viewer's own: the code's share of the width
  // it splits with the output, and the debugger's width in pixels.
  const [codeShare, setCodeShare, keepCodeShare] = useStoredSize("rawr.play.split.code", 0.5);
  const [debuggerWidth, setDebuggerWidth, keepDebuggerWidth] = useStoredSize("rawr.play.split.debugger", 288);
  const debuggerColumn = debugging ? debuggerWidth + LINE : 0;
  const moveCode = (x: number) => {
    const box = main.current!.getBoundingClientRect();
    const shared = box.width - LINE - debuggerColumn;
    setCodeShare(clamp(x - box.left, CODE_MIN, shared - OUTPUT_MIN) / shared);
  };
  const codeLine = () => {
    const box = main.current!.getBoundingClientRect();
    return box.left + codeShare * (box.width - LINE - debuggerColumn);
  };
  const moveDebugger = (x: number) => {
    const box = main.current!.getBoundingClientRect();
    setDebuggerWidth(clamp(box.right - x, DEBUGGER_MIN, box.width - 2 * LINE - CODE_MIN - OUTPUT_MIN));
  };
  const debuggerLine = () => main.current!.getBoundingClientRect().right - debuggerWidth;
  // The columns on a wide screen; a phone stacks the panes and ignores them.
  const columns = `minmax(0, ${codeShare}fr) ${LINE}px minmax(0, ${1 - codeShare}fr)`
    + (debugging ? ` ${LINE}px min(${debuggerWidth}px, max(${DEBUGGER_MIN}px, calc(100% - ${CODE_MIN + OUTPUT_MIN + 2 * LINE}px)))` : "");

  // After hydration: the session from the page's data, the editor, the theme and the keys.
  useEffect(() => {
    const data = readData();
    const s = new Session(data, { ...browserDeps(), frameHost: () => frames.current }, pageTheme());
    setSession(s);
    const view = createEditor({ parent: host.current!, doc: s.state.code, onChange: (code) => s.setCode(code), onRun: () => void s.go() });
    editor.current = view;
    // The view on its element, for the browser test and a console.
    Object.defineProperty(host.current!, "editor", { value: view, configurable: true });
    const stop = onTheme((theme: Theme) => s.setTheme(theme));
    const keys = (event: KeyboardEvent) => {
      if ((event.ctrlKey || event.metaKey) && event.key === "Enter") {
        event.preventDefault();
        void s.go();
      }
    };
    document.addEventListener("keydown", keys);
    const preload = () => setTimeout(() => s.preload(), 0);
    if (document.readyState === "complete") preload();
    else window.addEventListener("load", preload, { once: true });
    return () => {
      stop();
      document.removeEventListener("keydown", keys);
      view.destroy();
    };
  }, []);

  // A new example's code goes into the editor. (Before the session exists,
  // the state is the server render's, with no code.)
  useEffect(() => {
    if (editor.current && session) setDoc(editor.current, state.code);
  }, [session, state.code]);

  // While the debugger is on, the editor's line numbers set breakpoints.
  useEffect(() => {
    if (editor.current && debug) setDebugging(editor.current, state.debugging ? (line) => void debug.toggle(line) : null);
  }, [debug, state.debugging]);
  useEffect(() => {
    if (!editor.current || !debugState) return;
    const paused = debugState.paused?.file === ROOT ? debugState.paused.line : null;
    setMarks(editor.current, debugState.lines, paused);
  }, [debugState]);

  // The debugger's keys, as DevTools has them: F8 continues or pauses,
  // F10 steps over, F11 into and Shift+F11 out. The editor binds none.
  useEffect(() => {
    if (!debug || !state.debugging) return;
    const keys = (event: KeyboardEvent) => {
      const action = ACTIONS.find((action) => action.key === event.key && action.shift === event.shiftKey && action.enabled(debug.state));
      if (!action) return;
      event.preventDefault();
      action.run(debug);
    };
    document.addEventListener("keydown", keys);
    return () => document.removeEventListener("keydown", keys);
  }, [debug, state.debugging]);

  const examples = session?.data.examples ?? [];
  const groups: [string, Example[]][] = [];
  for (const example of examples) {
    const group = example.group ?? "";
    if (groups.at(-1)?.[0] !== group) groups.push([group, []]);
    groups.at(-1)![1].push(example);
  }
  const current = examples.find((e) => e.slug === state.mark);

  return (
    <div className="flex min-h-0 flex-1 flex-col">
      <div className="tools flex flex-wrap items-center gap-2 border-b border-grid px-4 py-3 md:px-6">
        <Select value={state.mark || ""} onValueChange={(value) => session?.pick(value)}>
          <SelectTrigger className="flex-1 md:w-[240px] md:flex-none" aria-label="Examples">
            {current ? <span className="normal-case tracking-normal font-sans text-[13px]">{current.name}</span> : <span className="uppercase">Examples</span>}
          </SelectTrigger>
          <SelectContent align="start">
            {groups.map(([group, items], i) => (
              <Fragment key={group || i}>
                {i > 0 && <SelectSeparator />}
                <SelectGroup>
                  {group && <SelectLabel>{group}</SelectLabel>}
                  {items.map((e) => (
                    <SelectItem key={e.slug} value={e.slug}>
                      {e.name}
                    </SelectItem>
                  ))}
                </SelectGroup>
              </Fragment>
            ))}
            {examples.length > 0 && (
              <>
                <SelectSeparator />
                <SelectItem value="reset">Reset</SelectItem>
              </>
            )}
          </SelectContent>
        </Select>
        <span className="order-3 hidden grow md:block" />
        <DebugIcon session={session} on={state.debugging} />
        <Button id="share" className="order-3" onClick={() => void session?.share()}>Share</Button>
        <Button id="run" className="order-3" variant="primary" title="Ctrl/Cmd + Enter" onClick={() => void session?.go()}>
          Run
        </Button>
      </div>
      <main ref={main} className="playmain flex min-h-0 flex-1 flex-col md:grid" style={{ "--columns": columns } as CSSProperties}>
        <section className="pane flex min-h-0 flex-col px-4 pt-4 pb-6 md:px-6">
          <p className="playcap mb-2"><span className="text-red">//</span> Code</p>
          <div ref={host} className={"editor min-h-[300px] flex-1 " + (debugging ? "md:min-h-[120px]" : "md:min-h-[360px]")} />
          {debugging && <DebugConsole debug={debug!} state={debugState!} />}
        </section>
        <SplitBar between="columns" label="Resize the code and the output" onMove={moveCode} onEnd={keepCodeShare} at={codeLine} />
        <section className="pane flex min-h-0 flex-col border-t border-grid px-4 pt-4 pb-6 md:border-t-0 md:px-6">
          <p className="playcap mb-2 flex justify-between gap-2">
            <span className="shrink-0"><span className="text-red">//</span> Output</span>
            <span id="status">{state.status}</span>
          </p>
          <div id="out" className={state.output.kind === "frame" ? "out framed" : "out"} aria-live="polite">
            {/* The universe's frames (session.ts), which the session adds and removes itself. */}
            <div ref={frames} className="frames" hidden={state.output.kind !== "frame"} />
            <OutputView output={state.output} />
          </div>
        </section>
        {debugging && <SplitBar between="columns" label="Resize the debugger" onMove={moveDebugger} onEnd={keepDebuggerWidth} at={debuggerLine} />}
        {debugging && <DebugPanel debug={debug!} state={debugState!} />}
      </main>
      <footer className="playfoot flex flex-wrap justify-between gap-4 border-t border-rule px-4 py-3 md:px-6">
        <nav aria-label="Playground" className="flex flex-wrap gap-4">
          <a href="/docs/play">Help</a>
          <a href="/bot">Status</a>
          <a href="/bot/invite">Add to Discord</a>
        </nav>
        <span>MIT or Apache-2.0 · <a href="https://airstrike.fyi">airstrike.fyi</a></span>
      </footer>
    </div>
  );
}

/** Why the debugger icon is off in a browser that can't pause a program. */
const NO_PAUSE = "The debugger needs WebAssembly JSPI, which this browser doesn't support.";

/**
 * The debugger icon, which turns the debugger on and off. In a browser
 * that can't pause a program it is greyed and `aria-disabled`, and a tap
 * or a click shows why under it, until the next tap or click elsewhere or
 * Escape.
 */
function DebugIcon({ session, on }: { session: Session | null; on: boolean }) {
  const pausable = session?.deps.debugging?.pausable ?? false;
  const [why, setWhy] = useState(false);
  const self = useRef<HTMLSpanElement>(null);
  useEffect(() => {
    if (!why) return;
    const away = (event: PointerEvent) => {
      if (!self.current?.contains(event.target as Node)) setWhy(false);
    };
    const escape = (event: KeyboardEvent) => {
      if (event.key === "Escape") setWhy(false);
    };
    document.addEventListener("pointerdown", away);
    document.addEventListener("keydown", escape);
    return () => {
      document.removeEventListener("pointerdown", away);
      document.removeEventListener("keydown", escape);
    };
  }, [why]);
  return (
    <span ref={self} className="relative order-3 flex" hidden={!session?.deps.debugging}>
      <Button
        id="debug"
        className="px-2"
        variant={on ? "primary" : "default"}
        aria-pressed={pausable ? on : undefined}
        aria-label="Debugger"
        aria-disabled={pausable ? undefined : true}
        aria-describedby={pausable ? undefined : "debug-why"}
        aria-expanded={pausable ? undefined : why}
        title={pausable ? (on ? "Turn the debugger off" : "Run under the debugger") : undefined}
        onClick={() => (pausable ? void session?.debug() : setWhy(!why))}
      >
        <BugIcon />
      </Button>
      {!pausable && (
        <span id="debug-why" role="tooltip" className="debug-why" hidden={!why}>
          {NO_PAUSE}
        </span>
      )}
    </span>
  );
}

/** The debugger icon: a small bug, drawn in the button's colour. */
function BugIcon() {
  return (
    <svg width="16" height="16" viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="square" aria-hidden="true">
      <rect x="4.75" y="5.25" width="6.5" height="8" rx="3.25" />
      <path d="M6 5.25a2 2 0 0 1 4 0M8 8v5M1.5 9h3.25M11.25 9h3.25M2.5 5l2.5 1.5M13.5 5L11 6.5M2.5 13.5l2.5-1.5M13.5 13.5L11 12" />
    </svg>
  );
}

/** What the program is doing, in the panel's words. */
function phaseText(state: DebugState): string {
  switch (state.phase) {
    case "idle":
      return "starting";
    case "held":
      return "held at its start";
    case "running":
      return "running";
    case "paused":
      return `paused · ${state.paused?.reason ?? ""}`;
    case "ended":
      return `${state.status === 101 ? "panicked" : "main returned"} · exit ${state.status}`;
  }
}

/** A frame's function by its last path segment. */
function shortName(name: string): string {
  return name.split("::").at(-1) ?? name;
}

/** One of the debugger toolbar's actions, with its key and its glyph. */
interface Action {
  id: string;
  label: string;
  /** Its key's name, as `KeyboardEvent.key` gives it, and whether Shift is held. */
  key: string;
  shift: boolean;
  shortcut: string;
  enabled(state: DebugState): boolean;
  run(debug: Debugger): void;
  /** The glyph's SVG paths, on a 16 px grid: filled, then stroked. */
  fill?: string;
  stroke?: string;
}

const isPaused = (state: DebugState) => state.phase === "paused";

const ACTIONS: Action[] = [
  {
    id: "continue", label: "Continue", key: "F8", shift: false, shortcut: "F8", enabled: isPaused, run: (debug) => void debug.continue(),
    fill: "M7 3l7 5-7 5z", stroke: "M3.5 3v10",
  },
  {
    id: "pause", label: "Pause", key: "F8", shift: false, shortcut: "F8",
    enabled: (state) => state.pause === true && state.phase === "running", run: (debug) => void debug.pause(),
    fill: "M3.5 3h3v10h-3zM9.5 3h3v10h-3z",
  },
  {
    id: "over", label: "Step over", key: "F10", shift: false, shortcut: "F10", enabled: isPaused, run: (debug) => void debug.step("over"),
    fill: "M8 11.5a1.75 1.75 0 1 1 0 3.5a1.75 1.75 0 1 1 0-3.5z", stroke: "M2.5 9a5.5 5 0 0 1 11 0M13.5 9l-3-1.25M13.5 9l1-3",
  },
  {
    id: "into", label: "Step into", key: "F11", shift: false, shortcut: "F11", enabled: isPaused, run: (debug) => void debug.step("into"),
    fill: "M8 11.5a1.75 1.75 0 1 1 0 3.5a1.75 1.75 0 1 1 0-3.5z", stroke: "M8 1.5v7M5 5.5l3 3 3-3",
  },
  {
    id: "out", label: "Step out", key: "F11", shift: true, shortcut: "Shift+F11", enabled: isPaused, run: (debug) => void debug.step("out"),
    fill: "M8 11.5a1.75 1.75 0 1 1 0 3.5a1.75 1.75 0 1 1 0-3.5z", stroke: "M8 9V2M5 5l3-3 3 3",
  },
];

/** An action's glyph, in the button's colour. */
function ActionIcon({ action }: { action: Action }) {
  return (
    <svg width="16" height="16" viewBox="0 0 16 16" aria-hidden="true">
      {action.fill && <path d={action.fill} fill="currentColor" />}
      {action.stroke && <path d={action.stroke} fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="square" />}
    </svg>
  );
}

/**
 * The debugger's console, under the code: what was evaluated and its
 * answers as compact rows, with the `>` prompt right after the last, where
 * Enter evaluates and Shift+Enter adds a line. An answer with parts opens
 * to them. Its caption folds it; on a wide screen the bar over it sets its
 * height.
 */
function DebugConsole({ debug, state }: { debug: Debugger; state: DebugState }) {
  const [folded, setFolded] = useState(false);
  const [height, setHeight, keepHeight] = useStoredSize("rawr.play.split.console", 200);
  const [open, toggle] = useOpen();
  const self = useRef<HTMLElement>(null);
  const prompt = useRef<HTMLDivElement>(null);
  const body = useRef<HTMLDivElement>(null);
  useEffect(() => {
    if (folded || !prompt.current) return;
    const view = createConsole({ parent: prompt.current, onSubmit: (code) => void debug.evaluate(code) });
    return () => view.destroy();
  }, [debug, folded]);
  // The latest answer shows, as a console scrolls to it.
  useEffect(() => {
    body.current?.scrollTo(0, body.current.scrollHeight);
  }, [state.evaluated]);
  // The bar over the console moves its top; the editor above keeps its least height.
  const move = (y: number) => {
    const box = self.current!.getBoundingClientRect();
    const editor = self.current!.parentElement!.querySelector(".editor")!.getBoundingClientRect();
    setHeight(clamp(box.bottom - y, CONSOLE_MIN, box.bottom - editor.top - EDITOR_MIN));
  };
  return (
    <>
      {!folded && <SplitBar between="rows" label="Resize the console" onMove={move} onEnd={keepHeight} at={() => self.current!.getBoundingClientRect().top} />}
      <section
        ref={self}
        id="console"
        className={"console" + (folded ? " folded" : "")}
        aria-label="Console"
        style={{ "--console-height": `${height}px` } as CSSProperties}
      >
        <button type="button" className="playcap fold" aria-expanded={!folded} onClick={() => setFolded(!folded)}>
          <span className="text-red">//</span> Console
        </button>
        {!folded && (
          <div ref={body} className="pane-body">
            <ol className="scrollback">
              {state.evaluated.map((evaluated, at) => {
                const row: Row = { path: String(at), depth: 0, name: "", value: evaluated.value, children: evaluated.children };
                const parts = evaluated.children?.length ? evaluated.children : null;
                return (
                  <li key={at} className="entry">
                    <pre className="in">{evaluated.code}</pre>
                    <div className={parts ? "out-line opens" : "out-line"} onClick={parts ? () => toggle(row.path) : undefined}>
                      {parts && <Twisty row={row} open={open} toggle={toggle} />}
                      <pre
                        className={(evaluated.error !== undefined ? "answer err" : "answer") + (parts ? " elided" : "")}
                        title={parts ? evaluated.value : undefined}
                      >
                        {evaluated.error ?? evaluated.value}
                      </pre>
                    </div>
                    {parts && open.has(row.path) && <PartsOf path={row.path} parts={parts} open={open} toggle={toggle} />}
                  </li>
                );
              })}
            </ol>
            <div className="prompt">
              <span className="mark" aria-hidden="true">&gt;</span>
              <div ref={prompt} className="input" />
            </div>
          </div>
        )}
      </section>
    </>
  );
}

/**
 * The debugger's panel, right of the output, dense as DevTools' sources
 * sidebar: its toolbar beside where the program is, then the frames and
 * the bindings as compact tables under section bars. A binding with parts
 * opens to them, and stays open across pauses.
 */
function DebugPanel({ debug, state }: { debug: Debugger; state: DebugState }) {
  const [open, toggle] = useOpen();
  const paused = state.phase === "paused";
  const canPause = state.pause === true;
  const rows: Row[] = state.bindings.map((binding) => ({
    path: binding.name, depth: 0, name: binding.name, value: binding.value, error: binding.error, children: binding.children,
  }));
  return (
    <section id="debugger" className="pane debugger flex min-h-0 flex-col border-t border-grid pt-4 pb-6 md:border-t-0" aria-label="Debugger">
      {/* The caption alone, as the code's and the output's. */}
      <p className="playcap mb-1 whitespace-nowrap px-3"><span className="text-red">//</span> Debugger</p>
      <div className="bar flex items-center gap-2 px-2">
        <div className="actions flex" role="toolbar" aria-label="Debugger controls">
          {ACTIONS.map((action) => (
            <button
              key={action.id}
              type="button"
              data-control={action.id}
              aria-label={action.label}
              title={`${action.label} (${action.shortcut})`}
              disabled={!action.enabled(state)}
              onClick={() => action.run(debug)}
            >
              <ActionIcon action={action} />
            </button>
          ))}
        </div>
        <p className="phase truncate" title={phaseText(state)}>{phaseText(state)}</p>
      </div>
      <div className="notes px-3">
        {state.pause === false && (
          <p className="note">This browser can't pause a program: it has no WebAssembly JS Promise Integration (JSPI). Breakpoints and steps need it.</p>
        )}
        {state.phase === "ended" && <p className="note">The program's <code>main</code> has ended. Run it again to debug it.</p>}
        {canPause && state.phase !== "ended" && state.lines.length === 0 && !paused && <p className="note">Click a line number to set a breakpoint.</p>}
        {state.error && <p className="error">{state.error}</p>}
      </div>
      {paused && state.paused && (
        <div className="sections min-h-0 flex-1 overflow-auto">
          <p className="where px-3" title={state.paused.function}>{shortName(state.paused.function)} · line {state.paused.line}</p>
          <h3>Call stack</h3>
          <ol className="frames-list">
            {state.frames.map((frame, at) => (
              <li key={at}>
                <button type="button" aria-current={at === state.frame} title={frame.function} onClick={() => void debug.select(at)}>
                  <span className="fn">{shortName(frame.function)}</span>
                  {frame.line ? <span className="line">:{frame.line}</span> : null}
                </button>
              </li>
            ))}
          </ol>
          <h3>Bindings</h3>
          {state.unread && <p className="note px-3">{state.unread}</p>}
          <ValueTable rows={rows} open={open} toggle={toggle} className="bindings" />
        </div>
      )}
    </section>
  );
}

function OutputView({ output }: { output: Output }) {
  switch (output.kind) {
    case "note":
      return (
        <p className="note">
          Run (Ctrl/Cmd + Enter). A Rust program shows its output.
        </p>
      );
    case "error":
      return <pre className="error">{output.text}</pre>;
    case "frame": {
      // The program draws in its frame; under it, what it printed or how it
      // failed, and an edit's compile error while it runs its last good code.
      const printed = output.chunks.some((chunk) => chunk.text !== "") || output.status !== 0;
      const error = output.error !== undefined && <pre className="error">{output.error}</pre>;
      const term = printed && (
        <pre className="term">
          {output.chunks.map((chunk, i) => (
            <span key={i} className={chunk.stream === "stderr" ? "err" : undefined}>{chunk.text}</span>
          ))}
          {output.status !== 0 && <span className="exit">exit status {output.status}{"\n"}</span>}
        </pre>
      );
      if (!printed && !error) return null;
      return <>{error}{term}</>;
    }
    case "term": {
      const empty = !output.chunks.some((chunk) => chunk.text !== "");
      return (
        <pre className="term">
          {output.chunks.map((chunk, i) => (
            <span key={i} className={chunk.stream === "stderr" ? "err" : undefined}>{chunk.text}</span>
          ))}
          {empty && <span className="none">(no output){"\n"}</span>}
          {output.status !== 0 && <span className="exit">exit status {output.status}{"\n"}</span>}
          {output.note && <span className="note">{"\n" + output.note + "\n"}</span>}
        </pre>
      );
    }
  }
}
