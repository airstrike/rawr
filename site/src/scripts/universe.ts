// The browser's universe (#1395): ?play's universe built for wasm32
// (play/universe/web.sh, packed by web/build.sh), whose runner compiles a
// snippet against the generated bindings of iced, ratatui, ratzilla and
// tokio and runs it in the page, with no server round trip per input.
//
// The page compiles the module once. Each run gets an instance of its own
// in a frame of its own, in place of the run strip's output: a program that
// hands its loop to the page keeps that instance, and winit allows one
// event loop per instance. iced's `run` hands it to its web shell.
// ratzilla's `draw_web` calls a closure of the program's on every
// animation frame, and its key callback changes the state that closure
// draws. wasm-bindgen-futures' `spawn_local` polls an async loop of the
// program's on the page's events. An edit replaces the running program's
// text in its instance, which keeps its state (#1455), and the next call
// from the page runs the new code; only an edit that can't reload there,
// as `rawr run` restarts for one, gets a fresh instance. In the frame,
// iced's shell puts its
// canvas in place of the element `#iced`, and ratzilla's terminal fills the
// body. Keys reach the program only while its frame has focus, which a
// click or Tab gives it, and the frame draws an outline while it does.
//
// A `main` that loops in plain Rust and sleeps keeps its instance too: its
// run starts `main` on a stack each `std::thread::sleep` suspends (JSPI,
// `WebAssembly.promising`), so the page's events and the program's edits
// run while it waits. What it prints from then on goes to a terminal in
// its frame, which redraws in place from the cursor and erase escapes the
// program writes (lib/terminal.ts). A browser without JSPI gets stand-ins
// in the frame: every call runs to its end, as before JSPI, and only a
// sleep on `main`'s stack stops the program, which says so in its frame.
//
// On Apple's WebKit (Safari, and every browser on iOS) the frame hides
// `navigator.gpu` before the program starts, so wgpu draws with WebGL2:
// WebKit's WebGPU fails a command buffer that copies twice out of one
// still-mapped buffer, which is how wgpu's StagingBelt uploads iced's quads
// and glyphs, and the canvas stays black.
//
// The program's window is sized in device pixels: winit reads the canvas's
// `devicePixelContentBoxSize`. Chrome's device emulation (DevTools, and
// Playwright's `deviceScaleFactor`) sets `devicePixelRatio` but leaves that
// size in CSS pixels, so there iced draws at twice the size it hit-tests
// (#1449). A real display at that scale doesn't: check scale 2 in a Chrome
// started with `--force-device-scale-factor=2` and no viewport override.
//
// The module comes in two builds (web/build.sh): PLAIN, which every run
// uses, and DEBUGGER, the same universe with rawr's browser debugger, whose
// instances also export `debug` and `debug_events` (D1071). Each build is
// its own `universe`, which fetches and compiles its module the first time
// it makes a frame.

import { Screen } from "../lib/terminal.ts";

/**
 * What a program that sleeps on main's stack says in its frame in a
 * browser without JSPI (WebAssembly JS Promise Integration).
 */
export const NO_JSPI = "This browser can't run a program that sleeps: it has no WebAssembly JS Promise Integration (JSPI).";

/** The universe's files, beside rawr-web's (web/build.sh). */
export const UNIVERSE_GLUE = "/web/universe.js";
export const UNIVERSE_WASM = "/web/universe_bg.wasm";

/** A build of the universe's module: its glue and its wasm. */
export interface Build {
  glue: string;
  wasm: string;
}

/** The module ?play runs every program on. */
export const PLAIN: Build = { glue: UNIVERSE_GLUE, wasm: UNIVERSE_WASM };

/**
 * The same universe built with rawr's browser debugger (D1071), which the
 * playground loads only when its debugger is turned on: every program it
 * runs is under a debugger the page asks through `debug`.
 */
export const DEBUGGER: Build = { glue: "/web/universe-debugger.js", wasm: "/web/universe-debugger_bg.wasm" };
/** The universe's Cargo.toml, which a snippet's `use`s name crates from. */
export const UNIVERSE_MANIFEST = "/web/universe.toml";

