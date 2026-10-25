// The playground and the docs' Run buttons in a real headless Chrome, driven
// over the DevTools protocol with Node's own WebSocket:
//   web/build.sh && npm run build --prefix site && node test/browser.ts [WEB_DIR] [SHOTS_DIR]
// It serves the site's build (site/dist) as the Worker does: /play is
// play.html with the request's data written in (playpage.ts's inject), and
// /web is rawr-web from WEB_DIR (default target/wasm/site). On the
// playground's island it runs the Counter in the browser (the default),
// clicks Increment three times, edits it in the editor and keeps the count,
// switches the theme from the site's header and sees the app follow, runs a
// plain program, an iced app whose task sleeps, the "Async tasks" example
// from its #async-tasks address on the page's real clock, picks an example
// from the menu, runs a program on a stand-in server, and checks the phone
// layout. Then it opens the docs' Play page and checks that run.ts posts the
// app snippet with its ?play options. It saves window screenshots in
// SHOTS_DIR (default target/web-check).
// CHROME names the browser when it is not in the usual place.
import { spawn } from "node:child_process";
import { existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, statSync, writeFileSync } from "node:fs";
import { createServer } from "node:http";
import { extname, join, resolve } from "node:path";
import { parseSnippet, webAsset } from "../src/play.ts";
import { inject } from "../src/playpage.ts";

const repo = resolve(import.meta.dirname, "../../../..");
const web = resolve(process.argv[2] ?? join(repo, "target/wasm/site"));
const shots = resolve(process.argv[3] ?? join(repo, "target/web-check"));
const chrome = process.env.CHROME ?? "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome";
const counter = readFileSync(join(repo, "play/tests/fixtures/counter.rs"), "utf8");
const tasks = readFileSync(join(repo, "play/tests/fixtures/rust/tasks.rs"), "utf8");
const channel = readFileSync(join(repo, "play/tests/fixtures/channel.rs"), "utf8");
const histogram = readFileSync(join(repo, "play/tests/fixtures/rust/histogram.rs"), "utf8");
mkdirSync(shots, { recursive: true });

// The site's build, served as the Worker's assets serve it.
const site = join(repo, "site/dist");
if (!existsSync(join(site, "play.html"))) throw new Error("no site/dist/play.html: run `npm run build` in site/");
const og = { title: "rawr playground", description: "", url: "http://localhost/play", image: "", alt: "" };
const examples = [
  { name: "Letter histogram", code: histogram, group: "Rust" },
  { name: "Async tasks", code: tasks, group: "Rust" },
  { name: "Counter", code: counter, messages: ["Message::Increment", "Message::Increment"], group: "iced" },
];
const page = inject(readFileSync(join(site, "play.html"), "utf8"), {
  snippet: { code: counter, theme: "dark", size: "", messages: [] }, examples, sitekey: null, shared: null, og,
});
const brotli: string[] = [];
const siteRuns: unknown[] = [];
const renders: unknown[] = [];
const types: Record<string, string> = { ".html": "text/html; charset=utf-8", ".js": "text/javascript", ".css": "text/css", ".png": "image/png", ".svg": "image/svg+xml" };
const body = (request: import("node:http").IncomingMessage) =>
  new Promise<string>((done) => {
    let text = "";
    request.on("data", (chunk) => (text += chunk));
    request.on("end", () => done(text));
  });
const server = createServer(async (request, response) => {
  const path = new URL(request.url ?? "/", "http://localhost").pathname;
  if (path === "/play") return response.writeHead(200, { "content-type": types[".html"] }).end(page);
  if (path === "/play/run" && request.method === "POST") {
    siteRuns.push(JSON.parse(await body(request)));
    return response.writeHead(200, { "content-type": "image/png" }).end(readFileSync(join(site, "img/docs-play-counter.png")));
  }
  // A stand-in for the server's renderer: a plain program's output.
  if (path === "/play/render" && request.method === "POST") {
    renders.push(JSON.parse(await body(request)));
    return response.writeHead(200, { "content-type": "application/json" }).end(JSON.stringify({ kind: "text", status: 0, output: [{ stream: "stdout", text: "from the stand-in server\n" }] }));
  }
  if (path.startsWith("/web/")) {
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
    const served = await webAsset(new Request(`http://127.0.0.1${path}`, { headers }), assets);
    if (served.headers.get("content-encoding")) brotli.push(path);
    return response.writeHead(served.status, Object.fromEntries(served.headers)).end(Buffer.from(await served.arrayBuffer()));
  }
  const file = [path, `${path}.html`, `${path}/index.html`].map((p) => join(site, p)).find((f) => existsSync(f) && statSync(f).isFile());
  if (!file) return response.writeHead(404).end(`no ${path}`);
  response.writeHead(200, { "content-type": types[extname(file)] ?? "application/octet-stream" }).end(readFileSync(file));
});
await new Promise<void>((done) => server.listen(0, "127.0.0.1", done));
const port = (server.address() as { port: number }).port;

