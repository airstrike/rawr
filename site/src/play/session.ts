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
// The address keeps the page's state: `#<slug>` names the example loaded,
// and `?run=server` the server when the visitor chose it (the browser, the
// default, has no parameter). Both are written with history.replaceState.
// A share's code wins over an example's hash, which a share page drops, and an
// edit to the code drops the hash, since the code no longer is the example's.
// Share's link keeps ?run=server and never has a hash.
//
// The theme is the page's (theme.ts): Turnstile's widget draws in it.

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
}

/** rawr-web's API (web/src/lib.rs). */
export interface Web {
  run_text(code: string): string;
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
  /** Imports WEB_MODULE and initializes it. */
  loadWeb(): Promise<Web>;
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
    this.typing = this.deps.setTimeout(() => this.browser(), 500);
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
    return this.state.where === "browser" ? this.browser() : this.run();
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

  private show(output: Output) {
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

  /** "Run in browser", the default. A run that a later one replaced shows nothing. */
  async browser(): Promise<void> {
    this.save();
    const mine = ++this.runs;
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
    const result: TextResult = JSON.parse(m.run_text(this.state.code));
    this.result(result, Math.round(this.deps.now() - start) + " ms in the browser");
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
