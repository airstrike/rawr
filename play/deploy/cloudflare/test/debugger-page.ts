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
// opens it again without fetching the debugger build's module again. It
// saves window screenshots in SHOTS_DIR (default target/web-check): the
// paused debugger in the dark and light themes at 1440 wide and on a
// phone. CHROME names the browser when it is not in the usual place.
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
mkdirSync(shots, { recursive: true });
for (const file of ["rawr_web.js", "universe.js", "universe.toml", "universe-debugger.js", "universe-debugger_bg.wasm.br"]) {
  if (!existsSync(join(web, file))) throw new Error(`no ${join(web, file)}: build both modules with web/build.sh`);
}
const site = join(repo, "site/dist");
if (!existsSync(join(site, "play.html"))) throw new Error("no site/dist/play.html: run `npm run build` in site/");

/** A `main` that loops and sleeps, each turn two calls deep in `inner`. */
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
    loop {
        turn += 1;
        let total = outer(turn);
        println!("turn {turn}: {total}");
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
}
`;

const og = { title: "rawr playground", description: "", url: "http://localhost/play", image: "", alt: "" };
const page = inject(readFileSync(join(site, "play.html"), "utf8"), {
  snippet: { code: LOOPING }, examples: [], sitekey: null, shared: null, og,
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
socket.addEventListener("message", (event) => {
  const message = JSON.parse(String(event.data));
  if (message.id !== undefined) pending.get(message.id)?.(message);
  if (message.method === "Runtime.consoleAPICalled") logs.push(message.params.args.map((a: any) => a.value ?? a.description).join(" "));
  if (message.method === "Runtime.exceptionThrown") logs.push("exception: " + message.params.exceptionDetails.exception?.description);
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
const until = async (what: string, expression: string, ms = 30000) => {
  const start = Date.now();
  while (!(await evaluate(expression))) {
    if (Date.now() - start > ms) throw new Error(`timed out waiting for ${what}\n${logs.join("\n")}`);
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
const binding = (name: string) => `(${panel}?.querySelector('[data-binding="${name}"] dd')?.textContent ?? null)`;
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
  await on("Emulation.setDeviceMetricsOverride", { width: 1440, height: 900, deviceScaleFactor: 1, mobile: false });
  await on("Emulation.setEmulatedMedia", { features: [{ name: "prefers-color-scheme", value: "dark" }] });
  await on("Page.navigate", { url: `http://127.0.0.1:${port}/play` });
  await until("the island", `document.readyState === "complete" && !!document.querySelector("#playground .cm-content") && !${icon}.hidden`);
  check(debuggerFetches() === 0, "the page loads without the debugger build");
  check(await evaluate(`!document.querySelector("#playground .cm-lineNumbers")`), "the editor shows no line numbers while the debugger is off");

  // The icon: the debugger build runs the program, and the panel opens.
  await clickOn(icon);
  await until("the panel", `!!${panel}`, 5000);
  check(await evaluate(`${icon}.getAttribute("aria-pressed") === "true"`), "the icon shows the debugger is on");
  await until("the program's turns under the debugger", `${printed}.includes("turn 3:")`);
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
  const written = await evaluate(`${printed}.length`);
  await sleep(300);
  check((await evaluate(`${printed}.length`)) === written, "the program prints nothing while paused");

  // The toolbar's actions: glyphs named for their action and key.
  const titles: string[] = await evaluate(`[...${panel}.querySelectorAll("[role=toolbar] button")].map((b) => b.title)`);
  check(JSON.stringify(titles) === JSON.stringify(["Continue (F8)", "Pause (F8)", "Step over (F10)", "Step into (F11)", "Step out (Shift+F11)"]), "the toolbar's glyphs name their action and key", titles);
  check(await evaluate(`${control("Pause")}.disabled && !${control("Continue")}.disabled`), "paused, Pause is off and Continue on");

  // The console under the code: a typed Enter evaluates where it is paused.
  check(await evaluate(`!!document.querySelector("#playground section.pane #console") && !${panel}.querySelector("#console")`), "the console sits under the code, not in the panel");
  await clickOn(`document.querySelector("#console .prompt .cm-content")`);
  await on("Input.insertText", { text: "depth + 1" });
  await press("Enter", "Enter", 13, 0, "\r");
  await until("the eval", `document.querySelectorAll("#console .scrollback .entry").length === 1`, 5000);
  check(JSON.stringify(await evaluate(entries)) === JSON.stringify([["depth + 1", String(depth + 1)]]), "an eval answers in the paused frame", await evaluate(entries));
  check(await evaluate(`document.querySelector("#console .prompt").textContent === ">"`), "and the prompt empties");
  // Shift+Enter adds a line; the eval and its answer join the scrollback.
  await on("Input.insertText", { text: "depth" });
  await press("Enter", "Enter", 13, SHIFT, "\r");
  check(await evaluate(`document.querySelectorAll("#console .prompt .cm-line").length === 2`), "Shift+Enter adds a line to the prompt");
  await on("Input.insertText", { text: "* 3" });
  await press("Enter", "Enter", 13, 0, "\r");
  await until("the second eval", `document.querySelectorAll("#console .scrollback .entry").length === 2`, 5000);
  const scrolled: string[][] = await evaluate(entries);
  check(scrolled[0][0] === "depth + 1" && scrolled[1][0].startsWith("depth\n") && scrolled[1][0].endsWith("* 3") && scrolled[1][1] === String(depth * 3), "a two-line eval answers under the first in the scrollback", scrolled);
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
