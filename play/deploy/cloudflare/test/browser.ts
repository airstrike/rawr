// The playground's "Run in browser" in a real headless Chrome, driven over the
// DevTools protocol with Node's own WebSocket:
//   web/build.sh && node test/browser.ts [WEB_DIR] [SHOTS_DIR]
// It serves the page (playpage.ts) with rawr-web from WEB_DIR (default
// target/wasm/site), runs the Counter in the browser (the default), clicks
// Increment three times, makes an edit that keeps the count, runs a plain
// program, an iced app whose task sleeps, and the "Async tasks" example from
// its #async-tasks address on the page's real clock, and saves window
// screenshots in SHOTS_DIR (default target/web-check).
// CHROME names the browser when it is not in the usual place.
import { spawn } from "node:child_process";
import { mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { createServer } from "node:http";
import { tmpdir } from "node:os";
import { extname, join, resolve } from "node:path";
import { webAsset } from "../src/play.ts";
import { render } from "../src/playpage.ts";

const repo = resolve(import.meta.dirname, "../../../..");
const web = resolve(process.argv[2] ?? join(repo, "target/wasm/site"));
const shots = resolve(process.argv[3] ?? join(repo, "target/web-check"));
const chrome = process.env.CHROME ?? "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome";
const counter = readFileSync(join(repo, "play/tests/fixtures/counter.rs"), "utf8");
const tasks = readFileSync(join(repo, "play/tests/fixtures/rust/tasks.rs"), "utf8");
const channel = readFileSync(join(repo, "play/tests/fixtures/channel.rs"), "utf8");
mkdirSync(shots, { recursive: true });

const og = { title: "rawr playground", description: "", url: "http://localhost/play", image: "", alt: "" };
const brotli: string[] = [];
const page = render({ snippet: { code: counter, theme: "dark", size: "", messages: [] }, examples: [{ name: "Async tasks", code: tasks, group: "Rust" }], sitekey: null, shared: null, og });
const server = createServer((request, response) => {
  const path = new URL(request.url ?? "/", "http://localhost").pathname;
  if (path === "/play") return response.writeHead(200, { "content-type": "text/html; charset=utf-8" }).end(page);
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
    webAsset(new Request(`http://127.0.0.1${path}`, { headers }), assets).then(async (served) => {
      if (served.headers.get("content-encoding")) brotli.push(path);
      response.writeHead(served.status, Object.fromEntries(served.headers)).end(Buffer.from(await served.arrayBuffer()));
    });
    return;
  }
  response.writeHead(404).end(`no ${path} ${extname(path)}`);
});
await new Promise<void>((done) => server.listen(0, "127.0.0.1", done));
const port = (server.address() as { port: number }).port;

const profile = mkdtempSync(join(tmpdir(), "rawr-web-check-"));
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
const until = async (what: string, expression: string, ms = 30000) => {
  const start = Date.now();
  while (!(await evaluate(expression))) {
    if (Date.now() - start > ms) throw new Error(`timed out waiting for ${what}\n${logs.join("\n")}`);
    await new Promise((done) => setTimeout(done, 100));
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
    await on("Input.dispatchMouseEvent", { type, x, y, button: "left", clickCount: 1 });
  }
};