mkdirSync(join(repo, "target"), { recursive: true });
const profile = mkdtempSync(join(repo, "target/web-check-profile-"));
const browser = spawn(chrome, [
  "--headless=new", "--remote-debugging-port=0", `--user-data-dir=${profile}`, "--window-size=1280,800",
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
const evaluate = async (expression: string) =>
  (await on("Runtime.evaluate", { expression, awaitPromise: true, returnByValue: true })).result.value;
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
  return data as string;
};
const out = async () => {
  const { data } = await on("Page.captureScreenshot", { format: "png", clip: { ...(await evaluate(`(() => { const r = document.getElementById("out").getBoundingClientRect(); return { x: r.x, y: r.y, width: r.width, height: r.height }; })()`)), scale: 1 } });
  return data as string;
};
const click = async (x: number, y: number) => {
  for (const type of ["mousePressed", "mouseReleased"]) {
    await on("Input.dispatchMouseEvent", { type, x, y, button: "left", clickCount: 1, pointerType: "mouse" });
  }
};
/** A real click at the middle of the element `selector` (or `expression`) finds. */
const clickOn = async (expression: string) => {
  const box = await evaluate(`(() => { const e = ${expression}; if (!e) return null; e.scrollIntoView({ block: "nearest" }); const r = e.getBoundingClientRect(); return { x: r.x + r.width / 2, y: r.y + r.height / 2 }; })()`);
  if (!box) throw new Error(`nothing at ${expression}`);
  await click(box.x, box.y);
};
const editorText = `document.querySelector("#playground .editor").editor.state.doc.toString()`;
/** Types `text` over the editor's whole text, as a visitor would: select all, then type. */
const setCode = async (text: string) => {
  await evaluate(`document.querySelector("#playground .editor").editor.focus()`);
  const mod = process.platform === "darwin" ? 4 : 2;
  await on("Input.dispatchKeyEvent", { type: "keyDown", key: "a", code: "KeyA", modifiers: mod, windowsVirtualKeyCode: 65, commands: ["selectAll"] });
  await on("Input.dispatchKeyEvent", { type: "keyUp", key: "a", code: "KeyA", modifiers: mod, windowsVirtualKeyCode: 65 });
  await on("Input.insertText", { text });
};
const radio = (group: string, name: string) => `[...document.querySelectorAll('[aria-label="${group}"] [role=radio]')].find((b) => b.textContent === ${JSON.stringify(name)})`;
const checked = (group: string) => `document.querySelector('[aria-label="${group}"] [role=radio][aria-checked=true]')?.textContent`;

let failed = false;
const check = (ok: boolean, what: string) => {
  console.log(`${ok ? "ok" : "FAILED"}: ${what}`);
  failed ||= !ok;
};
try {
  await on("Runtime.enable");
  await on("Page.enable");
  await on("Emulation.setDeviceMetricsOverride", { width: 1280, height: 800, deviceScaleFactor: 1, mobile: false });
  await on("Emulation.setEmulatedMedia", { features: [{ name: "prefers-color-scheme", value: "dark" }] });
  const loaded = performance.now();
  await on("Page.navigate", { url: `http://127.0.0.1:${port}/play` });
  await until("the island", `document.readyState === "complete" && !!document.querySelector("#playground .cm-content")`);
  // The capture listener starts a click's clock and the bubbling one stops it: iced's handlers run between.
  await evaluate(`window.clicks = []; let t = 0;
    addEventListener("pointerup", () => { t = performance.now(); }, true);
    addEventListener("pointerup", () => { const handled = performance.now() - t; requestAnimationFrame(() => requestAnimationFrame(() => window.clicks.push([handled, performance.now() - t]))); });`);
  check(await evaluate(`!document.querySelector("script[src*=rawr_web]")`), "nothing loads rawr-web in the HTML");
  check(await evaluate(`${checked("Where it runs")} === "Run in browser"`), "the browser is the default");
  check(await evaluate(`document.documentElement.dataset.theme === "dark" && ${checked("Theme")} === "Dark"`), "the page and the playground's control follow the system's dark theme");
  check(await evaluate(`(${editorText}).includes("struct Counter")`), "the editor holds the page's snippet");
  check(await evaluate(`document.querySelectorAll("#playground .cm-content .k").length > 10`), "the editor highlights Rust in the site's classes");
  await evaluate(`document.getElementById("run").click()`);
  await until("the canvas", `!!document.querySelector("#out .live canvas") && document.getElementById("status").textContent.includes("live")`);
  await until("the first frame", `document.querySelector("#out .live canvas").width > 0`);
  console.log(`canvas up ${Math.round(performance.now() - loaded)} ms after navigation`);
  await sleep(500);
  const before = await out();
  await shot("counter.png");
  const box = await evaluate(`(() => { const r = document.querySelector("#out .live canvas").getBoundingClientRect(); return { x: r.x, y: r.y }; })()`);
  // The Counter's column: 20 px of padding, then Increment centred above the wider Decrement.
  for (let i = 0; i < 3; i++) {
    await click(box.x + 69, box.y + 36);
    await sleep(150);
  }
  await sleep(300);
  const clicked = await out();
  await shot("counter-clicked.png");
  check(clicked !== before, "three clicks on Increment change the canvas");
  const clicks: number[][] = await evaluate("window.clicks");
  console.log(`click handlers: ${clicks.map(([h]) => h.toFixed(1)).join(", ")} ms; to the next frame: ${clicks.map(([, f]) => f.toFixed(1)).join(", ")} ms`);

  // An edit in the editor: ten per click, relabelled. The count carries over.
  const edited = counter.replace("self.value += 1", "self.value += 10").replace('button("Increment")', 'button("Add ten")');
  await setCode(edited);
  await sleep(1500);
  await shot("counter-edited.png");
  check(await evaluate(`(${editorText}) === ${JSON.stringify(edited)}`), "typing in the editor replaces the code");
  check(logs.some((line) => line.includes("state carried")), "an edit swaps in a new generation and keeps the count");
  await click(box.x + 69, box.y + 36);
  await sleep(400);
  await shot("counter-edited-clicked.png");

  // The site's theme switch: the page and the running app go light together.
  const dark = await out();
  await clickOn(`document.querySelector("[data-theme-switch]")`);
  await sleep(1500);
  const light = await out();
  await shot("counter-light.png");
  check(await evaluate(`document.documentElement.dataset.theme === "light" && ${checked("Theme")} === "Light"`), "the header's switch sets the page's theme and the playground's control");
  check(await evaluate(`getComputedStyle(document.body).backgroundColor === "rgb(247, 247, 248)"`), "the page draws in the light palette");
  check(dark !== light && !!(await evaluate(`!!document.querySelector("#out .live canvas")`)), "the running app follows the theme");
  check(await evaluate(`localStorage.getItem("rawr-theme") === "light"`), "the choice is remembered");
  // And back, from the playground's own control.
  await clickOn(radio("Theme", "Dark"));
  await sleep(300);
  check(await evaluate(`document.documentElement.dataset.theme === "dark"`), "the playground's Dark sets the page's theme");

  // A broken edit keeps the app and shows the report under it.
  await setCode(edited.replace("fn update", "fn update("));
  await until("the report", `!!document.querySelector("#out .live ~ pre.error, #out pre.error")`, 5000);
  await shot("counter-error.png");
  check(await evaluate(`!!document.querySelector("#out .live canvas")`), "a broken edit keeps the running app");

  // Monospace text and symbols, from the fonts rawr-web fetches.
  await setCode('column![text("Fira Mono: let x = 42;").font(Font::MONOSPACE).size(24), text("Symbols: → ★ ✓ ∑ ⌘").size(24), text("CJK: 你好 こんにちは").size(24)].spacing(12).padding(20)');
  const fonts = ["FiraMono", "NotoSansSymbols-", "NotoSansSymbols2", "NotoSansCJK"];
  const start = Date.now();
  while (!fonts.every((font) => logs.some((line) => line.includes(`font ${font}`) && line.includes("Ok")))) {
    if (Date.now() - start > 30000 || logs.some((line) => line.includes("did not load"))) break;
    await sleep(200);
  }
  console.log(`fonts in ${Date.now() - start} ms`);
  await sleep(500);
  await shot("fonts.png");
  check(fonts.every((font) => logs.some((line) => line.includes(`font ${font}`) && line.includes("Ok"))), "the fonts load");

  // A plain program runs in the browser and shows its output as text.
  await setCode('for i in 1..=3 {\n    println!("line {i}");\n}\neprintln!("done");');
  await until("the output", `(document.querySelector("#out pre.term") || {}).textContent?.includes("line 3")`, 5000);
  await shot("plain.png");
  console.log("plain output:", JSON.stringify(await evaluate(`document.querySelector("#out pre.term").textContent`)), "·", await evaluate(`document.getElementById("status").textContent`));
  check(true, "a plain program runs in the browser");

  // An iced app whose boot task sleeps and joins a channel's two ends: "adding up", then "sum 10" 400 ms on.
  await setCode(channel);
  await until("the app", `!!document.querySelector("#out .live canvas") && document.getElementById("status").textContent.includes("live")`, 5000);
  await sleep(100);
  const adding = await out();
  await sleep(1200);
  const summed = await out();
  await shot("iced-task.png");
  check(adding !== summed && !logs.some((line) => line.includes("does not run in a browser")), "an iced app's task sleeps on the page's clock and ends");

  // The "Async tasks" example from its address: restored and run in the browser once the module is up.
  // From another page: a hash change alone would not load the page again.
  await on("Page.navigate", { url: "about:blank" });
  await on("Page.navigate", { url: `http://127.0.0.1:${port}/play#async-tasks` });
  await until("the restored example", `(document.querySelector("#out pre.term") || {}).textContent?.includes("jobs in")`, 15000);
  check(await evaluate(`(${editorText}).includes("mpsc::channel(4)")`), "#async-tasks restores the example");
  check(await evaluate(`document.querySelector('[aria-label="Examples"]').textContent.includes("Async tasks")`), "the menu names the example");
  // Run it again, timing each line as it shows.
  await evaluate(`window.lines = []; window.ran = performance.now();
    new MutationObserver(() => { const text = (document.querySelector("#out pre.term") || {}).textContent || "";
      for (const line of text.split("\\n")) if (line && !window.lines.some((l) => l[0] === line)) window.lines.push([line, performance.now() - window.ran]); })
      .observe(document.getElementById("out"), { childList: true, subtree: true, characterData: true });
    document.getElementById("run").click();`);
  await until("the async output", `window.lines.some(([line]) => line.includes("jobs in"))`, 5000);
  await shot("async.png");
  const lines: [string, number][] = await evaluate("window.lines");
  console.log("async output:", lines.map(([line, ms]) => `${JSON.stringify(line)} at ${Math.round(ms)} ms`).join(", "), "·", await evaluate(`document.getElementById("status").textContent`));
  const printed = lines.map(([line]) => line);
  const times = printed.map((line) => Number(line.match(/(\d+) ms/)?.[1]));
  const shown = (start: string) => lines.find(([line]) => line.startsWith(start))?.[1] ?? 0;
  check(printed[0]?.startsWith("fetch at") && printed[1]?.startsWith("parse at") && printed[2]?.startsWith("render at") && printed[3]?.startsWith("3 jobs in"), "the Async tasks example prints its four lines in order");
  check(times.every((ms, i) => ms >= [100, 200, 300, 300][i] && ms < [100, 200, 300, 300][i] + 60), "its times are the real clock's, 100, 200 and 300 ms");
  check(shown("fetch at") < shown("3 jobs in") - 150 && shown("3 jobs in") >= 290 && shown("3 jobs in") < 600, "its lines show as they are written, the last about 300 ms after the run");
  check(await evaluate(`location.hash === "#async-tasks" && location.search === ""`), "the address keeps the example and the default mode");
  check(brotli.includes("/web/rawr_web_bg.wasm"), "the module comes as its brotli copy");

  // The examples menu: grouped, its names in their own case, and a pick runs the example.
  await clickOn(`document.querySelector('[aria-label="Examples"]')`);
  await until("the menu", `!!document.querySelector("[role=listbox]")`, 5000);
  await shot("examples-menu.png");
  check(await evaluate(`[...document.querySelectorAll("[role=listbox] [role=group]")].map((g) => g.firstElementChild.textContent).join() === "Rust,iced"`), "the menu groups Rust and iced under their own names");
  await clickOn(`[...document.querySelectorAll("[role=option]")].find((o) => o.textContent === "Letter histogram")`);
  await until("the histogram", `(document.querySelector("#out pre.term") || {}).textContent?.includes("#")`, 5000);
  check(await evaluate(`location.hash === "#letter-histogram" && (${editorText}).includes("struct Histogram")`), "picking an example loads it, runs it and names it in the address");

  // On the server: the choice is in the address, and a run goes to /play/render.
  await clickOn(radio("Where it runs", "Run on server"));
  await until("the server's output", `(document.querySelector("#out pre.term") || {}).textContent?.includes("from the stand-in server")`, 5000);
  check(await evaluate(`location.search === "?run=server"`), "choosing the server writes ?run=server");
  check((renders.at(-1) as { code?: string })?.code?.includes("struct Histogram") === true, "the server gets the editor's code");
  await shot("server.png");
  await clickOn(radio("Where it runs", "Run in browser"));

  // The phone: no sideways scroll, and the controls in three rows.
  await on("Emulation.setDeviceMetricsOverride", { width: 390, height: 844, deviceScaleFactor: 2, mobile: true });
  await on("Page.navigate", { url: `http://127.0.0.1:${port}/play` });
  await until("the island on a phone", `document.readyState === "complete" && !!document.querySelector("#playground .cm-content")`);
  await sleep(500);
  await shot("phone.png");
  check(await evaluate(`document.documentElement.scrollWidth <= innerWidth`), "a phone has no sideways scroll");
  const rows: number = await evaluate(`new Set([...document.querySelector(".tools").children].filter((e) => e.offsetParent).map((e) => Math.round(e.getBoundingClientRect().top))).size`);
  check(rows === 3, `the phone's controls sit in three rows (${rows})`);
  check(await evaluate(`[...document.querySelector(".tools").querySelectorAll("button, input")].every((e) => e.getBoundingClientRect().right <= innerWidth - 15)`), "every control fits inside the gutter");
  await on("Emulation.setDeviceMetricsOverride", { width: 1280, height: 800, deviceScaleFactor: 1, mobile: false });

  // The docs' app snippet: its Run button posts the code with the ?play
  // options its header shows, and shows the frame that comes back.
  await on("Page.navigate", { url: `http://127.0.0.1:${port}/docs/play#app` });
  await until("the docs' Run button", `!!document.querySelector('.runbtn[data-source="src-increment"]:not([hidden])')`, 10000);
  check(await evaluate(`document.querySelectorAll("#src-increment .k").length > 0`), "the docs' runnable code shows highlighted");
  await evaluate(`document.querySelector('.runbtn[data-source="src-increment"]').click()`);
  await until("the docs' frame", `!!document.querySelector("#out-increment img.frame")`, 10000);
  await shot("docs-app.png");
  const sent = siteRuns.at(-1) as { code: string; play?: string };
  check(sent?.play === "light msg=Message::Increment msg=Message::Increment" && sent.code.includes("struct Counter"), "the docs' app snippet posts its ?play options with its code");
  const snippet = parseSnippet(sent);
  check(typeof snippet !== "string" && snippet.messages.length === 2 && snippet.theme === "light", "those options are two Increment messages in the light theme");
  check(await evaluate(`document.documentElement.dataset.theme === "dark"`), "the docs open in the theme chosen on the playground");
} catch (error) {
  check(false, String(error));
} finally {
  console.log(logs.filter((line) => line.startsWith("rawr-web") || line.startsWith("exception")).join("\n"));
  socket.close();
  const exited = new Promise((done) => browser.once("exit", done));
  browser.kill();
  await exited;
  server.close();
  rmSync(profile, { recursive: true, force: true, maxRetries: 5 });
}
process.exit(failed ? 1 : 0);
