// The playground and the docs' Run buttons in a real headless Chrome, driven
// over the DevTools protocol with Node's own WebSocket:
//   web/build.sh && npm run build --prefix site && node test/browser.ts [WEB_DIR] [SHOTS_DIR]
// It serves the site's build (site/dist) as the Worker does: /play is
// play.html with the request's data written in (playpage.ts's inject), and
// /web is rawr-web from WEB_DIR (default target/wasm/site). On the
// playground's island it runs a plain program in the browser (the default),
// edits it and sees the output follow, checks that an iced snippet and an
// async one each show their message, restores an example from its address,
// picks one from the menu, runs a program on a stand-in server, and checks
// the phone layout. Then it opens the docs' Play page and runs its snippet.
// It saves window screenshots in SHOTS_DIR (default target/web-check).
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
const histogram = readFileSync(join(repo, "play/tests/fixtures/rust/histogram.rs"), "utf8");
const wordCount = readFileSync(join(repo, "play/tests/fixtures/rust/word_count.rs"), "utf8");
mkdirSync(shots, { recursive: true });

// The site's build, served as the Worker's assets serve it.
const site = join(repo, "site/dist");
if (!existsSync(join(site, "play.html"))) throw new Error("no site/dist/play.html: run `npm run build` in site/");
const og = { title: "rawr playground", description: "", url: "http://localhost/play", image: "", alt: "" };
const examples = [
  { name: "Letter histogram", code: histogram, group: "Rust" },
  { name: "Word count", code: wordCount, group: "Rust" },
];
const page = inject(readFileSync(join(site, "play.html"), "utf8"), {
  snippet: { code: histogram }, examples, sitekey: null, shared: null, og,
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
    return response.writeHead(200, { "content-type": "application/json" }).end(JSON.stringify({ kind: "text", status: 0, output: [{ stream: "stdout", text: "6\n" }] }));
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
/** Picks an example from the menu, as a visitor does. */
const pick = async (name: string) => {
  await clickOn(`document.querySelector('[aria-label="Examples"]')`);
  await until("the menu", `!!document.querySelector("[role=listbox]")`, 5000);
  await clickOn(`[...document.querySelectorAll("[role=option]")].find((o) => o.textContent === ${JSON.stringify(name)})`);
};
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
  await on("Page.navigate", { url: `http://127.0.0.1:${port}/play` });
  await until("the island", `document.readyState === "complete" && !!document.querySelector("#playground .cm-content")`);
  check(await evaluate(`!document.querySelector("script[src*=rawr_web]")`), "nothing loads rawr-web in the HTML");
  check(await evaluate(`${checked("Where it runs")} === "Run in browser"`), "the browser is the default");
  check(await evaluate(`document.documentElement.dataset.theme === "dark"`), "the page follows the system's dark theme");
  check(await evaluate(`(${editorText}).includes("struct Histogram")`), "the editor holds the page's snippet");
  check(await evaluate(`document.querySelectorAll("#playground .cm-content .k").length > 10`), "the editor highlights Rust in the site's classes");
  await evaluate(`document.getElementById("run").click()`);
  await until("the output", `(document.querySelector("#out pre.term") || {}).textContent?.includes("#")`, 15000);
  await shot("histogram.png");
  check(await evaluate(`document.getElementById("status").textContent.includes("in the browser")`), "a plain program runs in the browser");
  check(brotli.includes("/web/rawr_web_bg.wasm"), "the module comes as its brotli copy");

  // An edit runs again after a pause, and the output follows.
  await setCode('for i in 1..=3 {\n    println!("line {i}");\n}\neprintln!("done");');
  await until("the edited output", `(document.querySelector("#out pre.term") || {}).textContent?.includes("line 3")`, 5000);
  await shot("plain.png");
  check(await evaluate(`document.querySelector("#out pre.term .err")?.textContent === "done\\n"`), "stderr shows in its own colour");

  // An iced snippet and an async one each show their message.
  await setCode('column![text("hi"), button("go")]');
  await until("the iced message", `(document.querySelector("#out .note") || {}).textContent?.startsWith("iced programs don't run here yet.")`, 5000);
  await shot("iced.png");
  check(true, "an iced snippet shows its message");
  await setCode("#[tokio::main]\nasync fn main() {\n    println!(\"hi\");\n}");
  await until("the async message", `(document.querySelector("#out .note") || {}).textContent?.startsWith("async programs")`, 5000);
  check(await evaluate(`document.getElementById("status").textContent === "not available here yet"`), "an async snippet shows its message");

  // An example from its address: restored and run once the module is up.
  // From another page: a hash change alone would not load the page again.
  await on("Page.navigate", { url: "about:blank" });
  await on("Page.navigate", { url: `http://127.0.0.1:${port}/play#word-count` });
  await until("the restored example", `(document.querySelector("#out pre.term") || {}).textContent?.length > 0`, 15000);
  check(await evaluate(`document.querySelector('[aria-label="Examples"]').textContent.includes("Word count")`), "#word-count restores the example");
  check(await evaluate(`location.hash === "#word-count" && location.search === ""`), "the address keeps the example and the default mode");

  // The examples menu: a pick runs the example and names it in the address.
  await pick("Letter histogram");
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

  // The docs' snippet: its Run button posts the code and shows the output.
  await on("Page.navigate", { url: `http://127.0.0.1:${port}/docs/play#shapes` });
  await until("the docs' Run button", `!!document.querySelector('.runbtn[data-source="src-sum"]:not([hidden])')`, 10000);
  check(await evaluate(`document.querySelectorAll("#src-sum .k").length > 0`), "the docs' runnable code shows highlighted");
  await evaluate(`document.querySelector('.runbtn[data-source="src-sum"]').click()`);
  for (const start = Date.now(); siteRuns.length === 0 && Date.now() - start < 10000; ) await sleep(100);
  await until("the docs' output", `(document.getElementById("out-sum") || {}).textContent?.includes("6")`, 10000);
  await shot("docs-sum.png");
  const sent = siteRuns.at(-1) as { code: string };
  check(sent?.code.includes("xs.iter().sum") === true, "the docs' snippet posts its code");
  check(typeof parseSnippet(sent) !== "string", "and the Worker reads it");
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
