// The playground's state and what it does, apart from how it is drawn, so a
// test can drive it with stand-ins (test/session.test.ts). Playground.tsx
// draws it and hands it the browser.
//
// A snippet runs in the browser, and only there: rawr-web (WEB_MODULE,
// from web/build.sh, in the set the site's pointer names: scripts/web.ts)
// loads once the page is up, and a program's output
// shows as the ?play bot's would. No server runs a visitor's snippet. A run
// that can't start in the browser shows why and stops. Share goes through
// POST /play/share.
//
// A snippet that names a crate the browser's prebuilt runner has (iced,
// tokio, ratatui: universe.toml) runs on that runner instead
// (scripts/universe.ts), in a frame of its own in the output pane: the
// program draws there and takes clicks and keys there. Each Run gets a
// fresh frame, and the next one is made ahead, so a run only compiles and
// runs. An edit reloads the program running in its frame instead, with its
// state kept, as `rawr run` reloads a save (#1455): a compile error shows
// under the frame while the program runs its last good code, and an edit
// that can't reload in place, or a program that has ended, runs in the
// next frame.
//
// A snippet that starts with a cargo-script frontmatter block
// (frontmatter.ts) runs with the block blanked when each dependency it
// names is one of the prebuilt runner's crates, at a version its manifest
// allows. The browser builds nothing, so any other dependency gets one
// line that says which crates run here.
//
// The address keeps the page's state: `#<slug>` names the example picked,
// written with history.replaceState, so `/play#<slug>` opens that example
// as written. An edit keeps the hash, which names where the code came
// from, until Share or the next pick replaces it. A share's code wins over
// an example's hash, which a share page drops. Share's link never has a
// hash. Picking an example whose code differs from the editor's clears the
// debugger's breakpoints, whose lines were the old code's.
//
// The debugger icon (D1071) runs every program, plain or not, on the
// debugger build of the universe instead (Deps.debugging), fetched the first
// time it is turned on: the program in the output starts again from its
// start, under the debugger (debugger.ts), and the panel opens. Turned off,
// the program runs again as before, on the plain module or rawr-web. A
// browser without JSPI can't pause a program, so there it stays off.

import { crates, usesCrate, type Debugged, type Frame } from "../scripts/universe.ts";
import { ROOT, type Debugger } from "./debugger.ts";
import { frontmatter, offered, satisfied } from "./frontmatter.ts";

/** rawr-web's module: its glue's name in a set of web/build.sh's files (scripts/web.ts). */
export const WEB_MODULE = "rawr_web.js";

export type Theme = "dark" | "light";

export interface Snippet {
  code: string;
}

export interface Example {
  name: string;
  code: string;
  /** The menu group: "Rust". */
  group?: string;
  /** The name in the address: "Word count" is "word-count". */
  slug: string;
}

/** What the Worker writes into the page (#play-data) for each request. */
export interface PlayData {
  snippet: Snippet;
  examples: Example[];
  /** The share this page was opened from. */
  shared: string | null;
}

export interface Chunk {
  stream: "stdout" | "stderr";
  text: string;
}

export interface TextResult {
  /** "text", or "error" for a snippet that does not compile. */
  kind?: string;
  status: number;
  output?: Chunk[];
  note?: string;
  text?: string;
}

export type Output =
  | { kind: "note" }
  /** A program's output, with its status. */
  | { kind: "term"; chunks: Chunk[]; status: number; note?: string }
  /**
   * A program that runs in a runner frame, shown in the frame host, and
   * what it printed. `error` is an edit's compile error, which left it
   * running its last good code.
   */
  | { kind: "frame"; chunks: Chunk[]; status: number; error?: string }
  | { kind: "error"; text: string };

export interface State {
  code: string;
  theme: Theme;
  output: Output;
  status: string;
  /** The example the code is, by slug, or "". */
  mark: string;
  /** Whether programs run under the debugger. */
  debugging: boolean;
}

/** rawr-web's API (web/src/lib.rs). */
export interface Web {
  run_text(code: string): string;
}

