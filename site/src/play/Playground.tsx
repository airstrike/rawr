// The playground's island: the controls, the editor and the output, drawn
// from a Session (session.ts). The page is static; the Worker writes each
// request's data (the share, the examples, Turnstile's key) into #play-data.
// Before hydration the server-rendered controls show; the editor mounts
// once the island runs. The debugger icon runs the program under the
// debugger build of the universe and opens its panel beside the code
// (debugger.ts, D1071).
import type { EditorView } from "@codemirror/view";
import { Fragment, useEffect, useRef, useState, useSyncExternalStore } from "react";
import { Button } from "../components/ui/button.tsx";
import { Input } from "../components/ui/input.tsx";
import { Select, SelectContent, SelectGroup, SelectItem, SelectLabel, SelectSeparator, SelectTrigger, SelectValue } from "../components/ui/select.tsx";
import { ToggleGroup, ToggleGroupItem } from "../components/ui/toggle-group.tsx";
import { onTheme, theme as pageTheme, type Theme } from "../lib/theme.ts";
import { DEBUGGER, universe } from "../scripts/universe.ts";
import { Debugger, ROOT, type DebugState } from "./debugger.ts";
import { createEditor, setDebugging, setDoc, setMarks } from "./editor.ts";
import { Session, TURNSTILE_JS, WEB_MODULE, type Example, type Output, type PlayData, type State, type Web, type Where } from "./session.ts";

/** The page's data, or a bare page's when the Worker wrote none (a static preview). */
function readData(): PlayData {
  try {
    const data = JSON.parse(document.getElementById("play-data")?.textContent ?? "null");
    if (data) return data;
  } catch {}
  return { snippet: { code: "" }, examples: [], sitekey: null, pass: false, shared: null };
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
    // The browser's universe, for snippets that use crates.
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
    },
    now: () => performance.now(),
    clipboard: (text: string) => navigator.clipboard.writeText(text),
    turnstile: () => (window as Window & { turnstile?: any }).turnstile,
    hasContainer: () => document.getElementById("ts") !== null,
    loadTurnstile: () => {
      if (document.querySelector(`script[data-turnstile], script[src^="${TURNSTILE_JS.split("?")[0]}"]`)) return;
      const js = document.createElement("script");
      js.src = TURNSTILE_JS;
      js.async = true;
      js.dataset.turnstile = "";
      document.head.append(js);
    },
    setTimeout: (f: () => void, ms: number) => window.setTimeout(f, ms),
    clearTimeout: (id: unknown) => window.clearTimeout(id as number),
  };
}

