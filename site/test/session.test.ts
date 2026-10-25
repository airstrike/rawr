// node --test test/session.test.ts
// The playground's Session against stand-ins for the browser: where a run
// goes, what it shows, the address and the theme.
import assert from "node:assert/strict";
import { test } from "node:test";
import { Session, WEB_MODULE, type Example, type PlayData, type Web } from "../src/play/session.ts";

const EXAMPLES: Example[] = [
  { name: "Async tasks", code: "async fn main() {}", group: "Rust", slug: "async-tasks" },
  { name: "Counter", code: "counter code", messages: ["Message::Increment"], group: "iced", slug: "counter" },
];

interface Options {
  code?: string;
  web?: Partial<Web> | null;
  gpu?: boolean;
  url?: string;
  shared?: string | null;
  examples?: Example[];
  sitekey?: string | null;
  pass?: boolean;
  draft?: string | null;
  theme?: "dark" | "light";
}

/** A Session on stand-ins, with what it did. */
function standIn(options: Options = {}) {
  const { code = "", web = null, gpu = true, url = "http://localhost/play", shared = null, examples = [], sitekey = null, pass = false, draft = null, theme = "dark" } = options;
  const data: PlayData = { snippet: { code, theme: "dark", size: "", messages: [] }, examples, sitekey, pass, shared };
  const parsed = new URL(url);
  const loads: string[] = [];
  const fetched: { url: string; body: any }[] = [];
  const addresses: string[] = [];
  const stored = new Map<string, string>(draft ? [["rawr-play-draft", draft]] : []);
  const timers: (() => void)[] = [];
  const holder = { className: "live", style: {} as Record<string, string>, classes: new Set<string>(), classList: { toggle(this: void, _n: string, _on: boolean) {} } };
  holder.classList.toggle = (name: string, on: boolean) => void (on ? holder.classes.add(name) : holder.classes.delete(name));
  let attached = false;
  let respond: (url: string) => Response = () =>
    Response.json({ kind: "text", status: 0, output: [{ stream: "stdout", text: "from the server\n" }] });
  const widgets: string[] = [];
  const session = new Session(
    data,
    {
      fetch: (async (input: string, init?: { body?: string }) => {
        fetched.push({ url: input, body: init?.body ? JSON.parse(init.body) : null });
        return respond(input);
      }) as typeof fetch,
      location: { pathname: parsed.pathname, search: parsed.search, hash: parsed.hash, origin: parsed.origin },
      replaceState: (to) => void addresses.push(to),
      storage: { getItem: (key) => stored.get(key) ?? null, setItem: (key, value) => void stored.set(key, value) },
      loadWeb: async () => {
        loads.push(WEB_MODULE);
        if (!web) throw new Error("404");
        return web as Web;
      },
      gpu: () => gpu,
      now: () => 0,
      clipboard: async () => {},
      turnstile: () => ({ render: () => (widgets.push("render"), "w1"), reset: () => void widgets.push("reset"), remove: () => void widgets.push("remove") }),
      loadTurnstile: () => {},
      live: { holder: () => holder, attach: (on) => void (attached = on) },
      setTimeout: (f) => (timers.push(f), timers.length),
      clearTimeout: () => {},
    },
    theme,
  );
  return {
    session, loads, fetched, addresses, stored, holder, widgets,
    attached: () => attached,
    respond: (next: (url: string) => Response) => void (respond = next),
    /** Runs the timers set so far, as their time passing would. */
    tick: () => timers.splice(0).forEach((f) => f()),
  };
}

test("Run in browser runs a plain program in rawr-web and shows its output as the server's", async () => {
  const page = standIn({ code: 'println!("hi")', web: { kind: () => "text", run_text: () => JSON.stringify({ kind: "text", status: 3, output: [{ stream: "stdout", text: "hi\n" }] }) } });
  await page.session.go();
  assert.deepEqual(page.loads, [WEB_MODULE]);
  assert.deepEqual(page.fetched, [], "the server is not asked");
  assert.equal(page.session.state.output.kind, "term");
  assert.match(page.session.state.status, /in the browser · exit 3$/);
  assert.ok(page.session.state.plain, "messages are marked unused");
});

test("Run in browser mounts an iced snippet on the canvas holder and runs it there in the page's theme", async () => {
  const calls: unknown[][] = [];
  const page = standIn({ code: 'text("hi")', theme: "light", web: { kind: () => "iced", run: (...args: unknown[]) => void calls.push(args) } });
  page.session.setSize("200x100");
  await page.session.go();
  assert.deepEqual(calls, [['text("hi")', "light", []]]);
  assert.ok(page.attached(), "the holder is on the page before rawr-web looks for #iced");
  assert.equal(page.session.state.output.kind, "live");
  assert.equal(page.holder.style.width, "200px");
  assert.ok(page.holder.classes.has("sized"));
  assert.match(page.session.state.status, /· live$/);
  assert.deepEqual(page.fetched, []);
});