/** One instance's API (rawr::universe::browser). */
export interface Instance {
  prepare(): void;
  /** The answer once `main` returns; meanwhile `sleeping` is true. */
  run(manifest: string, source: string): Promise<string>;
  sleeping(): boolean;
  set_output(sink: (stream: string, text: string) => void): void;
  replace(source: string): string;
  panic_message(): string | undefined;
  /** The debugger build's (rawr::universe::debugger): one request's answer, as JSON text. */
  debug?(request: string): Promise<string>;
  /** The debugger build's: each event of its debugger, as JSON text. */
  debug_events?(listener: (event: string) => void): void;
}

/** The compiled module and the manifest snippets compile beside. */
export interface Loaded {
  module: WebAssembly.Module;
  manifest: string;
}

/**
 * A run under the debugger (session.ts): `started` once the program
 * compiled and waits at its held start, `ended` with its exit status once
 * `main` returns or panics.
 */
export interface Debugged {
  started(): Promise<void>;
  ended(status: number): void;
}

/** A frame with its own instance, its bindings installed, ready to run. */
export interface Frame {
  element: HTMLIFrameElement;
  instance: Instance;
}

/** What the universe touches, so a test can hand it stand-ins. */
export interface UniverseEnv {
  document: Document;
  fetch: typeof fetch;
  compile(response: Promise<Response>): Promise<WebAssembly.Module>;
  /** Which build of the module its frames run: PLAIN unless named. */
  build?: Build;
}

/**
 * Where the frame's program draws, and how focus shows. ratzilla's terminal
 * (rows of `pre` in a `div`) draws a cell's default colours as a terminal
 * does, light text with no background, so its frame is a dark terminal in
 * both themes.
 */
const FRAME_STYLE = `html, body { margin: 0; height: 100%; overflow: hidden; background: transparent; }
body { font: 13px/1.25 ui-monospace, SFMono-Regular, Menlo, Consolas, monospace; color: CanvasText; }
body:has(> div > pre), body:has(> pre.terminal) { background: #111; color: #eee; }
pre { margin: 0; font: inherit; }
pre.terminal { padding: 8px 12px; }
:focus { outline: none; }
canvas:focus-visible, [tabindex]:focus { outline: 2px solid #d43; outline-offset: -2px; }`;

/**
 * The frame's document: it imports the glue, takes the compiled module from
 * the page, instantiates it, and hands the page the instance.
 */
export function frameDocument(glue: string): string {
  return `<!doctype html><html><head><meta charset="utf-8"><style>${FRAME_STYLE}</style></head>
<body><div id="iced"></div><script type="module">
import * as glue from ${JSON.stringify(glue)};
// Without JSPI nothing suspends: a call runs to its end, and a sleep on
// main's stack refuses with NO_JSPI, which stops the program there.
if (typeof WebAssembly.Suspending !== "function") {
  WebAssembly.Suspending = function () {
    return () => { throw ${JSON.stringify(NO_JSPI)}; };
  };
  WebAssembly.promising = (call) => (...args) => new Promise((resolve) => resolve(call(...args)));
}
if (navigator.vendor === "Apple Computer, Inc.") {
  Object.defineProperty(navigator, "gpu", { value: undefined, configurable: true });
}
const say = (detail) => frameElement.dispatchEvent(new CustomEvent("universe", { detail }));
// A key the focused program reads is its own: the browser's default for it,
// scrolling the frame or the page around it, doesn't run. The program's
// listeners have already seen it. Tab still moves focus.
const scrolls = new Set(["ArrowUp", "ArrowDown", "ArrowLeft", "ArrowRight", " ", "PageUp", "PageDown", "Home", "End"]);
addEventListener("keydown", (event) => {
  if (document.activeElement !== document.body && scrolls.has(event.key)) event.preventDefault();
});
addEventListener("message", (event) => {
  glue.default({ module_or_path: event.data }).then(
    () => say({ instance: glue }),
    (error) => say({ error: String(error) }),
  );
}, { once: true });
say({ listening: true });
</script></body></html>`;
}

/**
 * The crates `manifest` declares in its dependency tables, a target's own
 * among them, by the name a snippet's paths give them (`-` read as `_`).
 */