/** The browser's prebuilt runner (scripts/universe.ts), as the playground uses it. */
export interface Universe {
  manifest(): Promise<string>;
  frame(parent: Element): Promise<Frame>;
  /** A run `debugged` is held at its start, and tells its end (scripts/universe.ts). */
  run(frame: Frame, code: string, debugged?: Debugged): Promise<any>;
  /** The edit `code` for the program running in `frame`, which reloads in place. */
  replace(frame: Frame, code: string): any;
}

/** What the session touches outside itself. */
export interface Deps {
  fetch: typeof fetch;
  location: { pathname: string; search: string; hash: string; origin: string };
  replaceState(url: string): void;
  storage: { getItem(key: string): string | null; setItem(key: string, value: string): void } | null;
  /** Imports WEB_MODULE and initializes it: a fresh copy on each call. */
  loadWeb(): Promise<Web>;
  /** The browser's prebuilt runner, for snippets that use crates. Without it they run in rawr-web. */
  universe?: Universe;
  /** Where a runner's frame goes: the output pane's host for it. */
  frameHost?(): Element | null;
  /**
   * The debugger build of the prebuilt runner, the page's side of its debugger,
   * and whether this browser can pause a program (debugger.ts's
   * `canPause`). Where it can't, the debugger stays off.
   */
  debugging?: { universe: Universe; debugger: Debugger; pausable: boolean };
  now(): number;
  clipboard(text: string): Promise<void>;
  setTimeout(f: () => void, ms: number): unknown;
  clearTimeout(id: unknown): void;
}

/** The latest snippet stays in this browser, as on the Rust Playground. */
const DRAFT = "rawr-play-draft";
/** What New puts in the editor. */
export const BLANK = "fn main() {\n    \n}\n";

export class Session {
  readonly data: PlayData;
  readonly deps: Deps;
  state: State;
  private listeners = new Set<(state: State) => void>();
  private path: string;
  private restored: Example | undefined;
  private web: Promise<Web> | null = null;
  private loaded = false;
  /** The prebuilt runner's manifest, which says which crates it has. */
  private declared: Promise<string> | null = null;
  /** The runner's frame shown, and the next one, made ahead. */
  private frame: Frame | null = null;
  private next: Promise<Frame> | null = null;
  /** The edit the frame's program refused last, for its compile error. */
  private refused: string | null = null;
  private runs = 0;
  private typing: unknown = null;
  private saving: unknown = null;

  constructor(data: PlayData, deps: Deps, theme: Theme) {
    this.data = data;
    this.deps = deps;
    const { location } = deps;
    this.path = location.pathname;
    const s = data.snippet;
    this.state = {
      code: s.code, theme,
      output: { kind: "note" }, status: data.shared ? `shared · ${data.shared}` : "ready", mark: "",
      debugging: false,
    };
    let named = "";
    try {
      named = decodeURIComponent(location.hash.slice(1));
    } catch {}
    this.restored = data.shared ? undefined : data.examples.find((e) => e.slug === named);
    if (this.restored) {
      this.fill(this.restored);
      this.state.mark = this.restored.slug;
    } else if (!data.shared) {
      try {
        const draft = JSON.parse(deps.storage?.getItem(DRAFT) ?? "null");
        if (draft && typeof draft.code === "string" && draft.code.trim()) this.fill(draft);
      } catch {}
    }
    if (location.search || location.hash) this.remember();
  }

  subscribe(listener: (state: State) => void): () => void {
    this.listeners.add(listener);
    return () => this.listeners.delete(listener);
  }

  private set(change: Partial<State>) {
    this.state = { ...this.state, ...change };
    for (const listener of this.listeners) listener(this.state);
  }

  private fill(s: { code: string }) {
    this.state = { ...this.state, code: s.code };
  }

  snippet(): Snippet {
    return { code: this.state.code };
  }

  private save = () => {
    try {
      this.deps.storage?.setItem(DRAFT, JSON.stringify(this.snippet()));
    } catch {}
  };

