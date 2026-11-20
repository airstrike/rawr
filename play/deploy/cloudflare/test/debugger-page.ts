// The playground's debugger icon (#2304, D1071) in a real headless Chrome,
// driven over the DevTools protocol with Node's own WebSocket:
//   RAWR_IMPORT=target/release/rawr-import web/build.sh && npm run build --prefix site
//   node play/deploy/cloudflare/test/debugger-page.ts [WEB_DIR] [SHOTS_DIR]
// It serves the site's build (site/dist) as the Worker does, with /web from
// WEB_DIR (default target/wasm/site): rawr-web, the plain universe and its
// debugger build (universe-debugger.js and universe-debugger_bg.wasm.br),
// which web/build.sh writes side by side.
//
// On the playground, a `main` that loops and sleeps: a click on the bug
// icon fetches the debugger build, runs the program under it and opens the
// panel. A click on line 2's number sets a breakpoint, and the program
// pauses there, the line highlighted, with its frames and its bindings in
// the panel right of the output. The toolbar's glyphs name their actions
// and keys. The console under the code evaluates at a typed Enter, takes a
// second line at Shift+Enter, and keeps each answer in its scrollback. F10
// steps over a line, the Continue glyph pauses on the next turn, and F8
// lets the loop run on once the breakpoint is cleared. A second click on
// the icon closes the panel and runs the program as before, and a third
// opens it again without fetching the debugger build's module again. A map
// binding opens to one row per key and value. Dragging the split bars
// resizes the code, the debugger and the console, and a reload keeps the
// sizes; a phone draws no bars. It
// saves window screenshots in SHOTS_DIR (default target/web-check): the
// paused debugger in the dark and light themes at 1440 wide and on a
// phone. CHROME names the browser when it is not in the usual place.
//
// PLAY_URL runs the same checks against a deployed playground instead
// (PLAY_URL=https://rawr.airstrike.fyi/play): the page's own snippet is
// replaced with the looping `main` through the editor, and the module
// fetches are counted from the browser's network log. A failure prints
// every log the page and the program's frames wrote, each marked with its
// frame, the /web responses, and what the page and its frames show.
import { spawn } from "node:child_process";
import { existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, statSync, writeFileSync } from "node:fs";
import { createServer } from "node:http";
import { extname, join, resolve } from "node:path";
import { webAsset } from "../src/play.ts";
import { inject } from "../src/playpage.ts";

const repo = resolve(import.meta.dirname, "../../../..");
const web = resolve(process.argv[2] ?? join(repo, "target/wasm/site"));
const shots = resolve(process.argv[3] ?? join(repo, "target/web-check"));
const chrome = process.env.CHROME ?? "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome";
const live = process.env.PLAY_URL;
mkdirSync(shots, { recursive: true });
for (const file of live ? [] : ["rawr_web.js", "universe.js", "universe.toml", "universe-debugger.js", "universe-debugger_bg.wasm.br"]) {
  if (!existsSync(join(web, file))) throw new Error(`no ${join(web, file)}: build both modules with web/build.sh`);
}
const site = join(repo, "site/dist");
if (!live && !existsSync(join(site, "play.html"))) throw new Error("no site/dist/play.html: run `npm run build` in site/");

/**
 * A `main` that loops and sleeps, each turn two calls deep in `inner`,
 * keeping its latest odd and even turns in a map.
 */
const LOOPING = `fn inner(depth: u64) -> u64 {
    let deep = depth * 2;
    deep
}

fn outer(turn: u64) -> u64 {
    let total = inner(turn) + 1;
    total
}

fn main() {
    let mut turn = 0;
    let mut latest = std::collections::HashMap::new();
    loop {
        turn += 1;
        latest.insert(if turn % 2 == 0 { "even" } else { "odd" }, turn);
        let total = outer(turn);
        println!("turn {turn}: {total}");
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
}
`;