test("a broken edit keeps the running app with its report under it", async () => {
  let fail = false;
  const page = standIn({ code: "x", web: { kind: () => "iced", run: () => { if (fail) throw "error: expected `)`"; } } });
  await page.session.go();
  fail = true;
  await page.session.go();
  assert.deepEqual(page.session.state.output, { kind: "live", report: "error: expected `)`" });
  assert.equal(page.session.state.status, "error");
});

test("Run in browser falls back to the server when the module does not load or cannot draw", async () => {
  const failed = standIn({ code: "1 + 2", web: null });
  await failed.session.go();
  assert.deepEqual(failed.fetched.map((f) => f.url), ["/play/render"]);
  assert.match(failed.session.state.status, /on the server: rawr-web did not load$/);
  assert.equal(failed.session.state.where, "server", "the choice goes back to the server");
  assert.deepEqual(failed.addresses, [], "the fallback does not write ?run=server");
  const iced = standIn({ code: 'text("hi")', gpu: false, web: { kind: () => "iced", run() { throw new Error("not called"); } } });
  await iced.session.go();
  assert.deepEqual(iced.fetched.map((f) => f.url), ["/play/render"]);
  assert.match(iced.session.state.status, /no WebGL2 or WebGPU$/);
  const later = standIn({ code: "1 + 2", web: { kind: () => "server" } });
  await later.session.go();
  assert.match(later.session.state.status, /rawr-web cannot run this yet$/);
});

test("Run in browser runs an async main and shows its output as it is written", async () => {
  const shown: number[] = [];
  const holder: { page?: ReturnType<typeof standIn> } = {};
  holder.page = standIn({
    code: "async fn main() {}",
    web: {
      kind: () => "async",
      run_async: async (_code, write) => {
        write("stdout", "one\n");
        const out = holder.page!.session.state.output;
        shown.push(out.kind === "term" && !out.done ? out.chunks.length : -1);
        write("stderr", "two\n");
        return JSON.stringify({ kind: "text", status: 1, output: [{ stream: "stdout", text: "one\n" }, { stream: "stderr", text: "two\n" }] });
      },
    },
  });
  await holder.page.session.go();
  assert.deepEqual(shown, [1], "the first chunk shows before the run ends");
  const out = holder.page.session.state.output;
  assert.ok(out.kind === "term" && out.done && out.chunks.length === 2);
  assert.match(holder.page.session.state.status, /in the browser · exit 1$/);
  assert.deepEqual(holder.page.fetched, [], "the server is not asked");
});

test("the server answers with text or a refusal", async () => {
  const page = standIn({ code: "x" });
  page.session.choose("server");
  await new Promise((done) => setTimeout(done, 0));
  assert.deepEqual(page.fetched.at(-1)?.body.code, "x");
  assert.equal(page.session.state.output.kind, "term");
  page.respond(() => new Response("slow down", { status: 429 }));
  await page.session.go();
  assert.deepEqual(page.session.state.output, { kind: "error", text: "slow down" });
  assert.equal(page.session.state.status, "slow down");
});

test("the address restores an example from its hash and the server from ?run=server", () => {
  const page = standIn({ url: "http://localhost/play?run=server#counter", examples: EXAMPLES });
  assert.equal(page.session.state.code, "counter code");
  assert.equal(page.session.state.messages, "Message::Increment");
  assert.equal(page.session.state.where, "server");
  assert.deepEqual(page.addresses, ["/play?run=server#counter"]);
  // The browser is the default: ?run=browser is dropped, and the hash kept.
  const browser = standIn({ url: "http://localhost/play?run=browser#async-tasks", examples: EXAMPLES });
  assert.equal(browser.session.state.code, "async fn main() {}");
  assert.equal(browser.session.state.where, "browser");
  assert.deepEqual(browser.addresses, ["/play#async-tasks"]);
  // A bare address writes nothing.
  assert.deepEqual(standIn({ examples: EXAMPLES }).addresses, []);
});

test("bare /play restores the draft; a share shows the share", () => {
  const draft = JSON.stringify({ code: "my draft", theme: "light", size: "10x10", messages: ["A"] });
  const page = standIn({ code: "first example", draft });
  assert.equal(page.session.state.code, "my draft");
  assert.equal(page.session.state.size, "10x10");
  assert.equal(page.session.state.theme, "dark", "the page's theme, not the draft's");
  const shared = standIn({ code: "shared code", shared: "abcDEF1234", url: "http://localhost/play/abcDEF1234", draft });
  assert.equal(shared.session.state.code, "shared code");
  assert.equal(shared.session.state.status, "shared · abcDEF1234");
});