  private edited() {
    this.deps.clearTimeout(this.saving);
    this.saving = this.deps.setTimeout(this.save, 400);
  }

  /** An edit runs again after 500 ms. */
  private typed() {
    this.deps.clearTimeout(this.typing);
    this.typing = this.deps.setTimeout(() => this.browser(true), 500);
  }

  private remember() {
    this.deps.replaceState(this.path + (this.state.mark ? "#" + this.state.mark : ""));
  }

  setCode(code: string) {
    if (code === this.state.code) return;
    this.set({ code });
    this.edited();
    this.typed();
  }

  /** The page's theme changed. */
  setTheme(theme: Theme) {
    this.set({ theme });
  }

  /** An example from the menu, or "reset" for the first one. */
  pick(value: string) {
    const example = value === "reset" ? this.data.examples[0] : this.data.examples.find((e) => e.slug === value);
    if (!example) return;
    if (example.code !== this.state.code) this.deps.debugging?.debugger.clearBreakpoints();
    this.fill(example);
    this.set({ mark: example.slug });
    this.save();
    this.path = "/play";
    this.remember();
    this.go();
  }

  /** New: an empty `main` in place of the code, with no example picked. */
  blank() {
    this.deps.debugging?.debugger.clearBreakpoints();
    this.fill({ code: BLANK });
    this.set({ mark: "" });
    this.save();
    this.path = "/play";
    this.remember();
    this.go();
  }

  go(): Promise<void> {
    return this.browser();
  }

  /**
   * The debugger icon: run the program again from its start under the
   * debugger, or, turned off, as it ran before. A browser that can't
   * pause a program never turns it on, nor fetches its build.
   */
  debug(on = !this.state.debugging): Promise<void> {
    if (on === this.state.debugging || (on && !this.deps.debugging?.pausable)) return Promise.resolve();
    // The frame made ahead is the other module's.
    const next = this.next;
    this.next = null;
    next?.then((frame) => frame.element.remove(), () => {});
    this.frame?.element.remove();
    this.frame = null;
    this.deps.debugging?.debugger.detach();
    this.set({ debugging: on });
    return on ? this.browser() : this.go();
  }

  /** The prebuilt runner the next frame runs on: the debugger's while it is on. */
  private get universe(): Universe | undefined {
    return this.state.debugging ? this.deps.debugging?.universe : this.deps.universe;
  }

  /** Any other output than a frame's ends the frame shown. */
  private show(output: Output) {
    if (output.kind !== "frame") {
      this.frame?.element.remove();
      this.frame = null;
      this.deps.debugging?.debugger.detach();
    }
    this.set({ output });
  }

  async share(): Promise<void> {
    this.set({ status: "sharing…" });
    const response = await this.deps.fetch("/play/share", {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify(this.snippet()),
    });
    if (!response.ok) {
      this.show({ kind: "error", text: await response.text() });
      this.set({ status: "share failed" });
      return;
    }
    const { id } = await response.json();
    // An example's hash does not apply to a share: its code is the share.
    const link = this.deps.location.origin + "/play/" + id;
    this.path = "/play/" + id;
    this.set({ mark: "" });
    this.remember();
    try {
      await this.deps.clipboard(link);
      this.set({ status: "link copied · " + id });
    } catch {
      this.set({ status: "shared · " + id });
    }
  }

  private load(): Promise<Web> {
    this.web ??= this.deps.loadWeb().then(
      (m) => ((this.loaded = true), m),
      (error) => {
        this.web = null;
        throw error;
      },
    );
    return this.web;
  }

  /** A run that can't start in the browser: why, in place of the output. */
  private stopped(reason: string) {
    this.show({ kind: "error", text: reason });
    this.set({ status: "did not run" });
  }

  /** The prebuilt runner's manifest, or none when it has no manifest. */
  private manifest(): Promise<string> {
    if (!this.deps.universe) return Promise.resolve("");
    this.declared ??= this.deps.universe.manifest().catch(() => ((this.declared = null), ""));
    return this.declared;
  }