const og = { title: "rawr playground", description: "", url: "http://localhost/play", image: "", alt: "" };
const page = live ? "" : inject(readFileSync(join(site, "play.html"), "utf8"), {
  snippet: { code: LOOPING }, examples: [], shared: null, og,
});
/** Each /web path the page asked for, in order. */
const served: string[] = [];
const types: Record<string, string> = { ".html": "text/html; charset=utf-8", ".js": "text/javascript", ".css": "text/css", ".png": "image/png", ".svg": "image/svg+xml" };
const server = createServer(async (request, response) => {
  const path = new URL(request.url ?? "/", "http://localhost").pathname;
  if (path === "/play") return response.writeHead(200, { "content-type": types[".html"] }).end(page);
  if (path.startsWith("/web/")) {
    served.push(path);
    // The Worker's own /web handler, over the files in WEB_DIR.
    const assets = {
      fetch: async (asset: Request) => {
        try {
          return new Response(readFileSync(join(web, new URL(asset.url).pathname.slice("/web/".length))));
        } catch {
          return new Response("not found", { status: 404 });
        }
      },
    };
    const headers = new Headers(request.headers as Record<string, string>);
    const answer = await webAsset(new Request(`http://127.0.0.1${path}`, { headers }), assets);
    return response.writeHead(answer.status, Object.fromEntries(answer.headers)).end(Buffer.from(await answer.arrayBuffer()));
  }
  const file = [path, `${path}.html`, `${path}/index.html`].map((p) => join(site, p)).find((f) => existsSync(f) && statSync(f).isFile());
  if (!file) return response.writeHead(404).end(`no ${path}`);
  response.writeHead(200, { "content-type": types[extname(file)] ?? "application/octet-stream" }).end(readFileSync(file));
});
await new Promise<void>((done) => server.listen(0, "127.0.0.1", done));
const port = (server.address() as { port: number }).port;

mkdirSync(join(repo, "target"), { recursive: true });
const profile = mkdtempSync(join(repo, "target/debugger-page-profile-"));
const browser = spawn(chrome, [
  "--headless=new", "--remote-debugging-port=0", `--user-data-dir=${profile}`, "--window-size=1440,900",
  "--no-first-run", "--enable-unsafe-swiftshader", "about:blank",
], { stdio: ["ignore", "ignore", "pipe"] });
const endpoint = await new Promise<string>((done, fail) => {
  let text = "";
  browser.stderr.on("data", (chunk) => {
    text += chunk;
    const found = text.match(/DevTools listening on (ws:\S+)/);
    if (found) done(found[1]);
  });
  browser.on("exit", () => fail(new Error("Chrome exited: " + text)));
});

// One DevTools session on the page's target.
const socket = new WebSocket(endpoint);
await new Promise((done) => socket.addEventListener("open", done, { once: true }));
let next = 0;
const pending = new Map<number, (message: any) => void>();
const logs: string[] = [];
/**
 * Each JavaScript context's frame: the page's own, or the program's frame
 * (a srcdoc iframe the page makes per run), so a log says whose it is.
 */
