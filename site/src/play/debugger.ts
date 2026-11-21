// The playground's debugger (D1071): the page's side of the debugger build
// of the browser universe (scripts/universe.ts's DEBUGGER), apart from how
// it is drawn, so a test can drive it with a stand-in instance
// (test/debugger.test.ts). Playground.tsx draws its state as a panel beside
// the code, and the editor's gutter as its breakpoints.
//
// Each run under the debugger gets a fresh instance, which `attach` takes
// before the program runs. It asks `hello` first, which says whether a
// pause can wait (it needs the browser's JSPI), so the panel says so before
// any pause is tried. Where one can, it asks `hold`: the program waits at
// its first statement once it compiled, `start` sets the page's
// breakpoints, and `release` lets it run. The breakpoints are lines of the
// crate root, kept across runs; the debugger's `breakpoints` events say
// where they are now, as a compile moves them. A `paused` event reads the
// frames and the innermost one's bindings; `resumed` clears them. Once
// `main` returns or panics, the run's end and exit status show, and the
// controls have nothing to act on. Bindings and evals are asked with
// `"children": true`, so each value comes with its parts for the panel's
// and the console's trees.

/**
 * Whether this browser can pause a program: its `WebAssembly.Suspending`
 * (JSPI) is the browser's own. The debugger's `hello` makes the same test
 * in the frame, where a page without one has a stand-in
 * (scripts/universe.ts). The page asks it before it fetches the debugger
 * build.
 */
export function canPause(assembly: object | null = globalThis.WebAssembly ?? null): boolean {
  const suspending = assembly && Reflect.get(assembly, "Suspending");
  return typeof suspending === "function" && Function.prototype.toString.call(suspending).includes("[native code]");
}

/** Where the universe puts a snippet's crate root. */
export const ROOT = "/universe/src/main.rs";

/** The debugger build's exports a run uses. */
export interface Debuggee {
  debug?(request: string): Promise<string>;
  debug_events?(listener: (event: string) => void): void;
}

export interface Paused {
  file: string;
  line: number;
  function: string;
  reason: string;
}

export interface Frame {
  function: string;
  file?: string;
  line?: number;
}

/**
 * One part of a value, as the inspector splits its `{:?}` text when asked
 * with `"children": true` (src/inspect/parts.rs): a field, an item or a
 * map's entry, named by its field, index or key.
 */
export interface Part {
  name: string;
  value: string;
  children?: Part[];
}

export interface Binding {
  name: string;
  value?: string;
  error?: string;
  /** The value's parts, when it has any. */
  children?: Part[];
}

/** One `eval` and what it gave. */
export interface Evaluated {
  code: string;
  value?: string;
  error?: string;
  /** The value's parts, when it has any. */
  children?: Part[];
}

/**
 * Rust naming the part `name` of the value at `place`, whose `{:?}` text is
 * `text`, as cli/src/dap/place.rs names one for an editor's setVariable: a
 * map's entry through `get_mut` by a reference to its key (a string
 * literal is one already), `[i]` a sequence's item, any other name a
 * field. Where Rust can't reach the part so, the compiler says why.
 */
export function partPlace(place: string, text: string, name: string): string {
  if (text.startsWith("{")) return `(*${place}.get_mut(${name.startsWith('"') ? name : `&${name}`}).unwrap())`;
  if (name.startsWith("[")) return `${place}${name}`;
  return `${place}.${name}`;
}

/** Code that assigns `value` to `place`, its value then the code's. */
export function assignment(place: string, value: string): string {
  return `${place} = ${value}; ${place}`;
}

/** What an assignment gave: the value written, or rustc's refusal. */
export type Assigned = { value: string } | { error: string };

/**
 * Where the program is: no program attached yet, held at its start, running,
 * paused, or past the end of `main`.
 */
export type Phase = "idle" | "held" | "running" | "paused" | "ended";

