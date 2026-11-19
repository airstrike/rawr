// The playground's state and what it does, apart from how it is drawn, so a
// test can drive it with stand-ins (test/session.test.ts). Playground.tsx
// draws it and hands it the browser.
//
// A snippet runs in the browser by default: rawr-web (WEB_MODULE, from
// web/build.sh) loads once the page is up, and a program's output shows as
// the server's would. "Run on server" runs it through POST /play/render
// instead, which answers the same way, and shows what an iced program drew
// (a PNG, or a GIF when it animated) above what it printed. The server also
// runs a snippet when the module did not load. Share goes through POST
// /play/share.
//
// A snippet that names a crate the browser's universe declares (iced,
// tokio, ratatui: universe.toml) runs on the universe instead
// (scripts/universe.ts), in a frame of its own in the output pane: the
// program draws there and takes clicks and keys there. Each Run gets a
// fresh frame, and the next one is made ahead, so a run only compiles and
// runs. An edit reloads the program running in its frame instead, with its
// state kept, as `rawr run` reloads a save (#1455): a compile error shows
// under the frame while the program runs its last good code, and an edit
// that can't reload in place, or a program that has ended, runs in the
// next frame. The server runs it when the universe doesn't load.
//
// The address keeps the page's state: `#<slug>` names the example loaded,
// and `?run=server` the server when the visitor chose it (the browser, the
// default, has no parameter). Both are written with history.replaceState.
// A share's code wins over an example's hash, which a share page drops, and an
// edit to the code drops the hash, since the code no longer is the example's.
// Share's link keeps ?run=server and never has a hash.
//
// The theme is the page's (theme.ts): Turnstile's widget draws in it.
//
// The debugger icon (D1071) runs every program, plain or not, on the
// debugger build of the universe instead (Deps.debugging), fetched the first
// time it is turned on: the program in the output starts again from its
// start, under the debugger (debugger.ts), and the panel opens. Turned off,
// the program runs again as before, on the plain module or rawr-web.

import { crates, usesCrate, type Frame } from "../scripts/universe.ts";
import type { Debugger } from "./debugger.ts";

/** rawr-web's module: web/build.sh's output, which the release puts in the Worker's /web. */
export const WEB_MODULE = "/web/rawr_web.js";

export type Theme = "dark" | "light";
export type Where = "browser" | "server";

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
  /** Turnstile site key; no widget without one. */
  sitekey: string | null;
  /** The browser holds a valid pass: no widget unless the server refuses it. */
  pass: boolean;
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
  /** What an iced program drew: a PNG of its settled frame, or a GIF when it animated. */
  frame?: { type: string; data: string };
}

export type Output =
  | { kind: "note" }
  /** A program's output, with its status, and the image an iced program drew, as a data URL. */
  | { kind: "term"; chunks: Chunk[]; status: number; note?: string; image?: string }
  /**
   * A program that runs in a universe frame, shown in the frame host, and
   * what it printed. `error` is an edit's compile error, which left it
   * running its last good code.
   */
  | { kind: "frame"; chunks: Chunk[]; status: number; error?: string }
  | { kind: "error"; text: string };

export interface State {
  code: string;
  theme: Theme;
  /** Where the next run goes. */
  where: Where;
  output: Output;
  status: string;
  busy: boolean;
  /** The example the code is, by slug, or "". */
  mark: string;
  /** Whether programs run under the debugger. */
  debugging: boolean;
}

/** rawr-web's API (web/src/lib.rs). */
export interface Web {
  run_text(code: string): string;
}

/** The browser's universe (scripts/universe.ts), as the playground uses it. */
export interface Universe {
  manifest(): Promise<string>;
  frame(parent: Element): Promise<Frame>;
  /** `started` runs once the program compiled and waits at its start, before `main` goes on. */
  run(frame: Frame, code: string, started?: () => Promise<void>): Promise<any>;
  /** The edit `code` for the program running in `frame`, which reloads in place. */
  replace(frame: Frame, code: string): any;
}