const contexts = new Map<number, string>();
let pageFrame = "";
socket.addEventListener("message", (event) => {
  const message = JSON.parse(String(event.data));
  if (message.id !== undefined) pending.get(message.id)?.(message);
  const params = message.params;
  const whose = (context: number | undefined) => `[${contexts.get(context ?? -1) ?? "?"}]`;
  if (message.method === "Runtime.executionContextCreated") {
    const { id, auxData } = params.context;
    if (!pageFrame && auxData?.isDefault) pageFrame = auxData.frameId;
    contexts.set(id, auxData?.frameId === pageFrame ? "page" : `frame ${auxData?.frameId?.slice(0, 6)}`);
  }
  if (message.method === "Runtime.consoleAPICalled") logs.push(`${whose(params.executionContextId)} console.${params.type}: ` + params.args.map((a: any) => a.value ?? a.description).join(" "));
  if (message.method === "Runtime.exceptionThrown") {
    const details = params.exceptionDetails;
    logs.push(`${whose(details.executionContextId)} exception: ${details.exception?.description ?? details.text}`);
  }
  if (message.method === "Log.entryAdded") logs.push(`[log] ${params.entry.level} ${params.entry.source}: ${params.entry.text} ${params.entry.url ?? ""}`);
  if (message.method === "Network.loadingFailed") logs.push(`[network] failed: ${params.errorText} (${params.type})`);
  if (message.method === "Network.responseReceived" && params.response.url.includes("/web/")) {
    const { status, url, headers } = params.response;
    logs.push(`[network] ${status} ${url} ${headers["content-type"] ?? ""} ${headers["content-encoding"] ?? ""}`);
    // A deployed page's fetches, counted as the local server counts its own.
    if (live && !params.response.fromDiskCache && status === 200) served.push(new URL(url).pathname);
  }
});
const send = (method: string, params: object = {}, sessionId?: string) =>
  new Promise<any>((done, fail) => {
    const id = ++next;
    pending.set(id, (message) => (message.error ? fail(new Error(`${method}: ${message.error.message}`)) : done(message.result)));
    socket.send(JSON.stringify({ id, method, params, sessionId }));
  });
const { targetId } = await send("Target.createTarget", { url: "about:blank" });
const { sessionId } = await send("Target.attachToTarget", { targetId, flatten: true });
const on = (method: string, params: object = {}) => send(method, params, sessionId);
const evaluate = async (expression: string) => {
  const { result, exceptionDetails } = await on("Runtime.evaluate", { expression, awaitPromise: true, returnByValue: true });
  if (exceptionDetails) throw new Error(`${expression}: ${exceptionDetails.exception?.description ?? exceptionDetails.text}`);
  return result.value;
};
const sleep = (ms: number) => new Promise((done) => setTimeout(done, ms));
/** What the page shows: its status, the panel, the output and each program frame's text. */
const scene = () =>
  evaluate(`JSON.stringify({
    status: document.getElementById("status")?.textContent,
    debugger: document.getElementById("debugger")?.innerText,
    out: document.getElementById("out")?.innerText,
    frames: [...document.querySelectorAll("#out iframe.universe")].map((f) => ({ hidden: f.hidden, text: f.contentDocument?.body?.innerText?.slice(-2000) })),
  }, null, 2)`).catch((error) => String(error));