  /** The crates the prebuilt runner declares, or none when it has no manifest. */
  private async crates(): Promise<string[]> {
    return crates(await this.manifest());
  }

  /** The next runner frame in `host`, made ahead of its run. */
  private nextFrame(host: Element): Promise<Frame> {
    return (this.next ??= this.universe!.frame(host).catch((error) => {
      this.next = null;
      throw error;
    }));
  }

  /**
   * Run `code`, which uses crates, in a runner frame of its own in place of
   * the output: the program draws there and takes input there. What it
   * prints shows under it, and a compile error in its place.
   */
  private async framed(mine: number, code: string, restarted = ""): Promise<void> {
    const host = this.deps.frameHost?.();
    if (!host) return this.stopped("This page has no place for the program's window.");
    if (!this.next) this.set({ status: this.state.debugging ? "loading the debugger…" : "loading…" });
    let frame: Frame;
    try {
      frame = await this.nextFrame(host);
    } catch (error) {
      console.error("The browser's prebuilt runner did not load:", error);
      if (this.state.debugging) return this.undebuggable(why(error));
      return this.stopped("The browser's prebuilt runner did not load: " + why(error));
    }
    // A newer run takes this frame.
    if (mine !== this.runs) return;
    this.next = null;
    this.frame?.element.remove();
    this.frame = frame;
    const start = this.deps.now();
    // Under the debugger the program waits at its start for the page's
    // breakpoints.
    const debug = this.state.debugging ? this.deps.debugging?.debugger : undefined;
    const attached = debug ? await debug.attach(frame.instance) : false;
    const result = await this.universe!.run(frame, code, attached ? { started: () => debug!.start(), ended: debug!.ending() } : undefined);
    if (mine !== this.runs) return;
    const took = Math.round(this.deps.now() - start) + " ms in the browser";
    if (result.kind === "error") this.result({ ...result, text: named(result.text ?? "") }, took);
    else {
      this.show({ kind: "frame", chunks: result.output ?? [], status: result.status });
      this.set({ status: took + (result.status !== 0 ? " · exit " + result.status : "") + (restarted && " · restarted: " + restarted) });
    }
    // The next run only compiles and runs.
    this.nextFrame(host).catch(() => {});
  }

  /**
   * Reload the program running in the frame shown with the edit `code`,
   * its state kept. A compile error shows under the frame and the program
   * runs on; an edit that can't reload there runs in the next frame.
   */
  private reload(mine: number, code: string, output: Extract<Output, { kind: "frame" }>): Promise<void> | void {
    const start = this.deps.now();
    const answer = this.universe!.replace(this.frame!, code);
    const took = Math.round(this.deps.now() - start) + " ms in the browser";
    switch (answer.kind) {
      case "restart":
        return this.framed(mine, code, answer.reason);
      case "unchanged":
        // The code the program runs, after an edit that failed: its error goes.
        if (output.error !== undefined && code !== this.refused) {
          this.show({ kind: "frame", chunks: output.chunks, status: output.status });
          this.set({ status: "unchanged" });
        }
        return;
      case "error":
        this.refused = code;
        this.show({ ...output, error: named(answer.text ?? "") });
        this.set({ status: "error · the program runs its last good code" });
        return;
      default:
        this.show({ kind: "frame", chunks: [...output.chunks, ...(answer.output ?? [])], status: output.status });
        this.set({ status: took + " · reloaded in place" });
    }
  }

