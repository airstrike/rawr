// The playground's island: the controls, the editor and the output, drawn
// from a Session (session.ts). The page is static; the Worker writes each
// request's data (the share, the examples, Turnstile's key) into #play-data.
// Before hydration the server-rendered controls show; the editor mounts
// once the island runs.
import type { EditorView } from "@codemirror/view";
import { Fragment, useEffect, useRef, useState, useSyncExternalStore } from "react";
import { Button } from "../components/ui/button.tsx";
import { Input, Textarea } from "../components/ui/input.tsx";
import { Select, SelectContent, SelectGroup, SelectItem, SelectLabel, SelectSeparator, SelectTrigger, SelectValue } from "../components/ui/select.tsx";
import { ToggleGroup, ToggleGroupItem } from "../components/ui/toggle-group.tsx";
import { onTheme, setTheme, theme as pageTheme, type Theme } from "../lib/theme.ts";
import { cn } from "../lib/utils.ts";
import { createEditor, setDoc } from "./editor.ts";
import { Session, TURNSTILE_JS, WEB_MODULE, type Example, type Output, type PlayData, type State, type Web, type Where } from "./session.ts";

const HINT = "(sent before preview, separated by newline)";
const PLAIN_HINT = "(ignored for programs without iced)";

/** The page's data, or a bare page's when the Worker wrote none (a static preview). */
function readData(): PlayData {
  try {
    const data = JSON.parse(document.getElementById("play-data")?.textContent ?? "null");
    if (data) return data;
  } catch {}
  return { snippet: { code: "", theme: "dark", size: "", messages: [] }, examples: [], sitekey: null, pass: false, shared: null };
}

function browserDeps(liveSlot: () => HTMLElement | null) {
  let holder: HTMLDivElement | null = null;
  const live = {
    holder() {
      if (!holder) {
        holder = document.createElement("div");
        holder.className = "live";
        holder.innerHTML = '<div id="iced"></div>';
      }
      return holder as unknown as { className: string; style: Record<string, string>; classList: { toggle(name: string, on: boolean): void } };
    },
    attach(on: boolean) {
      const slot = liveSlot();
      if (!slot) return;
      if (on && holder && holder.parentNode !== slot) slot.replaceChildren(holder);
      if (!on && slot.firstChild) slot.replaceChildren();
    },
  };
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
      const m = await import(/* @vite-ignore */ WEB_MODULE);
      await m.default();
      return m as Web;
    },
    gpu: () => Boolean((navigator as Navigator & { gpu?: unknown }).gpu || document.createElement("canvas").getContext("webgl2")),
    now: () => performance.now(),
    clipboard: (text: string) => navigator.clipboard.writeText(text),
    turnstile: () => (window as Window & { turnstile?: any }).turnstile,
    loadTurnstile: () => {
      if (document.querySelector("script[data-turnstile]")) return;
      const js = document.createElement("script");
      js.src = TURNSTILE_JS;
      js.async = true;
      js.dataset.turnstile = "";
      document.head.append(js);
    },
    live,
    setTimeout: (f: () => void, ms: number) => window.setTimeout(f, ms),
    clearTimeout: (id: unknown) => window.clearTimeout(id as number),
  };
}

const NONE: State = {
  code: "", messages: "", size: "", theme: "dark", where: "browser", output: { kind: "note" }, status: "ready", busy: false, plain: false, mark: "",
};