export interface DebugState {
  phase: Phase;
  /** Whether a pause can wait in this browser, once `hello` said. */
  pause: boolean | null;
  paused: Paused | null;
  frames: Frame[];
  /** The frame whose bindings show, 0 the innermost. */
  frame: number;
  bindings: Binding[];
  /** Why the frame's bindings can't be read. */
  unread: string | null;
  /** The breakpoints' lines in the crate root. */
  lines: number[];
  evaluated: Evaluated[];
  /** The last request the debugger refused, with its reason. */
  error: string | null;
  /** The exit status once `main` returned or panicked. */
  status: number | null;
}

const IDLE: DebugState = {
  phase: "idle", pause: null, paused: null, frames: [], frame: 0, bindings: [], unread: null, lines: [], evaluated: [], error: null, status: null,
};

/** The evals the console keeps as its scrollback, latest last. */
const KEPT = 100;

export class Debugger {
  state: DebugState = IDLE;
  private listeners = new Set<(state: DebugState) => void>();
  /** The attached instance's `debug`, as parsed JSON. */
  private ask: ((request: object) => Promise<any>) | null = null;
  /** Counts attachments: a detached instance's events and answers are dropped. */
  private run = 0;

  subscribe(listener: (state: DebugState) => void): () => void {
    this.listeners.add(listener);
    return () => this.listeners.delete(listener);
  }

  private set(change: Partial<DebugState>) {
    this.state = { ...this.state, ...change };
    for (const listener of this.listeners) listener(this.state);
  }

  /**
   * Take `instance`, which runs the next program, before it runs: hear its
   * events, learn whether it can pause, and hold its start where it can.
   * `false` when it isn't a debugger build.
   */
  async attach(instance: Debuggee): Promise<boolean> {
    const run = ++this.run;
    const debug = instance.debug?.bind(instance);
    if (!debug || !instance.debug_events) {
      this.ask = null;
      this.set({ ...IDLE, lines: this.state.lines, error: "this module has no debugger" });
      return false;
    }
    const ask = async (request: object) => JSON.parse(await debug(JSON.stringify(request)));
    this.ask = ask;
    instance.debug_events((text) => {
      if (run === this.run) this.heard(JSON.parse(text));
    });
    const hello = await ask({ op: "hello" });
    if (run !== this.run) return false;
    const pause = hello.pause === true;
    this.set({ ...IDLE, lines: this.state.lines, pause, evaluated: this.state.evaluated });
    if (pause) {
      const hold = await ask({ op: "hold" });
      if (run === this.run && hold.held) this.set({ phase: "held" });
    }
    return true;
  }

  /**
   * The held program compiled and waits at its start: set the page's
   * breakpoints, then let it run.
   */
  async start(): Promise<void> {
    const run = this.run;
    const ask = this.ask;
    if (!ask) return;
    if (this.state.phase === "held") {
      for (const line of this.state.lines) await this.answered(run, ask({ op: "break", file: ROOT, line }));
      await ask({ op: "release" });
    }
    // A pause the release ran into is told as an event, which already came.
    if (run === this.run && this.state.phase === "held") this.set({ phase: "running" });
  }

  /** What the attached run calls once `main` returns or panics, with its status. */
  ending(): (status: number) => void {
    const run = this.run;
    return (status) => {
      if (run !== this.run) return;
      this.set({ phase: "ended", status, paused: null, frames: [], frame: 0, bindings: [], unread: null });
    };
  }

  /** The instance went away: its program no longer runs. */
  detach() {
    this.run++;
    this.ask = null;
    this.set({ ...IDLE, lines: this.state.lines, evaluated: this.state.evaluated });
  }

  /**
   * Forget every breakpoint, for a different snippet: the next run sets
   * none. A program still attached runs the old code, which the next run
   * replaces, so it is not asked.
   */
  clearBreakpoints() {
    if (this.state.lines.length) this.set({ lines: [] });
  }

