// Run buttons for the site's runnable snippets, on the landing page and in
// the docs. Each snippet is editable in place. Its Run button sits in a
// terminal strip and shows what the program printed. Without JS the code
// stays read-only, the buttons stay hidden and the strips keep their static
// output.
//
// A snippet runs in the browser, through rawr-web, the playground's module,
// which loads at the page's first run. A block marked `data-where="server"`
// runs on rawr's server instead, for what a page's one thread can't do yet,
// such as spawning a thread. The server also runs a snippet when rawr-web
// doesn't load or stops.
//
// A block marked `data-where="universe"` uses crates (iced, ratatui): it
// runs in the browser's universe (universe.ts), in a frame of its own in
// place of its output, and stays interactive there. Its output before Run
// is the frame the server drew, as the build wrote it. The server runs it
// when the universe doesn't load.
//
// On the server, the site's own text goes to POST /play/run, which accepts
// only the exact sources on the site's pages and caches their results. Edited
// text goes to POST /play/render, the playground's endpoint, with the
// playground's checks: its pass cookie or a Turnstile token, and its per-IP
// limits.
//
// A page that asks for it (`warm`) warms its snippets once it has loaded, so
// Run answers at once: rawr-web is fetched and instantiated, and each
// snippet runs unseen, in rawr-web or through /play/run. The first Run of an
// unedited snippet shows that run; a later Run, or one after an edit, runs
// it again.
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
}

/** What the script touches, so a test can hand it a stand-in DOM. */
export interface Env {
  document: Document;
  window: Window & { turnstile?: any };
  fetch: typeof fetch;
  /** Imports rawr-web and initializes it. Without it every run goes to the server. */
  web?: () => Promise<Web>;
  /** The browser's universe. Without it a universe block runs on the server. */
  universe?: Universe;
  /** Warm the snippets once the page has loaded, so Run answers at once. */
  warm?: boolean;
}

/** The fewest pixels a universe block's frame is tall: iced's counter, or fourteen rows of text. */
const FRAME_HEIGHT = 400;

const TURNSTILE_JS = "https://challenges.cloudflare.com/turnstile/v0/api.js?render=explicit";

/**
 * A run started ahead of its Run button: `warm` starts it, and the next
 * `take` answers with it. Each later `take` runs again, and so does one
 * whose warmed run gave nothing or failed.
 */
export function ahead(run: () => Promise<any>): { warm(): Promise<any>; take(): Promise<any> } {
  let early: Promise<any> | null = null;
  return {
    warm: () => (early ??= run()),
    take: async () => {
      const warmed = await early?.catch(() => null);
      early = null;
      return warmed && warmed.failed === undefined ? warmed : run();
    },
  };
}

