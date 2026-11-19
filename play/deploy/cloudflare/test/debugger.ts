// The browser universe's debugger (#2268, D1057.5) in a real headless
// Chrome, driven over the DevTools protocol with Node's own WebSocket:
//   RAWR_UNIVERSE_DEBUGGER=1 RAWR_IMPORT=target/release/rawr-import \
//     sh play/universe/web.sh "$RAWR_IMPORT"
//   node play/deploy/cloudflare/test/debugger.ts [SITE_DIR]
// SITE_DIR (default target/wasm/universe/site-debugger) holds the debugger
// build's universe.js and universe_bg.wasm; the universe's manifest is
// target/wasm/universe/Cargo.toml. The page instantiates the module as the
// playground's frame does and asks its `debug` export.
//
// A `main` that loops and sleeps pauses at a breakpoint two calls deep:
// the page keeps running its timers while it waits, and the pause answers
// a read of each frame, an `eval`, a step and a continue, after which the
// loop pauses again on its next turn and runs on once cleared. Then, in a
// fresh instance, iced's counter answers a read of its `view` at the view
// a click runs, with no pause. CHROME names the browser when it is not in
// the usual place.
import { spawn } from "node:child_process";
import { existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync } from "node:fs";
import { createServer } from "node:http";
import { join, resolve } from "node:path";

const repo = resolve(import.meta.dirname, "../../../..");
const site = resolve(process.argv[2] ?? join(repo, "target/wasm/universe/site-debugger"));
const manifest = join(repo, "target/wasm/universe/Cargo.toml");
const chrome = process.env.CHROME ?? "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome";
for (const file of [join(site, "universe.js"), join(site, "universe_bg.wasm"), manifest]) {
  if (!existsSync(file)) throw new Error(`no ${file}: build the debugger universe (RAWR_UNIVERSE_DEBUGGER=1 sh play/universe/web.sh)`);
}

/** The universe's paths name a snippet's crate root so. */
const ROOT = "/universe/src/main.rs";

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

/**
 * iced's counter example, as the universe covers it, its `view` with a
 * binding of its own, which no other function of it makes.
 */
const COUNTER = `use iced::Center;
use iced::widget::{Widget, button, column, text};

pub fn main() -> iced::Result {
    iced::run(Counter::update, Counter::view)
}

#[derive(Default)]
struct Counter {
    value: i64,
}

#[derive(Debug, Clone, Copy)]
enum Message {
    Increment,
    Decrement,
}

impl Counter {
    fn update(&mut self, message: Message) {
        match message {
            Message::Increment => {
                self.value += 1;
            }
            Message::Decrement => {
                self.value -= 1;
            }
        }
    }

    fn view(&self) -> impl Widget<Message> {
        let shown = self.value;
        column![
            button("Increment").on_press(Message::Increment),
            text(shown).size(50),
            button("Decrement").on_press(Message::Decrement)
        ]
        .padding(20)
        .align_x(Center)
    }
}
`;

// The page: the module instantiated once, its universe prepared, and the
// debugger's events and the program's output kept for the test to read.
const page = `<!doctype html><html><head><meta charset="utf-8">
<style>html, body { margin: 0; height: 100%; }</style></head>
<body><div id="iced"></div><script type="module">
import * as glue from "/universe.js";
await glue.default({ module_or_path: "/universe_bg.wasm" });
window.manifest = await (await fetch("/universe.toml")).text();
window.events = [];
window.output = "";
window.ticks = 0;
setInterval(() => window.ticks++, 10);
glue.prepare();
glue.debug_events((event) => window.events.push(JSON.parse(event)));
glue.set_output((_stream, text) => (window.output += text));
window.ask = async (request) => JSON.parse(await glue.debug(JSON.stringify(request)));
window.start = (code) => {
  window.answer = glue.run(window.manifest, code).catch((error) => String(error));
  return glue.sleeping();
};
window.ready = true;
</script></body></html>`;

const server = createServer((request, response) => {
  const path = new URL(request.url ?? "/", "http://localhost").pathname;
  const served: Record<string, [string, string]> = {
    "/universe.js": [join(site, "universe.js"), "text/javascript"],
    "/universe_bg.wasm": [join(site, "universe_bg.wasm"), "application/wasm"],
    "/universe.toml": [manifest, "text/plain"],
  };
  if (path === "/") return response.writeHead(200, { "content-type": "text/html; charset=utf-8" }).end(page);
  const file = served[path];
  if (!file) return response.writeHead(404).end(`no ${path}`);
  response.writeHead(200, { "content-type": file[1] }).end(readFileSync(file[0]));
});
await new Promise<void>((done) => server.listen(0, "127.0.0.1", done));
const port = (server.address() as { port: number }).port;