  /**
   * Run the code in the browser. An `edit` reloads the program a frame runs
   * in place. A run that a later one replaced shows nothing.
   */
  async browser(edit = false): Promise<void> {
    this.save();
    const mine = ++this.runs;
    let code = this.state.code;
    // A frontmatter block runs blanked, when the browser has its crates.
    const block = frontmatter(code);
    if (block) {
      const has = offered(await this.manifest());
      if (mine !== this.runs) return;
      if (!block.dependencies.every((dependency) => satisfied(dependency, has))) {
        return this.stopped(onlyHere(has.map((dependency) => dependency.package)));
      }
      code = block.blanked;
    }
    // Under the debugger every program runs on its prebuilt runner.
    if (this.state.debugging || (this.deps.universe && usesCrate(code, await this.crates()))) {
      if (mine !== this.runs) return;
      const output = this.state.output;
      if (edit && this.frame && output.kind === "frame") return this.reload(mine, code, output);
      return this.framed(mine, code);
    }
    if (!this.loaded) this.set({ status: "loading…" });
    let m: Web;
    try {
      m = await this.load();
    } catch (error) {
      if (mine !== this.runs) return;
      return this.stopped("rawr did not load in this browser: " + why(error));
    }
    if (mine !== this.runs) return;
    const start = this.deps.now();
    let text: string;
    try {
      text = m.run_text(code);
    } catch (error) {
      // A panic in rawr-web itself traps the module, whose state it may
      // have left half-written: the next run loads a fresh copy of it.
      this.web = null;
      this.loaded = false;
      return this.stopped("rawr stopped in the browser: " + why(error) + ". The next run loads it again.");
    }
    const result: TextResult = JSON.parse(text);
    this.result(result, Math.round(this.deps.now() - start) + " ms in the browser");
  }

  /** The debugger's module didn't load: it turns off, and the program runs as before. */
  private async undebuggable(reason: string): Promise<void> {
    this.set({ debugging: false });
    await this.browser();
    this.set({ status: this.state.status + " · the debugger did not load: " + reason });
  }

  /** What a run gave: its output or its report. */
  private result(result: TextResult, took: string) {
    if (result.kind === "error") {
      this.show({ kind: "error", text: result.text ?? "" });
      this.set({ status: "error" });
    } else {
      this.show(terminal(result));
      this.set({ status: took + (result.status !== 0 ? " · exit " + result.status : "") });
    }
  }

  /**
   * Once the page has loaded: rawr-web loads, so it does not hold up the
   * first paint, and an example the address named runs.
   */
  preload() {
    // Code that uses crates warms a runner frame, so its first Run only
    // compiles and runs.
    const host = this.deps.frameHost?.();
    if (this.deps.universe && host) {
      this.crates().then((names) => {
        if (usesCrate(this.state.code, names)) this.nextFrame(host).catch(() => {});
      });
    }
    this.load().then(() => {
      if (this.restored && this.runs === 0) this.browser();
    }, () => {});
  }
}

/**
 * The line a snippet gets when its frontmatter names a crate the browser's
 * prebuilt runner doesn't have, which names the crates it has.
 */
export function onlyHere(crates: string[]): string {
  const names = [...new Set(crates)].sort();
  const bot = "The ?play bot on Discord builds more crates.";
  if (names.length === 0) return `This browser runs no snippet with dependencies. ${bot}`;
  const listed = names.length > 1 ? names.slice(0, -1).join(", ") + " and " + names.at(-1) : names[0];
  return `The browser runs only snippets whose dependencies are among ${listed}. ${bot}`;
}

/**
 * Why a load failed, short enough for the status line: the first line of
 * the error's message, without the error's name, cut at 120 characters.
 */
export function why(error: unknown): string {
  const text = (error instanceof Error ? error.message : String(error)).split("\n")[0].replace(/^\w*Error: /, "").trim();
  if (!text) return "no reason given";
  return text.length > 120 ? text.slice(0, 119) + "…" : text;
}

/** A plain program's output: stdout, stderr in its own colour, then the exit status if not 0. */
export function terminal(result: TextResult): Output {
  return { kind: "term", chunks: result.output ?? [], status: result.status, note: result.note };
}

/**
 * A report from the browser's prebuilt runner, which names the snippet by
 * the path it compiles it at, with the snippet named `play.rs`, as the
 * bot's reports name it.
 */
export function named(text: string): string {
  return text.replaceAll(ROOT, "play.rs");
}