export function setup({ document, window, fetch, web, universe, warm = false }: Env): void {
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

  // The playground's browser check, loaded only for an edited run.
  type Gate = { sitekey: string | null; pass: boolean };
  let gate: Promise<Gate> | null = null;
  const gateInfo = () =>
    (gate ??= fetch("/play/gate", { cache: "no-store" })
      .then((r) => (r.ok ? r.json() : { sitekey: null, pass: false }))
      .catch(() => ({ sitekey: null, pass: false })));
  let script: Promise<unknown> | null = null;
  const loadTurnstile = () =>
    (script ??= new Promise((resolve, reject) => {
      const js = document.createElement("script");
      js.src = TURNSTILE_JS;
      js.async = true;
      js.onload = resolve;
      js.onerror = () => reject(new Error("the browser check could not load"));
      document.head.append(js);
    }));
  // A Turnstile token, or null when no check is needed. The widget stays
  // invisible unless Cloudflare asks for an interaction, shown under `strip`.
  const token = async (strip: Element): Promise<string | null> => {
    const info = await gateInfo();
    if (!info.sitekey || info.pass) return null;
    await loadTurnstile();
    const box = document.createElement("div");
    box.className = "runcheck";
    strip.after(box);
    try {
      return await new Promise<string>((resolve, reject) => {
        const timer = setTimeout(() => reject(new Error("the browser check timed out")), 60000);
        window.turnstile.render(box, {
          sitekey: info.sitekey,
          appearance: "interaction-only",
          theme: document.documentElement?.dataset.theme === "light" ? "light" : "dark",
          callback: (t: string) => (clearTimeout(timer), resolve(t)),
          "error-callback": () => (clearTimeout(timer), reject(new Error("the browser check failed"))),
        });
      });
    } finally {
      box.remove();
    }
  };
  // POST an edited source to the playground's endpoint. A refused pass falls
  // back to Turnstile once.
  const render = async (code: string, strip: Element) => {
    const send = async () =>
      fetch("/play/render", {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify({ code, turnstile: await token(strip) }),
      });
    let response = await send();
    const info = await gateInfo();
    if (response.status === 403 && info.pass && info.sitekey) {
      info.pass = false;
      response = await send();
    }
    if (response.headers.get("x-play-pass") === "set") info.pass = true;
    return response;
  };

  // rawr-web, loaded once for the page. A load that fails, or a run that
  // traps, sends this run and later ones to the server.
  let module: Promise<Web> | null = null;
  let broken = !web;
  const browser = async (code: string): Promise<any> => {
    if (broken) return null;
    try {
      const m = await (module ??= web!());
      return JSON.parse(m.run_text(code));
    } catch {
      broken = true;
      return null;
    }
  };

  // A run of the site's own text on the server. 422 is a program that
  // doesn't compile, and its text is rawr's report.
  const answer = async (response: Response): Promise<any> => {
    const type = response.headers.get("content-type") || "";
    if (response.ok && type.startsWith("application/json")) return response.json();
    if (response.status === 422) return { kind: "error", text: await response.text() };
    return { failed: (await response.text()).trim().slice(0, 2000) };
  };
  const siteRun = (code: string) =>
    fetch("/play/run", {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ code }),
    }).then(answer);

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
    const onServer = source.dataset.where === "server";
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
    const clear = () => {
      out.replaceChildren(...staticOut.map((node) => node.cloneNode(true)));
      out.className = staticClass;
      out.classList.add("pending");
      ready = null;
      if (inUniverse && universe && warm) nextFrame().catch(() => {});
    };

    source.addEventListener("input", () => {
      highlighted = false;
      reset.hidden = !edited();
      if (state === "done") setState("idle");
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
      if (state === "done") setState("idle");
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
      if ((result.output ?? []).length > 0 || result.status !== 0) {
        const live = document.createElement("span");
        live.className = "live";
        live.innerHTML = outputHtml({ ...result, frame: undefined });
        out.append(live);
      }
    };

    const run = async () => {
      setState("running");
      // In a compare slider the output is on the right-hand file, so show it.
      button.closest(".compare")?.querySelector<HTMLButtonElement>('.tabs button[data-to="0"]')?.click();
      const code = text();
      if (inUniverse && universe) {
        try {
          await runFrame(code);
          setState("done");
          return;
        } catch (error) {
          // The universe didn't load: the server runs it.
          console.warn("rawr: the browser's universe could not run this snippet;", error);
          clear();
        }
      }
      try {
        let result = edited() ? (onServer || inUniverse ? null : await browser(code)) : await early.take();
        if (!result) result = edited() ? await answer(await render(code, strip)) : await siteRun(code);
        if (result.failed !== undefined) {
          fail("The code could not run. " + result.failed);
        } else {
          // The build draws each snippet's static output with the same function.
          show(outputHtml(result));
        }
      } catch (error) {
        // fetch throws a TypeError when the network fails.
        const message = error instanceof Error ? error.message : String(error);
        fail(error instanceof TypeError ? "rawr's server could not be reached. " + message : "The code could not run. " + message);
      }
      setState("done");
    };

    // The unedited snippet's run, which warming starts ahead of its Run.
    const early = ahead(() => (onServer || inUniverse ? siteRun(original) : browser(original)));
    // A universe block warms its frame: the module compiled and an
    // instance made, with its bindings installed.
    warmups.push(async () => {
      if (inUniverse && universe) await nextFrame();
      else if (!edited()) await early.warm();
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