export default function Playground() {
  const [session, setSession] = useState<Session | null>(null);
  const state = useSyncExternalStore(
    (listener) => session?.subscribe(listener) ?? (() => {}),
    () => session?.state ?? NONE,
    () => NONE,
  );
  const liveSlot = useRef<HTMLDivElement>(null);
  const host = useRef<HTMLDivElement>(null);
  const editor = useRef<EditorView | null>(null);

  // After hydration: the session from the page's data, the editor, the theme and the keys.
  useEffect(() => {
    const data = readData();
    const s = new Session(data, browserDeps(() => liveSlot.current), pageTheme());
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
      <div className="tools flex flex-wrap items-center gap-2 border-b border-line px-4 py-3 md:px-6">
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
        <ToggleGroup
          type="single"
          className="theme-toggle"
          aria-label="Theme"
          value={state.theme}
          onValueChange={(value) => value && setTheme(value as Theme)}
        >
          <ToggleGroupItem value="dark" data-theme-item="dark">Dark</ToggleGroupItem>
          <ToggleGroupItem value="light" data-theme-item="light">Light</ToggleGroupItem>
        </ToggleGroup>
        <Input
          className="order-3 w-[9em] flex-1 md:flex-none"
          placeholder="size: auto"
          aria-label="Size, WxH"
          spellCheck={false}
          value={state.size}
          onChange={(event) => session?.setSize(event.target.value)}
        />
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
        <Button id="share" className="order-3" onClick={() => void session?.share()}>Share</Button>
        <Button id="run" className="order-3" variant="primary" title="Ctrl/Cmd + Enter" disabled={state.busy} onClick={() => void session?.go()}>
          Run
        </Button>
      </div>
      <main className="flex min-h-0 flex-1 flex-col md:grid md:grid-cols-2">
        <section className="pane flex min-h-0 flex-col px-4 pt-4 pb-6 md:px-6">
          <p className="playcap mb-2"><span className="text-ink">//</span> Code</p>
          <div ref={host} className="editor min-h-[300px] flex-1 md:min-h-[360px]" />
          <p className="playcap mt-3 mb-1.5 flex flex-wrap justify-between gap-x-2">
            <span>Messages</span>
            <span className="normal-case tracking-normal">{state.plain ? PLAIN_HINT : HINT}</span>
          </p>
          <Textarea
            className={cn("h-[4.6em] whitespace-pre", state.plain && "opacity-45")}
            spellCheck={false}
            placeholder="Message::Foo"
            aria-label="Messages"
            value={state.messages}
            onChange={(event) => session?.setMessages(event.target.value)}
          />
          {session?.data.sitekey && <div id="ts" className="mt-3" />}
        </section>
        <section className="pane flex min-h-0 flex-col border-t border-line px-4 pt-4 pb-6 md:border-t-0 md:border-l md:px-6">
          <p className="playcap mb-2 flex justify-between gap-2">
            <span><span className="text-ink">//</span> Output</span>
            <span id="status" className="truncate">{state.status}</span>
          </p>
          <div id="out" className={cn("out", state.output.kind === "live" && "has-live")} aria-live="polite">
            <div ref={liveSlot} className="contents" />
            <OutputView output={state.output} />
          </div>
        </section>
      </main>
      <footer className="playfoot flex flex-wrap justify-between gap-4 border-t border-line px-4 py-3 md:px-6">
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

function OutputView({ output }: { output: Output }) {
  switch (output.kind) {
    case "note":
      return (
        <p className="note">
          Run (Ctrl/Cmd + Enter). In the browser, iced code runs live and updates as you type. On the server, iced code shows its last frame, and an app that moves shows a GIF. Other programs show their output.
        </p>
      );
    case "error":
      return <pre className="error">{output.text}</pre>;
    case "image":
      return <img src={output.url} alt={output.animated ? "The animation" : "The rendered frame"} />;
    case "live":
      return output.report ? <pre className="error">{output.report}</pre> : null;
    case "term": {
      const empty = output.done && !output.chunks.some((chunk) => chunk.text !== "");
      return (
        <pre className="term">
          {output.chunks.map((chunk, i) => (
            <span key={i} className={chunk.stream === "stderr" ? "err" : undefined}>{chunk.text}</span>
          ))}
          {empty && <span className="none">(no output){"\n"}</span>}
          {output.done && output.status !== 0 && <span className="exit">exit status {output.status}{"\n"}</span>}
          {output.done && output.note && <span className="note">{"\n" + output.note + "\n"}</span>}
        </pre>
      );
    }
  }
}