test("a share's code wins over an example's hash, which the share page drops", () => {
  const page = standIn({ code: "shared code", url: "http://localhost/play/abcDEF1234?run=server#counter", shared: "abcDEF1234", examples: EXAMPLES });
  assert.equal(page.session.state.code, "shared code");
  assert.deepEqual(page.addresses, ["/play/abcDEF1234?run=server"]);
});

test("a share's link keeps ?run=server and drops the example's hash", async () => {
  const page = standIn({ code: "x", url: "http://localhost/play?run=server#counter", examples: EXAMPLES });
  const copied: string[] = [];
  page.session.deps.clipboard = async (text) => void copied.push(text);
  page.respond(() => Response.json({ id: "abcDEF1234" }));
  await page.session.share();
  assert.equal(page.addresses.at(-1), "/play/abcDEF1234?run=server");
  assert.deepEqual(copied, ["http://localhost/play/abcDEF1234?run=server"]);
  assert.equal(page.session.state.status, "link copied · abcDEF1234");
});

test("choosing an example, the run mode and an edit keep the address with replaceState", async () => {
  const web: Partial<Web> = { kind: () => "text", run_text: () => JSON.stringify({ kind: "text", status: 0, output: [] }) };
  const page = standIn({ code: "x", url: "http://localhost/play/abcDEF1234", shared: "abcDEF1234", examples: EXAMPLES, web });
  page.session.pick("counter");
  assert.equal(page.session.state.code, "counter code");
  assert.equal(page.session.state.mark, "counter");
  assert.equal(page.addresses.at(-1), "/play#counter");
  page.session.choose("server");
  assert.equal(page.addresses.at(-1), "/play?run=server#counter");
  // An edit drops the hash: the code is no longer the example's.
  page.session.setCode("counter code, edited");
  assert.equal(page.addresses.at(-1), "/play?run=server");
  page.session.choose("browser");
  assert.equal(page.addresses.at(-1), "/play");
  page.session.pick("reset");
  assert.equal(page.session.state.code, "async fn main() {}", "Reset is the first example");
  await new Promise((done) => setTimeout(done, 10));
});

test("in the browser an edit or a change of theme runs again after a pause; on the server it waits for Run", async () => {
  const ran: string[] = [];
  const page = standIn({ code: "x", web: { kind: () => "iced", run: (code: string, theme: string) => void ran.push(`${code} ${theme}`) } });
  page.session.setCode("y");
  page.session.setTheme("light");
  page.tick();
  await new Promise((done) => setTimeout(done, 0));
  assert.deepEqual(ran, ["y light"]);
  page.session.choose("server");
  await new Promise((done) => setTimeout(done, 0));
  const asked = page.fetched.length;
  page.session.setCode("z");
  page.tick();
  assert.equal(page.fetched.length, asked);
});

test("in the browser, rawr-web loads once the page has loaded, and a restored example runs", async () => {
  const ran: string[] = [];
  const web: Partial<Web> = { kind: () => "text", run_text: (code: string) => (ran.push(code), JSON.stringify({ kind: "text", status: 0, output: [] })) };
  const page = standIn({ url: "http://localhost/play#async-tasks", examples: EXAMPLES, web });
  assert.deepEqual(page.loads, [], "nothing loads before the page has");
  page.session.preload();
  await new Promise((done) => setTimeout(done, 10));
  assert.deepEqual(page.loads, [WEB_MODULE]);
  assert.deepEqual(ran, ["async fn main() {}"]);
  // On the server, nothing loads.
  const server = standIn({ url: "http://localhost/play?run=server", web });
  server.session.preload();
  await new Promise((done) => setTimeout(done, 10));
  assert.deepEqual(server.loads, []);
});

test("a pass skips Turnstile until the server refuses it", async () => {
  const page = standIn({ code: "x", sitekey: "0xKEY", pass: true, url: "http://localhost/play?run=server" });
  assert.deepEqual(page.widgets, [], "no widget with a pass");
  let calls = 0;
  page.respond(() => (++calls === 1 ? new Response("no", { status: 403 }) : Response.json({ kind: "text", status: 0, output: [] })));
  const running = page.session.go();
  // The refused pass mounts the widget, and the run waits for its token.
  await new Promise((done) => setTimeout(done, 0));
  assert.deepEqual(page.widgets, ["render"]);
  // The run now waits for the widget's token; it is not awaited here.
  void running;
  const fresh = standIn({ code: "x", sitekey: "0xKEY" });
  assert.deepEqual(fresh.widgets, ["render"], "without a pass the widget mounts at once");
});