const until = async (what: string, expression: string, ms = 30000) => {
  const start = Date.now();
  while (!(await evaluate(expression))) {
    if (Date.now() - start > ms) throw new Error(`timed out waiting for ${what}\n${await scene()}`);
    await sleep(100);
  }
};
const shot = async (name: string) => {
  const { data } = await on("Page.captureScreenshot", { format: "png" });
  writeFileSync(join(shots, name), Buffer.from(data, "base64"));
};
/** A real click at the middle of the element `expression` finds. */
const clickOn = async (expression: string) => {
  const box = await evaluate(`(() => { const e = ${expression}; if (!e) return null; e.scrollIntoView({ block: "nearest" }); const r = e.getBoundingClientRect(); return { x: r.x + r.width / 2, y: r.y + r.height / 2 }; })()`);
  if (!box) throw new Error(`nothing at ${expression}`);
  for (const type of ["mouseMoved", "mousePressed", "mouseReleased"]) {
    await on("Input.dispatchMouseEvent", { type, x: box.x, y: box.y, button: "left", clickCount: 1, pointerType: "mouse" });
  }
};
/** A real drag of the element `expression` finds, from its middle by `dx` and `dy`. */
const drag = async (expression: string, dx: number, dy: number) => {
  const box = await evaluate(`(() => { const e = ${expression}; if (!e) return null; const r = e.getBoundingClientRect(); return { x: r.x + r.width / 2, y: r.y + r.height / 2 }; })()`);
  if (!box) throw new Error(`nothing at ${expression}`);
  const mouse = (type: string, x: number, y: number) =>
    on("Input.dispatchMouseEvent", { type, x, y, button: "left", buttons: type === "mouseReleased" ? 0 : 1, clickCount: 1, pointerType: "mouse" });
  await on("Input.dispatchMouseEvent", { type: "mouseMoved", x: box.x, y: box.y, pointerType: "mouse" });
  await mouse("mousePressed", box.x, box.y);
  for (let step = 1; step <= 4; step++) await mouse("mouseMoved", box.x + (dx * step) / 4, box.y + (dy * step) / 4);
  await mouse("mouseReleased", box.x + dx, box.y + dy);
  await sleep(100);
};
const icon = `document.getElementById("debug")`;
const panel = `document.getElementById("debugger")`;
/** The editor's line number `line`, which a click sets a breakpoint on. */
const lineNumber = (line: number) => `[...document.querySelectorAll("#playground .cm-lineNumbers .cm-gutterElement")].find((e) => e.textContent === "${line}")`;
/** The debugger toolbar's action `label`, as its aria-label names it. */
const control = (label: string) => `document.querySelector('#debugger [role=toolbar] [aria-label="${label}"]')`;
/** A key typed on the page, with `text` when it types one. */
const press = async (key: string, code: string, keyCode: number, modifiers = 0, text?: string) => {
  await on("Input.dispatchKeyEvent", { type: "keyDown", key, code, windowsVirtualKeyCode: keyCode, modifiers, ...(text ? { text } : {}) });
  await on("Input.dispatchKeyEvent", { type: "keyUp", key, code, windowsVirtualKeyCode: keyCode, modifiers });
};
const SHIFT = 8;
const entries = `[...document.querySelectorAll("#console .scrollback .entry")].map((e) => [e.querySelector(".in").textContent, e.querySelector(".answer").textContent])`;
const phase = `(${panel}?.querySelector(".phase")?.textContent ?? "")`;
const where = `(${panel}?.querySelector(".where")?.textContent ?? "")`;
const binding = (name: string) => `(${panel}?.querySelector('[data-binding="${name}"] > td.value')?.textContent ?? null)`;
/** What the program printed in its frame's terminal. */
const printed = `(document.querySelector("#out iframe.universe:not([hidden])")?.contentDocument?.querySelector("pre.terminal")?.textContent ?? "")`;
const editor = `document.querySelector("#playground .editor").editor`;
/** How often the page fetched the debugger build's module. */
const debuggerFetches = () => served.filter((path) => path === "/web/universe-debugger_bg.wasm").length;