export function crates(manifest: string): string[] {
  const names = new Set<string>();
  let dependencies = false;
  for (const line of manifest.split("\n")) {
    const table = line.match(/^\s*\[([^\]]*)\]/);
    if (table) {
      dependencies = /(^|\.)dependencies$/.test(table[1].trim());
      continue;
    }
    const key = dependencies ? line.match(/^\s*([A-Za-z0-9_-]+)\s*=/) : null;
    if (key) names.add(key[1].replaceAll("-", "_"));
  }
  return [...names];
}

/** Whether `code` names one of `crates`: a path that starts at it, or `extern crate`. */
export function usesCrate(code: string, crates: string[]): boolean {
  return crates.some((name) =>
    new RegExp(`(^|[^\\w:])(::)?${name}::|\\bextern\\s+crate\\s+${name}\\b|\\buse\\s+${name}\\s*;`).test(code),
  );
}

/**
 * Once a frame just shown has had its `resize`. Shown, a hidden frame's
 * window changes size, and the browser fires `resize` in the window's next
 * rendering update, before its animation frame callbacks. A program that
 * starts before then sees that `resize` after its first draw: ratzilla's
 * DomBackend then builds its grid again, empty and without its key
 * listener, on the next draw, and ratatui draws only the cells that
 * changed into it. Reading the window's size lays the frame out at its new
 * size now, so that `resize` fires in the next rendering update.
 */
function settled(element: HTMLIFrameElement): Promise<void> {
  const window = element.contentWindow;
  if (!window) return Promise.resolve();
  void window.innerWidth;
  return new Promise((resolve) => window.requestAnimationFrame(() => resolve()));
}

/**
 * Whether `body`, a frame's, shows nothing: no element but its scripts and
 * the empty place iced draws in.
 */
export function drewNothing(body: Element): boolean {
  return [...body.children].every((child) => child.tagName === "SCRIPT" || (child.id === "iced" && !child.hasChildNodes()));
}

/**
 * Mark `frame`, whose program has ended, `data-blank` when it drew
 * nothing, until something shows in it. A phone's layout folds a blank
 * frame away, so what the program printed sits at the top of the output
 * (play.css). A program that is still running is never marked: a frame
 * that appears later resizes under it.
 */
function markBlank(frame: Frame): void {
  const page = frame.element.contentDocument;
  if (!page?.body || !drewNothing(page.body)) return;
  frame.element.dataset.blank = "";
  const watch = new MutationObserver(() => {
    if (drewNothing(page.body)) return;
    delete frame.element.dataset.blank;
    watch.disconnect();
  });
  watch.observe(page.body, { childList: true, subtree: true });
}

/** An empty terminal in `frame`. */
function terminalIn(frame: Frame): HTMLPreElement {
  const page = frame.element.contentWindow!.document;
  const pre = page.createElement("pre");
  pre.className = "terminal";
  page.body.append(pre);
  return pre;
}

/**
 * A terminal in `frame` that shows what its program prints from now on,
 * and what it printed before, drawn once per animation frame.
 */
function terminal(frame: Frame): void {
  const window = frame.element.contentWindow!;
  const page = window.document;
  const pre = terminalIn(frame);
  const screen = new Screen();
  let drawing = false;
  frame.instance.set_output((_stream, text) => {
    screen.write(text);
    if (drawing) return;
    drawing = true;
    window.requestAnimationFrame(() => {
      drawing = false;
      pre.textContent = screen.text();
      // A program that appends shows its latest rows.
      page.scrollingElement?.scrollTo(0, page.scrollingElement.scrollHeight);
    });
  });
}

