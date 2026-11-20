// Run buttons for the site's runnable snippets, on the landing page and in
// the docs. Each snippet is editable in place. Its Run button sits in a
// terminal strip and shows what the program printed. Without JS the code
// stays read-only, the buttons stay hidden and the strips keep their static
// output.
//
// Every snippet runs in the browser, and only there. A plain snippet runs in
// rawr-web, the playground's module, which loads at the page's first run. A
// run that rawr-web can't start says why and stops. A run that traps rawr-web
// says so too, and the next run loads a fresh copy.
//
// A block marked `data-where="universe"` uses crates (iced, ratatui): it
// runs in the browser's universe (universe.ts), in a frame of its own in
// place of its output, and stays interactive there. Its output before Run
// is what the build wrote (docs.ts's runOutputs). Once its program runs, an
// edit reloads it in its frame with its state kept (#1455): a compile error
// shows after the frame while the program runs its last good code, and an
// edit that can't reload there runs in a fresh frame. A universe that
// doesn't load says so in place of the output.
//
// A page that asks for it (`warm`) warms its snippets once it has loaded, so
// Run answers at once: rawr-web is fetched and instantiated, each plain
// snippet runs unseen, and each universe block gets its frame. The first Run
// of an unedited snippet shows that run; a later Run, or one after an edit,
// runs it again.
//
// After a run the play icon becomes an ×, which clears the output. A Reset
// button at the code's top-right shows while the code differs from the
// original, and restores it. An edited snippet is highlighted again when it
// loses focus, by the build's highlighter, loaded only then.

import { outputHtml } from "../lib/output.ts";
import type { Frame } from "./universe.ts";

/** rawr-web's API (web/src/lib.rs), as the playground uses it. */
export interface Web {
  run_text(code: string): string;
}

/** The browser's universe (universe.ts), as a run strip uses it. */
export interface Universe {
  frame(parent: Element): Promise<Frame>;
  run(frame: Frame, code: string): Promise<any>;
  replace(frame: Frame, code: string): any;
}

/** What the script touches, so a test can hand it a stand-in DOM. */
export interface Env {
  document: Document;
  window: Window;
  /** Imports rawr-web and initializes it: a fresh copy on each call. */
  web: () => Promise<Web>;
  /** The browser's universe. Without it a universe block says it can't run here. */
  universe?: Universe;
  /** Warm the snippets once the page has loaded, so Run answers at once. */
  warm?: boolean;
}

/** The fewest pixels a universe block's frame is tall: iced's counter, or fourteen rows of text. */
const FRAME_HEIGHT = 400;

/**
 * A run started ahead of its Run button: `warm` starts it, and the next
 * `take` answers with it. Each later `take` runs again, and so does one
 * whose warmed run gave nothing or threw.
 */
export function ahead(run: () => Promise<any>): { warm(): Promise<any>; take(): Promise<any> } {
  let early: Promise<any> | null = null;
  return {
    warm: () => (early ??= run()),
    take: async () => {
      const warmed = await early?.catch(() => null);
      early = null;
      return warmed ?? run();
    },
  };
}

/** An error's message. */
const why = (error: unknown) => (error instanceof Error ? error.message : String(error));