mkdirSync(join(repo, "target"), { recursive: true });
const profile = mkdtempSync(join(repo, "target/debugger-check-profile-"));
const browser = spawn(chrome, [
  "--headless=new", "--remote-debugging-port=0", `--user-data-dir=${profile}`, "--window-size=800,600",
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
/** The debugger's answer to `request`, which the page asks. */
const ask = (request: object) => evaluate(`ask(${JSON.stringify(request)})`);
const sleep = (ms: number) => new Promise((done) => setTimeout(done, ms));
const until = async (what: string, expression: string, ms = 30000) => {
  const start = Date.now();
  while (!(await evaluate(expression))) {
    if (Date.now() - start > ms) throw new Error(`timed out waiting for ${what}\n${logs.join("\n")}`);
    await sleep(50);
  }
};
const open = async () => {
  await on("Page.navigate", { url: `http://127.0.0.1:${port}/` });
  await until("the module", `window.ready === true`);
};

let failed = false;
const check = (ok: boolean, what: string, detail: unknown = "") => {
  console.log(`${ok ? "ok" : "FAILED"}: ${what}${ok ? "" : ` ${JSON.stringify(detail)}`}`);
  failed ||= !ok;
};
const names = (frames: { function: string }[]) => frames.map((frame) => frame.function);
const value = (answer: any, name: string) =>
  answer.bindings?.find((binding: { name: string }) => binding.name === name)?.value;

try {
  await on("Runtime.enable");
  await on("Page.enable");

  // A looping main, paused two calls deep while the page runs on.
  await open();
  check(await evaluate(`start(${JSON.stringify(LOOPING)})`), "a looping main runs on, sleeping");
  await until("its first turns", `output.includes("turn 2:")`);
  const set = await ask({ op: "break", file: ROOT, line: 2 });
  check(!set.error && set.set?.text === "let deep = depth * 2;", "a breakpoint reads its line from the program's files", set);
  // A name the program never binds: only the pause answers it.
  const paused = await ask({ name: "nothing" });
  check(paused.paused?.line === 2 && paused.paused?.function?.endsWith("inner") && paused.depth === 3, "main pauses two calls deep", paused);
  check(await evaluate(`events.some((event) => event.event === "paused")`), "the pause is told as an event");
  const [ticks, written] = await evaluate(`[ticks, output.length]`);
  await sleep(300);
  const [later, still] = await evaluate(`[ticks, output.length]`);
  check(later > ticks + 5, "the page's timers run while main is paused", { ticks, later });
  check(still === written, "the program writes nothing while paused", { written, still });
  const callers = await ask({ frame: 2 });
  check(callers.reached === true && names(callers.frames).every((name: string, at: number) => name.endsWith(["inner", "outer", "main"][at])), "inspect reads each frame up to main", callers);
  check(value(callers, "turn") !== undefined, "and main's own bindings", callers);
  const depth = await ask({ name: "depth" });
  const evaluated = await ask({ op: "eval", code: "depth + 1" });
  check(evaluated.value === String(Number(value(depth, "depth")) + 1), "eval answers in the paused frame", { depth, evaluated });
  const step = await ask({ op: "step", kind: "over" });
  check(step.paused?.line === 3, "a step over goes on a line", step);
  const deep = await ask({ name: "deep" });
  check(value(deep, "deep") === String(Number(value(depth, "depth")) * 2), "the step's frame reads the binding it made", deep);
  check(JSON.stringify(await ask({ op: "continue" })) === `{"paused":null}`, "continue answers at once");
  const again = await ask({ name: "nothing" });
  check(again.paused?.line === 2 && again.paused?.reason === "breakpoint", "the next turn pauses at the breakpoint again", again);
  await ask({ op: "clear" });
  await ask({ op: "continue" });
  const turns = await evaluate(`output.length`);
  await until("more turns", `output.length > ${turns}`, 5000);
  check(true, "cleared and continued, main loops on");

  // iced's counter: a read of its view answered at the view a click runs.
  await open();
  check(!(await evaluate(`start(${JSON.stringify(COUNTER)})`)), "iced's run hands its loop to the page");
  await until("the counter's canvas", `!!document.querySelector("canvas")`);
  await sleep(500);
  const read = evaluate(`ask({ name: "shown" })`);
  // The Increment button, at the top of the column inside its padding.
  const box = await evaluate(`(() => { const r = document.querySelector("canvas").getBoundingClientRect(); return { x: r.x + 60, y: r.y + 35 }; })()`);
  for (const type of ["mouseMoved", "mousePressed", "mouseReleased"]) {
    await on("Input.dispatchMouseEvent", { type, x: box.x, y: box.y, button: "left", clickCount: 1, pointerType: "mouse" });
  }
  const view = await Promise.race([read, sleep(10000).then(() => ({ timedOut: "no view ran after the click" }))]);
  check(view.reached === true && view.function?.endsWith("view") && !view.paused, "inspect on iced's view answers without a pause", view);
  check(value(view, "shown") === "1", "and reads the view's binding after the click's update", view);
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