interface Widget {
  render(target: string | HTMLElement, options: Record<string, unknown>): string;
  reset(id: string): void;
  remove(id: string): void;
}

/** What the session touches outside itself. */
export interface Deps {
  fetch: typeof fetch;
  location: { pathname: string; search: string; hash: string; origin: string };
  replaceState(url: string): void;
  storage: { getItem(key: string): string | null; setItem(key: string, value: string): void } | null;
  /** Imports WEB_MODULE and initializes it: a fresh copy on each call. */
  loadWeb(): Promise<Web>;
  /** The browser's universe, for snippets that use crates. Without it they run in rawr-web. */
  universe?: Universe;
  /** Where a universe frame goes: the output pane's host for it. */
  frameHost?(): Element | null;
  /** The debugger build of the universe, and the page's side of its debugger. */
  debugging?: { universe: Universe; debugger: Debugger };
  now(): number;
  clipboard(text: string): Promise<void>;
  turnstile(): Widget | undefined;
  /** Adds Turnstile's script to the page. */
  loadTurnstile(): void;
  /** Whether Turnstile's container (#ts) is in the page. */
  hasContainer(): boolean;
  setTimeout(f: () => void, ms: number): unknown;
  clearTimeout(id: unknown): void;
}

export const TURNSTILE_JS = "https://challenges.cloudflare.com/turnstile/v0/api.js?render=explicit";

/** The latest snippet stays in this browser, as on the Rust Playground. */
const DRAFT = "rawr-play-draft";