export function setup({ document, window, web, universe, warm = false }: Env): void {
  const SVG = "http://www.w3.org/2000/svg";
  const icon = (cls: string, d: string) => {
    const svg = document.createElementNS(SVG, "svg");
    svg.setAttribute("viewBox", "0 0 12 12");
    svg.setAttribute("aria-hidden", "true");
    svg.setAttribute("focusable", "false");
    svg.classList.add(cls);
    const path = document.createElementNS(SVG, "path");
    path.setAttribute("d", d);
    path.setAttribute("fill", "currentColor");
    svg.append(path);
    return svg;
  };

  // rawr-web, loaded once for the page. A load that fails is tried again at
  // the next run. A run that traps it may have left its state half-written,
  // so the next run loads a fresh copy.
  let module: Promise<Web> | null = null;
  const browser = async (code: string): Promise<any> => {
    let m: Web;
    try {
      m = await (module ??= web());
    } catch (error) {
      module = null;
      throw new Error("rawr did not load in this browser: " + why(error));
    }
    let text: string;
    try {
      text = m.run_text(code);
    } catch (error) {
      module = null;
      throw new Error("rawr stopped in the browser: " + why(error) + ". The next run loads it again.");
    }
    return JSON.parse(text);
  };

  // What warming does, once the page has loaded: one task per snippet.
  const warmups: (() => Promise<unknown>)[] = [];

  // Plain-text editing where the browser has it. Elsewhere a paste is
  // reduced to its text.
  const probe = document.createElement("div");
  probe.contentEditable = "plaintext-only";
  const PLAIN = probe.contentEditable === "plaintext-only";

  for (const button of document.querySelectorAll<HTMLButtonElement>(".runbtn")) {
    const out = document.getElementById(button.dataset.out ?? "");
    const source = document.getElementById(button.dataset.source ?? "");
    if (!out || !source) continue;
    const strip = out.closest(".run")!;
    const name = source.dataset.run || "this snippet";
    const inUniverse = source.dataset.where === "universe";
    const label = button.getAttribute("aria-label") || `Run ${name}`;
    const original = source.textContent ?? "";
    const originalHtml = source.innerHTML;
    const staticOut = [...out.childNodes].map((node) => node.cloneNode(true));
    const staticClass = out.className;

    // The code: an editable text box.
    source.contentEditable = PLAIN ? "plaintext-only" : "true";
    source.spellcheck = false;
    source.setAttribute("autocapitalize", "off");
    source.setAttribute("autocorrect", "off");
    source.setAttribute("role", "textbox");
    source.setAttribute("aria-multiline", "true");
    source.setAttribute("aria-label", `Code of ${name}. Edit it, then run it with Control or Command and Enter.`);
    if (!PLAIN) {
      source.addEventListener("paste", (e) => {
        e.preventDefault();
        document.execCommand("insertText", false, (e as ClipboardEvent).clipboardData?.getData("text/plain") ?? "");
      });
    }
    // innerText reads the line breaks an edit inserts as elements.
    const baseline = source.innerText;
    const text = () => (source.innerText === baseline ? original : source.innerText.replace(/\n$/, ""));
    const edited = () => text() !== original;

    // Reset: shown while the code differs from the original.
    const reset = document.createElement("button");
    reset.type = "button";
    reset.className = "resetbtn";
    reset.textContent = "Reset";
    reset.setAttribute("aria-label", `Restore the original ${name}`);
    reset.title = `Restore the original ${name}`;
    reset.hidden = true;
    // It belongs to the code, so it sits at the code block's top-right corner.
    source.before(reset);

    // While it runs, the icon is three dots: there is nothing to stop. After
    // it, the icon is an ×.
    button.replaceChildren(
      icon("play", "M2 1L11 6L2 11Z"),
      icon("busy", "M1 5H3V7H1ZM5 5H7V7H5ZM9 5H11V7H9Z"),
      icon("clear", "M2.5 1.1L6 4.6L9.5 1.1L10.9 2.5L7.4 6L10.9 9.5L9.5 10.9L6 7.4L2.5 10.9L1.1 9.5L4.6 6L1.1 2.5Z"),
    );
    button.hidden = false;
    strip.classList.add("has-run");
    // The static output keeps its space but stays hidden until Run, so the
    // strip doesn't change height when the same output arrives.
    out.classList.add("pending");

    type State = "idle" | "running" | "done";
    let state: State = "idle";
    const setState = (next: State) => {
      state = next;
      button.dataset.state = next;
      const say = { idle: label, running: `Running ${name}…`, done: `Clear the output of ${name}` }[next];
      button.setAttribute("aria-label", say);
      button.title = say;
      if (next === "running") button.setAttribute("aria-disabled", "true");
      else button.removeAttribute("aria-disabled");
    };
    setState("idle");

    let highlighted = true;
    const highlight = () => {
      if (highlighted) return;
      const code = text();
      highlighted = true;
      source.textContent = code;
      import("../lib/highlight.ts").then(({ highlightRust }) => {
        // Only if nothing was typed while the highlighter loaded.
        if (highlighted && source.textContent === code && document.activeElement !== source) source.innerHTML = highlightRust(code);
      }, () => {});
    };
    const restore = () => {
      source.innerHTML = originalHtml;
      highlighted = true;
      reset.hidden = true;
    };
    // A universe block's next frame, made ahead of its Run by warming, and
    // again after each run is cleared, so a Run only compiles and runs.
    let ready: Promise<Frame> | null = null;
    const nextFrame = () => (ready ??= universe!.frame(out));
    // The frame whose program runs, which an edit reloads in place with its
    // state kept (#1455), the edit it refused last, and the pause after
    // typing before the reload.
    let live: Frame | null = null;
    let refused: string | null = null;
    let typing: number | undefined;
    const clear = () => {
      live = null;
      out.replaceChildren(...staticOut.map((node) => node.cloneNode(true)));
      out.className = staticClass;
      out.classList.add("pending");
      ready = null;
      if (inUniverse && universe && warm) nextFrame().catch(() => {});
    };

    // An edit to a program that runs in a frame reloads it after a pause.
    const edit = () => {
      window.clearTimeout(typing);
      typing = window.setTimeout(reload, 500);
    };
    source.addEventListener("input", () => {
      highlighted = false;
      reset.hidden = !edited();
      if (live) edit();
      else if (state === "done") setState("idle");
    });
    source.addEventListener("blur", highlight);
    source.addEventListener("keydown", (e) => {
      if (e.key === "Enter" && (e.metaKey || e.ctrlKey)) {
        e.preventDefault();
        if (state !== "running") run();
      }
    });
    reset.addEventListener("click", () => {
      restore();
      if (live) edit();
      else if (state === "done") setState("idle");
      source.focus();
    });

    const show = (html: string) => {
      const live = document.createElement("span");
      live.className = "live";
      live.innerHTML = html;
      out.replaceChildren(live);
      out.classList.remove("pending", "quiet");
    };
    const fail = (message: string) => show(outputHtml({ kind: "error", text: message }));

    // Run `code` in a universe frame in place of the output, as tall as the
    // output it replaces: what it prints follows the frame. A compile error
    // replaces the frame.
    const runFrame = async (code: string) => {
      const frame = await nextFrame();
      ready = null;
      out.classList.add("framed");
      const height = Math.max(out.getBoundingClientRect().height, FRAME_HEIGHT);
      for (const node of [...out.childNodes]) if (node !== frame.element) node.remove();
      out.classList.remove("pending", "quiet");
      frame.element.style.height = `${height}px`;
      const result = await universe!.run(frame, code);
      if (result.kind === "error") {
        frame.element.remove();
        show(outputHtml(result));
        return;
      }
      live = frame;
      refused = null;
      if ((result.output ?? []).length > 0 || result.status !== 0) append(outputHtml({ ...result, frame: undefined }));
    };
    /** `html` after the frame, as what the program printed; `error` marks an edit's compile error. */
    const append = (html: string, error = false) => {
      const span = document.createElement("span");
      span.className = error ? "live refused" : "live";
      span.innerHTML = html;
      out.append(span);
    };
    const unrefuse = () => out.querySelector(":scope > .refused")?.remove();

    // Reload the program the frame runs with the edited code. A compile
    // error shows after the frame while the program runs its last good
    // code; an edit that can't reload there runs in a fresh frame.
    const reload = () => {
      if (!live || state === "running") return;
      const code = text();
      const answer = universe!.replace(live, code);
      switch (answer.kind) {
        case "restart":
          run();
          return;
        case "unchanged":
          if (code !== refused) unrefuse();
          return;
        case "error":
          refused = code;
          unrefuse();
          append(outputHtml({ kind: "error", text: answer.text }), true);
          return;
        default:
          unrefuse();
          if ((answer.output ?? []).length > 0) append(outputHtml({ kind: "text", status: 0, output: answer.output }));
      }
    };

    const run = async () => {
      live = null;
      setState("running");
      // In a compare slider the output is on the right-hand file, so show it.
      button.closest(".compare")?.querySelector<HTMLButtonElement>('.tabs button[data-to="0"]')?.click();
      const code = text();
      if (inUniverse) {
        if (!universe) {
          fail("This program runs on the browser's prebuilt runner, which this page doesn't load.");
        } else {
          try {
            await runFrame(code);
          } catch (error) {
            console.warn("rawr: the browser's prebuilt runner could not run this snippet;", error);
            clear();
            fail("The browser's prebuilt runner did not load: " + why(error));
          }
        }
        setState("done");
        return;
      }
      try {
        // The build draws each snippet's static output with the same function.
        show(outputHtml(edited() ? await browser(code) : await early.take()));
      } catch (error) {
        fail(why(error));
      }
      setState("done");
    };

    // The unedited snippet's run, which warming starts ahead of its Run.
    const early = ahead(() => browser(original));
    // A universe block warms its frame: the module compiled and an
    // instance made, with its bindings installed.
    warmups.push(async () => {
      if (inUniverse) {
        if (universe) await nextFrame();
      } else if (!edited()) await early.warm();
    });

    button.addEventListener("click", () => {
      if (state === "running") return;
      if (state === "done") {
        clear();
        setState("idle");
        return;
      }
      highlight();
      run();
    });
  }

  // One snippet at a time, after the page's own loading, so warming never
  // competes with it. A task that fails leaves its Run to do the work.
  const start = async () => {
    for (const task of warmups) await task().catch(() => {});
  };
  if (warm && warmups.length > 0) {
    const later = () => setTimeout(start, 0);
    if (document.readyState === "complete") later();
    else window.addEventListener("load", later, { once: true });
  }
}