let failed = false;
const check = (ok: boolean, what: string) => {
  console.log(`${ok ? "ok" : "FAILED"}: ${what}`);
  failed ||= !ok;
};
try {
  await on("Runtime.enable");
  await on("Page.enable");
  await on("Emulation.setDeviceMetricsOverride", { width: 1280, height: 800, deviceScaleFactor: 1, mobile: false });
  const loaded = performance.now();
  await on("Page.navigate", { url: `http://127.0.0.1:${port}/play` });
  await until("the page", `document.readyState === "complete" && !!document.getElementById("out")`);
  // The capture listener starts a click's clock and the bubbling one stops it: iced's handlers run between.
  await evaluate(`window.clicks = []; let t = 0;
    addEventListener("pointerup", () => { t = performance.now(); }, true);
    addEventListener("pointerup", () => { const handled = performance.now() - t; requestAnimationFrame(() => requestAnimationFrame(() => window.clicks.push([handled, performance.now() - t]))); });`);
  check(await evaluate(`typeof window.rawr_web === "undefined" && !document.querySelector("script[src*=rawr_web]")`), "nothing loads rawr-web in the HTML");
  check(await evaluate(`document.querySelector("input[name=where]:checked").value === "browser"`), "the browser is the default");
  await evaluate(`document.getElementById("run").click()`);
  await until("the canvas", `!!document.querySelector("#out .live canvas") && document.getElementById("status").textContent.includes("live")`);
  await until("the first frame", `document.querySelector("#out .live canvas").width > 0`);
  console.log(`canvas up ${Math.round(performance.now() - loaded)} ms after navigation`);
  await new Promise((done) => setTimeout(done, 500));
  const before = await out();
  await shot("counter.png");
  const box = await evaluate(`(() => { const r = document.querySelector("#out .live canvas").getBoundingClientRect(); return { x: r.x, y: r.y }; })()`);
  // The Counter's column: 20 px of padding, then Increment centred above the wider Decrement.
  for (let i = 0; i < 3; i++) {
    await click(box.x + 69, box.y + 36);
    await new Promise((done) => setTimeout(done, 150));
  }
  await new Promise((done) => setTimeout(done, 300));
  const clicked = await out();
  await shot("counter-clicked.png");
  check(clicked !== before, "three clicks on Increment change the canvas");
  const clicks: number[][] = await evaluate("window.clicks");
  console.log(`click handlers: ${clicks.map(([h]) => h.toFixed(1)).join(", ")} ms; to the next frame: ${clicks.map(([, f]) => f.toFixed(1)).join(", ")} ms`);

  // An edit: ten per click, relabelled. The count carries over.
  await evaluate(`(() => { const code = document.getElementById("code");
    code.value = code.value.replace("self.value += 1", "self.value += 10").replace('button("Increment")', 'button("Add ten")');
    code.dispatchEvent(new Event("input")); })()`);
  await new Promise((done) => setTimeout(done, 1500));
  await shot("counter-edited.png");
  check(logs.some((line) => line.includes("state carried")), "an edit swaps in a new generation and keeps the count");
  await click(box.x + 69, box.y + 36);
  await new Promise((done) => setTimeout(done, 400));
  await shot("counter-edited-clicked.png");

  // A broken edit keeps the app and shows the report under it.
  await evaluate(`(() => { const code = document.getElementById("code"); code.value = code.value.replace("fn update", "fn update("); code.dispatchEvent(new Event("input")); })()`);
  await until("the report", `!!document.querySelector("#out .live + pre.error")`, 5000);
  await shot("counter-error.png");
  check(await evaluate(`!!document.querySelector("#out .live canvas")`), "a broken edit keeps the running app");

  // Monospace text and symbols, from the fonts rawr-web fetches.
  await evaluate(`(() => { const code = document.getElementById("code");
    code.value = 'column![text("Fira Mono: let x = 42;").font(Font::MONOSPACE).size(24), text("Symbols: → ★ ✓ ∑ ⌘").size(24), text("CJK: 你好 こんにちは").size(24)].spacing(12).padding(20)';
    code.dispatchEvent(new Event("input")); })()`);
  const fonts = ["FiraMono", "NotoSansSymbols-", "NotoSansSymbols2", "NotoSansCJK"];
  const start = Date.now();
  while (!fonts.every((font) => logs.some((line) => line.includes(`font ${font}`) && line.includes("Ok")))) {
    if (Date.now() - start > 30000 || logs.some((line) => line.includes("did not load"))) break;
    await new Promise((done) => setTimeout(done, 200));
  }
  console.log(`fonts in ${Date.now() - start} ms`);
  await new Promise((done) => setTimeout(done, 500));
  await shot("fonts.png");
  check(fonts.every((font) => logs.some((line) => line.includes(`font ${font}`) && line.includes("Ok"))), "the fonts load");

  // A plain program runs in the browser and shows its output as text.
  await evaluate(`(() => { const code = document.getElementById("code");
    code.value = 'for i in 1..=3 {\\n    println!("line {i}");\\n}\\neprintln!("done");';
    code.dispatchEvent(new Event("input")); })()`);
  await until("the output", `(document.querySelector("#out pre.term") || {}).textContent?.includes("line 3")`, 5000);
  await shot("plain.png");
  console.log("plain output:", JSON.stringify(await evaluate(`document.querySelector("#out pre.term").textContent`)), "·", await evaluate(`document.getElementById("status").textContent`));
  check(true, "a plain program runs in the browser");

  // An iced app whose boot task sleeps and joins a channel's two ends: "adding up", then "sum 10" 400 ms on.
  await evaluate(`(() => { const code = document.getElementById("code"); code.value = ${JSON.stringify(channel)}; code.dispatchEvent(new Event("input")); })()`);
  await until("the app", `!!document.querySelector("#out .live canvas") && document.getElementById("status").textContent.includes("live")`, 5000);
  await new Promise((done) => setTimeout(done, 100));
  const adding = await out();
  await new Promise((done) => setTimeout(done, 1200));
  const summed = await out();
  await shot("iced-task.png");
  check(adding !== summed && !logs.some((line) => line.includes("does not run in a browser")), "an iced app's task sleeps on the page's clock and ends");

  // The "Async tasks" example from its address: restored and run in the browser once the module is up.
  // From another page: a hash change alone would not load the page again.
  await on("Page.navigate", { url: "about:blank" });
  await on("Page.navigate", { url: `http://127.0.0.1:${port}/play#async-tasks` });
  await until("the restored example", `(document.querySelector("#out pre.term") || {}).textContent?.includes("jobs in")`, 15000);
  check(await evaluate(`document.getElementById("code").value.includes("mpsc::channel(4)")`), "#async-tasks restores the example");
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