export class Session {
  readonly data: PlayData;
  readonly deps: Deps;
  state: State;
  private listeners = new Set<(state: State) => void>();
  private path: string;
  private chosen: Where;
  private restored: Example | undefined;
  private pass: boolean;
  private widget: string | null = null;
  private token: string | null = null;
  private web: Promise<Web> | null = null;
  private loaded = false;
  private declared: Promise<string[]> | null = null;
  /** The universe frame shown, and the next one, made ahead. */
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
    this.pass = Boolean(data.sitekey && data.pass);
    this.path = location.pathname;
    this.chosen = new URLSearchParams(location.search).get("run") === "server" ? "server" : "browser";
    const s = data.snippet;
    this.state = {
      code: s.code, theme, where: this.chosen,
      output: { kind: "note" }, status: data.shared ? `shared · ${data.shared}` : "ready", busy: false, mark: "",
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
    if (data.sitekey && !this.pass) this.mount();
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

  /** In the browser, an edit runs again after 500 ms. */
  private typed() {
    if (this.state.where !== "browser") return;
    this.deps.clearTimeout(this.typing);
    this.typing = this.deps.setTimeout(() => this.browser(true), 500);
  }

  private remember() {
    this.deps.replaceState(this.path + (this.chosen === "server" ? "?run=server" : "") + (this.state.mark ? "#" + this.state.mark : ""));
  }

  setCode(code: string) {
    if (code === this.state.code) return;
    // An edit makes the code no longer the example's.
    const mark = this.state.mark;
    this.set({ code, mark: "" });
    if (mark) this.remember();
    this.edited();
    this.typed();
  }

  /** The page's theme changed: Turnstile's widget follows it. */
  setTheme(theme: Theme) {
    this.set({ theme });
  }

  /** A visitor's choice of where it runs is remembered in the address. */
  choose(where: Where) {
    this.chosen = where;
    this.set({ where });
    this.remember();
    this.go();
  }

  /** An example from the menu, or "reset" for the first one. */
  pick(value: string) {
    const example = value === "reset" ? this.data.examples[0] : this.data.examples.find((e) => e.slug === value);
    if (!example) return;
    this.fill(example);
    this.set({ mark: example.slug });
    this.save();
    this.path = "/play";
    this.remember();
    this.go();
  }

  go(): Promise<void> {
    return this.state.where === "browser" || this.state.debugging ? this.browser() : this.run();
  }

  /**
   * The debugger icon: run the program again from its start under the
   * debugger, or, turned off, as it ran before.
   */
  debug(on = !this.state.debugging): Promise<void> {
    if (on === this.state.debugging || (on && !this.deps.debugging)) return Promise.resolve();
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

  /** The universe the next frame runs on: the debugger's while it is on. */
  private get universe(): Universe | undefined {
    return this.state.debugging ? this.deps.debugging?.universe : this.deps.universe;
  }

  // Turnstile: the widget mounts only without a valid pass.
  private mount = () => {
    if (this.widget !== null || !this.data.sitekey) return;
    const turnstile = this.deps.turnstile();
    if (!turnstile) {
      this.deps.loadTurnstile();
      this.deps.setTimeout(this.mount, 200);
      return;
    }
    // The container renders with the island; until it is in the page, try again.
    if (!this.deps.hasContainer()) {
      this.deps.setTimeout(this.mount, 50);
      return;
    }
    this.widget = turnstile.render("#ts", {
      sitekey: this.data.sitekey,
      appearance: "interaction-only",
      theme: this.state.theme,
      callback: (t: string) => (this.token = t),
      "expired-callback": () => (this.token = null),
      "refresh-expired": "auto",
    });
  };

  private human(): Promise<string | null> {
    return new Promise((resolve) => {
      if (!this.data.sitekey || this.pass) return resolve(null);
      const start = this.deps.now();
      const wait = () =>
        this.token ? resolve(this.token) : this.deps.now() - start > 15000 ? resolve(null) : this.deps.setTimeout(wait, 150);
      wait();
    });
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

  /** "Run on server": POST /play/render. */
  async run(retry = true): Promise<void> {
    if (this.state.busy) return;
    this.save();
    this.set({ busy: true, status: "running…" });
    const started = this.deps.now();
    try {
      const check = await this.human();
      if (this.data.sitekey && !this.pass && !check) {
        this.show({ kind: "error", text: "Waiting for the browser check; try again in a moment." });
        return;
      }
      const sentPass = this.pass;
      const response = await this.deps.fetch("/play/render", {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify({ ...this.snippet(), turnstile: check }),
      });
      if (response.status === 403 && sentPass && retry) {
        // The pass expired or was refused: fall back to Turnstile and run again.
        this.pass = false;
        this.mount();
        this.set({ busy: false });
        return await this.run(false);
      }
      // The server set a pass cookie: later runs skip the browser check.
      if (response.headers.get("x-play-pass")) this.pass = true;
      const ms = Math.round(this.deps.now() - started);
      const type = response.headers.get("content-type") || "";
      if (response.ok && type.startsWith("application/json")) {
        const result: TextResult = await response.json();
        this.result(result, ms + " ms");
      } else {
        this.show({ kind: "error", text: await response.text() });
        this.set({ status: response.status === 429 ? "slow down" : response.status === 422 ? "error" : "failed" });
      }
    } catch (error) {
      this.show({ kind: "error", text: "Could not reach the playground: " + (error as Error).message });
      this.set({ status: "failed" });
    } finally {
      this.set({ busy: false });
      const turnstile = this.deps.turnstile();
      if (this.widget !== null && turnstile) {
        this.token = null;
        if (this.pass) {
          turnstile.remove(this.widget);
          this.widget = null;
        } else turnstile.reset(this.widget);
      }
    }
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
    // A share's link keeps the run mode; an example's hash does not apply, since its code is the share.
    const link = this.deps.location.origin + "/play/" + id + (this.chosen === "server" ? "?run=server" : "");
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

  /** The server runs what the browser cannot, and says why. */
  private async server(reason: string) {
    await this.run();
    this.set({ status: this.state.status + " · on the server: " + reason });
  }

  /** The crates the universe declares, or none when it has no manifest. */
  private crates(): Promise<string[]> {
    if (!this.deps.universe) return Promise.resolve([]);
    this.declared ??= this.deps.universe.manifest().then(crates, () => ((this.declared = null), []));
    return this.declared;
  }

  /** The next universe frame in `host`, made ahead of its run. */
  private nextFrame(host: Element): Promise<Frame> {
    return (this.next ??= this.universe!.frame(host).catch((error) => {
      this.next = null;
      throw error;
    }));
  }

  /**
   * Run `code`, which uses crates, in a universe frame of its own in place
   * of the output: the program draws there and takes input there. What it
   * prints shows under it, and a compile error in its place.
   */
  private async framed(mine: number, code: string, restarted = ""): Promise<void> {
    const host = this.deps.frameHost?.();
    if (!host) return this.server("the page has no place for the program's window");
    if (!this.next) this.set({ status: this.state.debugging ? "loading the debugger…" : "loading…" });
    let frame: Frame;
    try {
      frame = await this.nextFrame(host);
    } catch {
      if (this.state.debugging) return this.undebuggable();
      return this.server("the universe did not load");
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
    const result = await this.universe!.run(frame, code, attached ? () => debug!.start() : undefined);
    if (mine !== this.runs) return;
    const took = Math.round(this.deps.now() - start) + " ms in the browser";
    if (result.kind === "error") this.result(result, took);
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
        this.show({ ...output, error: answer.text ?? "" });
        this.set({ status: "error · the program runs its last good code" });
        return;
      default:
        this.show({ kind: "frame", chunks: [...output.chunks, ...(answer.output ?? [])], status: output.status });
        this.set({ status: took + " · reloaded in place" });
    }
  }

  /**
   * "Run in browser", the default. An `edit` reloads the program a frame
   * runs in place. A run that a later one replaced shows nothing.
   */
  async browser(edit = false): Promise<void> {
    this.save();
    const mine = ++this.runs;
    const code = this.state.code;
    // Under the debugger every program runs on its universe.
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
    } catch {
      this.set({ where: "server" });
      return this.server("rawr-web did not load");
    }
    if (mine !== this.runs) return;
    const start = this.deps.now();
    let text: string;
    try {
      text = m.run_text(this.state.code);
    } catch {
      // A panic in rawr-web itself traps the module, whose state it may
      // have left half-written: the server runs this program, and the next
      // run loads a fresh copy of the module.
      this.web = null;
      this.loaded = false;
      return this.server("rawr-web stopped");
    }
    const result: TextResult = JSON.parse(text);
    this.result(result, Math.round(this.deps.now() - start) + " ms in the browser");
  }

  /** The debugger's module didn't load: it turns off, and the program runs as before. */
  private async undebuggable(): Promise<void> {
    this.set({ debugging: false });
    await this.browser();
    this.set({ status: this.state.status + " · the debugger did not load" });
  }

  /** What a run gave, from the browser or the server: its output or its report. */
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
   * Once the page has loaded: in the browser, rawr-web loads, so it does not
   * hold up the first paint, and an example the address named runs.
   */
  preload() {
    if (this.state.where !== "browser") return;
    // Code that uses crates warms a universe frame, so its first Run
    // only compiles and runs.
    const host = this.deps.frameHost?.();
    if (this.deps.universe && host) {
      this.crates().then((names) => {
        if (usesCrate(this.state.code, names)) this.nextFrame(host).catch(() => {});
      });
    }
    this.load().then(() => {
      if (this.restored && this.state.where === "browser" && this.runs === 0) this.browser();
    }, () => {});
  }
}

/** A plain program's output: stdout, stderr in its own colour, then the exit status if not 0. */
export function terminal(result: TextResult): Output {
  const image = result.frame ? { image: `data:${result.frame.type};base64,${result.frame.data}` } : {};
  return { kind: "term", chunks: result.output ?? [], status: result.status, note: result.note, ...image };
}