/** The page's universe: the module compiled once, an instance per frame. */
export function universe({ document, fetch, compile, build = PLAIN }: UniverseEnv) {
  // The manifest alone, for a page that asks which snippets use crates
  // before it fetches the module. A fetch that failed is tried again.
  let manifest: Promise<string> | null = null;
  const readManifest = () =>
    (manifest ??= fetch(UNIVERSE_MANIFEST)
      .then((response) => {
        if (!response.ok) throw new Error(`the universe's manifest answered ${response.status}`);
        return response.text();
      })
      .catch((error) => {
        manifest = null;
        throw error;
      }));
  let loaded: Promise<Loaded> | null = null;
  const load = () =>
    (loaded ??= Promise.all([compile(fetch(build.wasm)), readManifest()]).then(
      ([module, manifest]) => ({ module, manifest }),
      // A module that didn't load is fetched again by the next frame.
      (error) => {
        loaded = null;
        throw error;
      },
    ));

  /** A new frame in `parent`, hidden until shown, with its instance prepared. */
  const frame = async (parent: Element): Promise<Frame> => {
    const { module } = await load();
    const element = document.createElement("iframe");
    element.className = "universe";
    element.title = "The program's window";
    element.hidden = true;
    const ready = new Promise<Instance>((resolve, reject) => {
      element.addEventListener("universe", (event) => {
        const detail = (event as CustomEvent).detail;
        if (detail.listening) element.contentWindow!.postMessage(module, "*");
        else if (detail.instance) resolve(detail.instance);
        else reject(new Error(detail.error));
      });
    });
    element.srcdoc = frameDocument(new URL(build.glue, document.baseURI).href);
    parent.append(element);
    const instance = await ready;
    instance.prepare();
    return { element, instance };
  };

  /**
   * Show `frame` and run `code` in it: the server's text result, or a
   * compile error. A panic aborts the instance; its message is the error.
   * A `main` that sleeps runs on in the frame and prints to its terminal:
   * the answer is then a status of 0 with no output. Without JSPI it stops
   * at its first sleep, and its frame says why. A run `debugged` is held at
   * its start until `started` settles, and tells `ended` its status.
   */
  const run = async (frame: Frame, code: string, debugged?: Debugged): Promise<any> => {
    const { manifest } = await load();
    frame.element.hidden = false;
    await settled(frame.element);
    try {
      const answer = frame.instance.run(manifest, code);
      if (debugged) return await heldRun(frame, answer, debugged);
      if (frame.instance.sleeping()) {
        terminal(frame);
        // A panic's message goes to the terminal.
        answer.catch(() => {});
        return { kind: "text", status: 0, output: [] };
      }
      const done = JSON.parse(await answer);
      markBlank(frame);
      return done;
    } catch (error) {
      const message = frame.instance.panic_message?.() ?? String(error);
      // A browser that can't suspend the program says so in its frame.
      if (message === NO_JSPI) {
        terminalIn(frame).textContent = message;
        return { kind: "text", status: 101, output: [] };
      }
      debugged?.ended(101);
      markBlank(frame);
      return { kind: "text", status: 101, output: [{ stream: "stderr", text: message + "\n" }] };
    }
  };

  /**
   * The rest of a held run, whose `answer` the release lets settle.
   * `sleeping` can't say whether it runs on: a hold in the program's
   * top-level items comes before `main` starts. A program that compiled
   * and runs to its end does so in the microtasks the release starts, so
   * its answer settles before the next task; one that sleeps or pauses
   * leaves its answer pending, runs on and prints to its terminal.
   */
  const heldRun = async (frame: Frame, answer: Promise<string>, debugged: Debugged): Promise<any> => {
    await debugged.started();
    const next = new Promise<null>((resolve) => setTimeout(() => resolve(null), 0));
    const done = await Promise.race([answer.then((text) => JSON.parse(text)), next]);
    if (done) {
      if (done.kind !== "error") debugged.ended(done.status);
      markBlank(frame);
      return done;
    }
    terminal(frame);
    // A panic's message goes to the terminal.
    answer.then(
      (text) => debugged.ended(JSON.parse(text).status),
      () => debugged.ended(101),
    );
    return { kind: "text", status: 0, output: [] };
  };

  /**
   * Give the program running in `frame` `code` as its new text, which it
   * takes at once with its state kept: "swapped" with what it printed
   * since, "unchanged", "error" for a compile error, which leaves it
   * running, or "restart" with a reason, when the code needs a fresh frame.
   * A panic aborts the instance, which then needs one too.
   */
  const replace = (frame: Frame, code: string): any => {
    try {
      return JSON.parse(frame.instance.replace(code));
    } catch (error) {
      return { kind: "restart", reason: frame.instance.panic_message?.() ?? String(error) };
    }
  };

  return { manifest: readManifest, load, frame, run, replace };
}
