// The browser's universe (#1395): ?play's universe built for wasm32
// (play/universe/web.sh, packed by web/build.sh), whose runner compiles a
// snippet against the generated bindings of iced, ratatui, ratzilla and
// tokio and runs it in the page, with no server round trip per input.
//
// The page compiles the module once. Each run gets an instance of its own
// in a frame of its own, in place of the run strip's output: a program that
// hands its loop to the page (iced's `run` to its web shell, an async loop
// to wasm-bindgen-futures' `spawn_local`, which polls it on the page's
// events, as a ratatui program's loop that awaits its keys from ratzilla's
// backend) keeps that instance, and winit
// allows one event loop per instance. An edit replaces the running
// program's text in its instance, which keeps its state (#1455); only an
// edit that can't reload there, as `rawr run` restarts for one, gets a
// fresh instance. In the frame, iced's shell puts its
// canvas in place of the element `#iced`, and ratzilla's terminal fills the
// body. Keys reach the program only while its frame has focus, which a
// click or Tab gives it, and the frame draws an outline while it does.
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

/** The universe's files, beside rawr-web's (web/build.sh). */
export const UNIVERSE_GLUE = "/web/universe.js";
export const UNIVERSE_WASM = "/web/universe_bg.wasm";
/** The universe's Cargo.toml, which a snippet's `use`s name crates from. */
export const UNIVERSE_MANIFEST = "/web/universe.toml";

/** One instance's API (rawr::universe::browser). */
export interface Instance {
  prepare(): void;
  run(manifest: string, source: string): string;
  replace(source: string): string;
  panic_message(): string | undefined;
}

/** The compiled module and the manifest snippets compile beside. */
export interface Loaded {
  module: WebAssembly.Module;
  manifest: string;
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
}

/**
 * Where the frame's program draws, and how focus shows. ratzilla's terminal
 * (rows of `pre` in a `div`) draws a cell's default colours as a terminal
 * does, light text with no background, so its frame is a dark terminal in
 * both themes.
 */
const FRAME_STYLE = `html, body { margin: 0; height: 100%; overflow: hidden; background: transparent; }
body { font: 13px/1.25 ui-monospace, SFMono-Regular, Menlo, Consolas, monospace; color: CanvasText; }
body:has(> div > pre) { background: #111; color: #eee; }
pre { margin: 0; font: inherit; }
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
if (navigator.vendor === "Apple Computer, Inc.") {
  Object.defineProperty(navigator, "gpu", { value: undefined, configurable: true });
}
const say = (detail) => frameElement.dispatchEvent(new CustomEvent("universe", { detail }));
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

/** The page's universe: the module compiled once, an instance per frame. */
export function universe({ document, fetch, compile }: UniverseEnv) {
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
    (loaded ??= Promise.all([compile(fetch(UNIVERSE_WASM)), readManifest()]).then(([module, manifest]) => ({
      module,
      manifest,
    })));

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
    element.srcdoc = frameDocument(new URL(UNIVERSE_GLUE, document.baseURI).href);
    parent.append(element);
    const instance = await ready;
    instance.prepare();
    return { element, instance };
  };

  /**
   * Show `frame` and run `code` in it: the server's text result, or a
   * compile error. A panic aborts the instance; its message is the error.
   */
  const run = async (frame: Frame, code: string): Promise<any> => {
    const { manifest } = await load();
    frame.element.hidden = false;
    try {
      return JSON.parse(frame.instance.run(manifest, code));
    } catch (error) {
      const message = frame.instance.panic_message?.() ?? String(error);
      return { kind: "text", status: 101, output: [{ stream: "stderr", text: message + "\n" }] };
    }
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