let failed = false;
const check = (ok: boolean, what: string, detail: unknown = "") => {
  console.log(`${ok ? "ok" : "FAILED"}: ${what}${ok ? "" : ` ${JSON.stringify(detail)}`}`);
  failed ||= !ok;
};
try {
  await on("Runtime.enable");
  await on("Page.enable");
  await on("Log.enable");
  await on("Network.enable");
  await on("Emulation.setDeviceMetricsOverride", { width: 1440, height: 900, deviceScaleFactor: 1, mobile: false });
  await on("Emulation.setEmulatedMedia", { features: [{ name: "prefers-color-scheme", value: "dark" }] });
  await on("Page.navigate", { url: live ?? `http://127.0.0.1:${port}/play` });
  await until("the island", `document.readyState === "complete" && !!document.querySelector("#playground .cm-content") && !${icon}.closest("[hidden]")`);
  // A deployed page has its own snippet: the looping one goes in its place.
  if (live) await evaluate(`${editor}.dispatch({ changes: { from: 0, to: ${editor}.state.doc.length, insert: ${JSON.stringify(LOOPING)} } })`);
  check(debuggerFetches() === 0, "the page loads without the debugger build");
  check(await evaluate(`!${icon}.hasAttribute("aria-disabled")`), "Chrome has JSPI, so the icon is enabled");
  check(await evaluate(`!document.querySelector("#playground .cm-lineNumbers")`), "the editor shows no line numbers while the debugger is off");

  // The icon: the debugger build runs the program, and the panel opens.
  await clickOn(icon);
  await until("the panel", `!!${panel}`, 5000);
  check(await evaluate(`${icon}.getAttribute("aria-pressed") === "true"`), "the icon shows the debugger is on");
  // The debugger build is 36 MB: a cold fetch from a deployed page takes a while.
  await until("the program's turns under the debugger", `${printed}.includes("turn 3:")`, 120000);
  check(debuggerFetches() > 0, "the debugger build is fetched on the click");
  check(await evaluate(`${phase} === "running"`), "the panel says the program runs", await evaluate(phase));
  check(await evaluate(`!${panel}.textContent.includes("JSPI")`), "Chrome's pause can wait, so the panel says nothing of JSPI");
  await shot("debugger-running.png");

  // A click on line 2's number: the loop pauses there, two calls deep.
  await clickOn(lineNumber(2));
  await until("the pause", `${phase}.startsWith("paused")`, 10000);
  check(await evaluate(`${lineNumber(2)}.classList.contains("cm-breakpoint")`), "line 2's number is marked as a breakpoint");
  check(await evaluate(`${where}.endsWith("line 2") && ${where}.startsWith("inner")`), "the panel says where it paused", await evaluate(where));
  check(await evaluate(`document.querySelector("#playground .cm-paused")?.textContent.includes("let deep = depth * 2;")`), "the paused line is highlighted");
  await until("the bindings", `${binding("depth")} !== null`, 5000);
  const depth = Number(await evaluate(binding("depth")));
  check(depth > 0, "the panel shows the paused frame's bindings", depth);
  const frames: string[] = await evaluate(`[...${panel}.querySelectorAll(".frames-list button")].map((b) => b.textContent)`);
  check(frames.length === 3 && frames[0].startsWith("inner") && frames[1].startsWith("outer") && frames[2].startsWith("main"), "and its frames up to main", frames);
  await clickOn(`[...${panel}.querySelectorAll(".frames-list button")][2]`);
  await until("main's bindings", `${binding("turn")} !== null`, 5000);
  check(Number(await evaluate(binding("turn"))) === depth, "a frame's click shows its own bindings");
  // A map opens to one row per key and its value, never its internals.
  const latest = `${panel}.querySelector('[data-binding="latest"]')`;
  check(await evaluate(`!!${latest}?.querySelector("button.twisty")`), "a map binding can open", await evaluate(`${latest}?.textContent ?? null`));
  await clickOn(`${latest}.querySelector("button.twisty")`);
  await until("the map's rows", `${panel}.querySelectorAll(".bindings tr[data-part]").length > 0`, 2000);
  const pairs: string[][] = await evaluate(`[...${panel}.querySelectorAll(".bindings tr[data-part]")].map((row) => [row.querySelector(".name").textContent, row.querySelector(".value").textContent])`);
  const parity = depth % 2 === 0 ? '"even"' : '"odd"';
  check(
    pairs.length >= 1 && pairs.length <= 2 && pairs.every(([key]) => key === '"even"' || key === '"odd"') && pairs.some(([key, value]) => key === parity && value === String(depth)),
    "a map binding opens to one row per key and value", pairs,
  );
  const written = await evaluate(`${printed}.length`);
  await sleep(300);
  check((await evaluate(`${printed}.length`)) === written, "the program prints nothing while paused");

  // The toolbar's actions: glyphs named for their action and key.
  const titles: string[] = await evaluate(`[...${panel}.querySelectorAll("[role=toolbar] button")].map((b) => b.title)`);
  check(JSON.stringify(titles) === JSON.stringify(["Continue (F8)", "Pause (F8)", "Step over (F10)", "Step into (F11)", "Step out (Shift+F11)"]), "the toolbar's glyphs name their action and key", titles);
  check(await evaluate(`${control("Pause")}.disabled && !${control("Continue")}.disabled`), "paused, Pause is off and Continue on");

  // The console under the code: a typed Enter evaluates where it is paused,
  // in the frame picked above, `main`'s.
  check(await evaluate(`!!document.querySelector("#playground section.pane #console") && !${panel}.querySelector("#console")`), "the console sits under the code, not in the panel");
  await clickOn(`document.querySelector("#console .prompt .cm-content")`);
  await on("Input.insertText", { text: "turn + 1" });
  await press("Enter", "Enter", 13, 0, "\r");
  await until("the eval", `document.querySelectorAll("#console .scrollback .entry").length === 1`, 5000);
  check(JSON.stringify(await evaluate(entries)) === JSON.stringify([["turn + 1", String(depth + 1)]]), "an eval answers in the selected frame, main's", await evaluate(entries));
  check(await evaluate(`document.querySelector("#console .prompt").textContent === ">"`), "and the prompt empties");
  // Shift+Enter adds a line; the eval and its answer join the scrollback.
  await on("Input.insertText", { text: "turn" });
  await press("Enter", "Enter", 13, SHIFT, "\r");
  check(await evaluate(`document.querySelectorAll("#console .prompt .cm-line").length === 2`), "Shift+Enter adds a line to the prompt");
  await on("Input.insertText", { text: "* 3" });
  await press("Enter", "Enter", 13, 0, "\r");
  await until("the second eval", `document.querySelectorAll("#console .scrollback .entry").length === 2`, 5000);
  const scrolled: string[][] = await evaluate(entries);
  check(scrolled[0][0] === "turn + 1" && scrolled[1][0].startsWith("turn\n") && scrolled[1][0].endsWith("* 3") && scrolled[1][1] === String(depth * 3), "a two-line eval answers under the first in the scrollback", scrolled);
  await shot("debugger-dark.png");
  await on("Emulation.setEmulatedMedia", { features: [{ name: "prefers-color-scheme", value: "light" }] });
  await until("the light theme", `document.documentElement.dataset.theme === "light"`, 5000);
  await shot("debugger-light.png");
  await on("Emulation.setDeviceMetricsOverride", { width: 390, height: 844, deviceScaleFactor: 2, mobile: true });
  await sleep(300);
  await shot("debugger-phone.png");
  check(await evaluate(`document.documentElement.scrollWidth <= innerWidth`), "a phone has no sideways scroll with the debugger on");
  await on("Emulation.setDeviceMetricsOverride", { width: 1440, height: 900, deviceScaleFactor: 1, mobile: false });
  await on("Emulation.setEmulatedMedia", { features: [{ name: "prefers-color-scheme", value: "dark" }] });
  await sleep(300);

  // F10 steps over a line; the Continue glyph pauses on the next turn.
  await evaluate(`document.activeElement?.blur()`);
  await press("F10", "F10", 121);
  await until("the step", `${where}.endsWith("line 3")`, 5000);
  await until("the step's bindings", `${binding("deep")} !== null`, 5000);
  check(Number(await evaluate(binding("deep"))) === depth * 2, "a step over reads the binding it made");
  await clickOn(control("Continue"));
  await until("the next turn's pause", `${where}.endsWith("line 2") && ${binding("depth")} === "${depth + 1}"`, 5000);
  check(true, "a continue pauses at the breakpoint on the next turn");

  // Cleared and continued, the loop runs on.
  await clickOn(lineNumber(2));
  await until("the cleared mark", `!${lineNumber(2)}.classList.contains("cm-breakpoint")`, 5000);
  await press("F8", "F8", 119);
  await until("the loop running on", `${phase} === "running"`, 5000);
  const turns = await evaluate(`${printed}.length`);
  await until("more turns", `${printed}.length > ${turns}`, 5000);
  check(true, "cleared and continued, the loop runs on");

  // Off: the panel closes, and a program that ends runs as before.
  await evaluate(`${editor}.dispatch({ changes: { from: 0, to: ${editor}.state.doc.length, insert: 'fn main() {\\n    println!("plain again");\\n}\\n' } })`);
  await clickOn(icon);
  await until("the plain run", `(document.querySelector("#out pre.term") || {}).textContent?.includes("plain again")`, 15000);
  check(await evaluate(`!${panel} && !document.querySelector("#playground .cm-lineNumbers")`), "the panel and the line numbers go");
  check(await evaluate(`!document.querySelector("#out iframe.universe:not([hidden])")`), "the debugger's frame goes");
  await shot("debugger-off.png");

  // On again: the debugger build isn't fetched again.
  const fetched = debuggerFetches();
  await clickOn(icon);
  await until("the panel again", `!!${panel}`, 5000);
  // A program that ends before the page looks prints under its frame.
  await until("the program's run under the debugger", `${printed}.includes("plain again") || (document.querySelector("#out pre.term") || {}).textContent?.includes("plain again")`, 15000);
  check(debuggerFetches() === fetched, "turned on again, the page fetches the module no more", served);
  // Its main returned: the panel says so, with its status, and its controls are off.
  await until("the end of main", `${phase} === "main returned · exit 0"`, 5000);
  check(await evaluate(`[...${panel}.querySelectorAll("[role=toolbar] button")].every((b) => b.disabled)`), "once main returned, every action is off");

  // The split bars: a drag moves the line between two panes, and the
  // viewer's sizes come back with the page.
  const code = `document.querySelector("#playground main > section.pane")`;
  const width = (pane: string) => evaluate(`Math.round(${pane}.getBoundingClientRect().width)`);
  const codeBefore = await width(code);
  const debuggerBefore = await width(panel);
  await drag(`document.querySelector('[role=separator][aria-label="Resize the code and the output"]')`, -120, 0);
  const codeAfter = await width(code);
  check(Math.abs(codeAfter - (codeBefore - 120)) <= 2, "dragging the code's bar narrows the code", [codeBefore, codeAfter]);
  await drag(`document.querySelector('[role=separator][aria-label="Resize the debugger"]')`, -80, 0);
  const debuggerAfter = await width(panel);
  check(Math.abs(debuggerAfter - (debuggerBefore + 80)) <= 2, "dragging the debugger's bar widens the debugger", [debuggerBefore, debuggerAfter]);
  const consolePane = `document.getElementById("console")`;
  const height = () => evaluate(`Math.round(${consolePane}.getBoundingClientRect().height)`);
  const consoleBefore = await height();
  await drag(`document.querySelector('[role=separator][aria-label="Resize the console"]')`, 0, -60);
  check(Math.abs((await height()) - (consoleBefore + 60)) <= 2, "dragging the console's bar raises the console", [consoleBefore, await height()]);
  const consoleAfter = await height();
  const codeSet = await width(code);
  await shot("debugger-resized.png");
  // The page again, and the debugger on again: the sizes are the ones dragged.
  await on("Page.reload");
  await until("the island again", `document.readyState === "complete" && !!document.querySelector("#playground .cm-content") && !${icon}.closest("[hidden]")`);
  await clickOn(icon);
  await until("the panel after the reload", `!!${panel} && !!${consolePane}`, 15000);
  check(Math.abs((await width(code)) - codeSet) <= 1, "a reload keeps the code's width", [codeSet, await width(code)]);
  check((await width(panel)) === debuggerAfter, "and the debugger's", [debuggerAfter, await width(panel)]);
  check((await height()) === consoleAfter, "and the console's height", [consoleAfter, await height()]);
  // A phone stacks the panes and draws no bars.
  await on("Emulation.setDeviceMetricsOverride", { width: 390, height: 844, deviceScaleFactor: 2, mobile: true });
  await sleep(300);
  check(await evaluate(`[...document.querySelectorAll("[role=separator]")].every((bar) => getComputedStyle(bar).display === "none")`), "a phone draws no split bars");
} catch (error) {
  check(false, String(error));
} finally {
  if (failed) console.log(logs.join("\n"));
  socket.close();
  const exited = new Promise((done) => browser.once("exit", done));
  browser.kill();
  await exited;
  server.close();
  rmSync(profile, { recursive: true, force: true, maxRetries: 5 });
}
process.exit(failed ? 1 : 0);