const NONE: State = {
  code: "", theme: "dark", where: "browser", output: { kind: "note" }, status: "ready", busy: false, mark: "", debugging: false,
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
        <ToggleGroup
          type="single"
          className="order-2 w-full md:order-3 md:w-auto"
          aria-label="Where it runs"
          value={state.where}
          onValueChange={(value) => value && session?.choose(value as Where)}
        >
          <ToggleGroupItem className="flex-1 md:flex-none" value="server">Run on server</ToggleGroupItem>
          <ToggleGroupItem className="flex-1 md:flex-none" value="browser">Run in browser</ToggleGroupItem>
        </ToggleGroup>
        <Button
          id="debug"
          className="order-3 px-2"
          variant={state.debugging ? "primary" : "default"}
          aria-pressed={state.debugging}
          aria-label="Debugger"
          title={state.debugging ? "Turn the debugger off" : "Run under the debugger"}
          hidden={!session?.deps.debugging}
          onClick={() => void session?.debug()}
        >
          <BugIcon />
        </Button>
        <Button id="share" className="order-3" onClick={() => void session?.share()}>Share</Button>
        <Button id="run" className="order-3" variant="primary" title="Ctrl/Cmd + Enter" disabled={state.busy} onClick={() => void session?.go()}>
          Run
        </Button>
      </div>
      <main className={"flex min-h-0 flex-1 flex-col md:grid " + (state.debugging ? "md:grid-cols-[minmax(0,1fr)_17rem_minmax(0,1fr)]" : "md:grid-cols-2")}>
        <section className="pane flex min-h-0 flex-col px-4 pt-4 pb-6 md:px-6">
          <p className="playcap mb-2"><span className="text-red">//</span> Code</p>
          <div ref={host} className="editor min-h-[300px] flex-1 md:min-h-[360px]" />
          {/* Always present, so Turnstile finds its container whenever the session mounts it. */}
          <div id="ts" className="mt-3 empty:hidden" />
        </section>
        {state.debugging && debug && debugState && <DebugPanel debug={debug} state={debugState} />}
        <section className="pane flex min-h-0 flex-col border-t border-grid px-4 pt-4 pb-6 md:border-t-0 md:border-l md:px-6">
          <p className="playcap mb-2 flex justify-between gap-2">
            <span><span className="text-red">//</span> Output</span>
            <span id="status" className="truncate">{state.status}</span>
          </p>
          <div id="out" className={state.output.kind === "frame" ? "out framed" : "out"} aria-live="polite">
            {/* The universe's frames (session.ts), which the session adds and removes itself. */}
            <div ref={frames} className="frames" hidden={state.output.kind !== "frame"} />
            <OutputView output={state.output} />
          </div>
        </section>
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

/** The debugger's panel, beside the code: its controls, the frames, the bindings and an eval box. */
function DebugPanel({ debug, state }: { debug: Debugger; state: DebugState }) {
  const [code, setCode] = useState("");
  const paused = state.phase === "paused";
  const canPause = state.pause === true;
  const controls: [string, string, () => void, boolean][] = [
    ["continue", "Continue", () => void debug.continue(), paused],
    ["pause", "Pause", () => void debug.pause(), canPause && state.phase === "running"],
    ["over", "Over", () => void debug.step("over"), paused],
    ["into", "Into", () => void debug.step("into"), paused],
    ["out", "Out", () => void debug.step("out"), paused],
  ];
  return (
    <section id="debugger" className="pane debugger flex min-h-0 flex-col gap-3 border-t border-grid px-4 pt-4 pb-6 md:border-t-0 md:border-l" aria-label="Debugger">
      <p className="playcap flex justify-between gap-2">
        <span><span className="text-red">//</span> Debugger</span>
        <span className="phase truncate">{phaseText(state)}</span>
      </p>
      {state.pause === false && (
        <p className="note">This browser can't pause a program: it has no WebAssembly JS Promise Integration (JSPI). Breakpoints and steps need it.</p>
      )}
      <div className="flex flex-wrap gap-1" role="toolbar" aria-label="Debugger controls">
        {controls.map(([id, label, act, enabled]) => (
          <Button key={id} data-control={id} className="px-2" title={id === "continue" || id === "pause" ? label : `Step ${id}`} disabled={!enabled} onClick={act}>
            {label}
          </Button>
        ))}
      </div>
      {state.phase === "ended" && <p className="note">The program's <code>main</code> has ended. Run it again to debug it.</p>}
      {canPause && state.phase !== "ended" && state.lines.length === 0 && !paused && <p className="note">Click a line number to set a breakpoint.</p>}
      {state.error && <p className="error">{state.error}</p>}
      {paused && state.paused && (
        <>
          <p className="where">{shortName(state.paused.function)} · line {state.paused.line}</p>
          <div>
            <h3>Frames</h3>
            <ol className="frames-list">
              {state.frames.map((frame, at) => (
                <li key={at}>
                  <button type="button" aria-current={at === state.frame} onClick={() => void debug.select(at)}>
                    {shortName(frame.function)}
                    {frame.line ? ` :${frame.line}` : ""}
                  </button>
                </li>
              ))}
            </ol>
          </div>
          <div>
            <h3>Bindings</h3>
            {state.unread && <p className="note">{state.unread}</p>}
            <dl className="bindings">
              {state.bindings.map((binding) => (
                <div key={binding.name} data-binding={binding.name}>
                  <dt>{binding.name}</dt>
                  <dd className={binding.error !== undefined ? "err" : undefined}>{binding.error ?? binding.value}</dd>
                </div>
              ))}
            </dl>
          </div>
          <form
            className="flex gap-1"
            onSubmit={(event) => {
              event.preventDefault();
              void debug.evaluate(code);
            }}
          >
            <Input className="min-w-0 flex-1" aria-label="Evaluate" placeholder="Rust to evaluate" value={code} onChange={(event) => setCode(event.target.value)} />
            <Button type="submit" className="px-2">Eval</Button>
          </form>
        </>
      )}
      {state.evaluated.length > 0 && (
        <ul className="evaluated">
          {state.evaluated.map((evaluated, at) => (
            <li key={at}>
              <code>{evaluated.code}</code> = <span className={evaluated.error !== undefined ? "err" : undefined}>{evaluated.error ?? evaluated.value}</span>
            </li>
          ))}
        </ul>
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
      const term = (
        <pre className="term">
          {output.chunks.map((chunk, i) => (
            <span key={i} className={chunk.stream === "stderr" ? "err" : undefined}>{chunk.text}</span>
          ))}
          {empty && <span className="none">(no output){"\n"}</span>}
          {output.status !== 0 && <span className="exit">exit status {output.status}{"\n"}</span>}
          {output.note && <span className="note">{"\n" + output.note + "\n"}</span>}
        </pre>
      );
      if (!output.image) return term;
      // An iced program's frame, and its text when it printed or failed.
      return (
        <div className="drawn">
          <img src={output.image} alt="What the program drew" />
          {(!empty || output.status !== 0 || output.note) && term}
        </div>
      );
    }
  }
}