  /** A click on line `line`'s number: set a breakpoint there, or clear the one there. */
  async toggle(line: number): Promise<void> {
    const had = this.state.lines.includes(line);
    const ask = this.ask;
    if (!ask) {
      const lines = had ? this.state.lines.filter((at) => at !== line) : [...this.state.lines, line].sort((a, b) => a - b);
      this.set({ lines });
      return;
    }
    const answer = await this.answered(this.run, ask(had ? { op: "clear", file: ROOT, line } : { op: "break", file: ROOT, line }));
    // Each answer lists the breakpoints there are now.
    const list = answer?.breakpoints ?? answer?.left;
    if (Array.isArray(list)) this.set({ lines: linesOf(list) });
  }

  continue(): Promise<void> {
    return this.resume({ op: "continue" });
  }

  pause(): Promise<void> {
    return this.resume({ op: "pause" });
  }

  step(kind: "over" | "into" | "out"): Promise<void> {
    return this.resume({ op: "step", kind });
  }

  private async resume(request: object): Promise<void> {
    if (this.ask) await this.answered(this.run, this.ask(request));
  }

  /** Show frame `frame`'s bindings. */
  async select(frame: number): Promise<void> {
    const run = this.run;
    if (!this.ask || !this.state.paused) return;
    this.set({ frame });
    const answer = await this.ask({ frame, children: true });
    if (run !== this.run || !this.state.paused || this.state.frame !== frame || answer.reached !== true) return;
    this.set({ bindings: answer.bindings ?? [], unread: answer.unread ?? null, frames: answer.frames ?? this.state.frames });
  }

  /**
   * Evaluate `code` where the program is paused, in the selected frame. The
   * panel's bindings are the program's, so an evaluation, which may write any
   * place, reads the frame again rather than keeping what it showed.
   */
  async evaluate(code: string): Promise<void> {
    const run = this.run;
    const frame = this.state.frame;
    if (!this.ask || !code.trim()) return;
    const answer = await this.ask({ op: "eval", code, frame, children: true });
    if (run !== this.run) return;
    if (this.state.paused) await this.select(frame);
    const evaluated: Evaluated = answer.error !== undefined
      ? { code, error: String(answer.error) }
      : { code, value: String(answer.value), ...(Array.isArray(answer.children) ? { children: answer.children } : {}) };
    this.set({ evaluated: [...this.state.evaluated, evaluated].slice(-KEPT) });
  }

  /**
   * Assign `value`, Rust, to `place` in the selected frame, through the same
   * eval the console runs, so the program's compiler decides whether Rust
   * allows the write there. A write reads the frame's bindings again.
   */
  async assign(place: string, value: string): Promise<Assigned> {
    const run = this.run;
    const frame = this.state.frame;
    if (!this.ask || !this.state.paused) return { error: "The program isn't paused." };
    const answer = await this.ask({ op: "eval", code: assignment(place, value), frame });
    if (answer.error !== undefined) return { error: String(answer.error) };
    if (run === this.run) await this.select(frame);
    return { value: String(answer.value) };
  }

  /** `answer`, its refusal shown as the panel's error while `run` is attached. */
  private async answered(run: number, answer: Promise<any>): Promise<any> {
    const value = await answer;
    if (run === this.run) this.set({ error: value?.error !== undefined ? String(value.error) : null });
    return value;
  }

  /** One event of the attached instance's debugger. */
  private heard(event: any) {
    switch (event.event) {
      case "paused": {
        const paused: Paused = { file: event.file, line: event.line, function: event.function, reason: event.reason };
        this.set({ phase: "paused", paused, frame: 0, frames: [], bindings: [], unread: null });
        void this.select(0);
        return;
      }
      case "resumed":
        if (this.state.phase === "ended") return;
        this.set({ phase: "running", paused: null, frames: [], frame: 0, bindings: [], unread: null });
        return;
      case "breakpoints":
        if (Array.isArray(event.breakpoints)) this.set({ lines: linesOf(event.breakpoints) });
        return;
    }
  }
}

/** The crate root's lines among the debugger's breakpoints. */
function linesOf(breakpoints: { file?: string; line?: number }[]): number[] {
  const lines = breakpoints.filter((point) => point.file === ROOT && typeof point.line === "number").map((point) => point.line!);
  return [...new Set(lines)].sort((a, b) => a - b);
}
