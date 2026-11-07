// node --test test/session.test.ts
// The playground's Session against stand-ins for the browser: where a run
// goes, what it shows, the address and the theme.
import assert from "node:assert/strict";
import { test } from "node:test";
import { Session, WEB_MODULE, type Example, type PlayData, type Web } from "../src/play/session.ts";

const EXAMPLES: Example[] = [
  { name: "Word count", code: "word count code", group: "Rust", slug: "word-count" },
  { name: "Traits and generics", code: "traits code", group: "Rust", slug: "traits" },
];

interface Options {
  code?: string;
  web?: Partial<Web> | null;
  url?: string;
  shared?: string | null;
  examples?: Example[];
  sitekey?: string | null;
  pass?: boolean;
  draft?: string | null;
  theme?: "dark" | "light";
  /** Whether #ts is in the page yet. */
  container?: boolean;
}

/** A Session on stand-ins, with what it did. */
function standIn(options: Options = {}) {
  const { code = "", web = null, url = "http://localhost/play", shared = null, examples = [], sitekey = null, pass = false, draft = null, theme = "dark", container = true } = options;
  const data: PlayData = { snippet: { code }, examples, sitekey, pass, shared };
  const parsed = new URL(url);
  const loads: string[] = [];
  const fetched: { url: string; body: any }[] = [];
  const addresses: string[] = [];
  const stored = new Map<string, string>(draft ? [["rawr-play-draft", draft]] : []);
  const timers: (() => void)[] = [];
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
      now: () => 0,
      clipboard: async () => {},
      turnstile: () => ({ render: () => (widgets.push("render"), "w1"), reset: () => void widgets.push("reset"), remove: () => void widgets.push("remove") }),
      loadTurnstile: () => {},
      hasContainer: () => container,
      setTimeout: (f) => (timers.push(f), timers.length),
      clearTimeout: () => {},
    },
    theme,
  );
  return {
    session, loads, fetched, addresses, stored, widgets,
    respond: (next: (url: string) => Response) => void (respond = next),
    /** Runs the timers set so far, as their time passing would. */
    tick: () => timers.splice(0).forEach((f) => f()),
  };
}

test("Run in browser runs a plain program in rawr-web and shows its output as the server's", async () => {
  const page = standIn({ code: 'println!("hi")', web: { run_text: () => JSON.stringify({ kind: "text", status: 3, output: [{ stream: "stdout", text: "hi\n" }] }) } });
  await page.session.go();
  assert.deepEqual(page.loads, [WEB_MODULE]);
  assert.deepEqual(page.fetched, [], "the server is not asked");
  assert.equal(page.session.state.output.kind, "term");
  assert.match(page.session.state.status, /in the browser · exit 3$/);
});

test("a snippet that does not compile shows its report", async () => {
  const page = standIn({ code: "x", web: { run_text: () => JSON.stringify({ kind: "error", text: "error: expected `)`" }) } });
  await page.session.go();
  assert.deepEqual(page.session.state.output, { kind: "error", text: "error: expected `)`" });
  assert.equal(page.session.state.status, "error");
});

test("Run in browser falls back to the server when the module does not load", async () => {
  const failed = standIn({ code: "1 + 2", web: null });
  await failed.session.go();
  assert.deepEqual(failed.fetched.map((f) => f.url), ["/play/render"]);
  assert.match(failed.session.state.status, /on the server: rawr-web did not load$/);
  assert.equal(failed.session.state.where, "server", "the choice goes back to the server");
  assert.deepEqual(failed.addresses, [], "the fallback does not write ?run=server");
});

test("a trap in rawr-web runs that program on the server and the next on a fresh module", async () => {
  let traps = 1;
  const web: Partial<Web> = {
    run_text: () => {
      if (traps-- > 0) throw new Error("RuntimeError: unreachable");
      return JSON.stringify({ kind: "text", status: 0, output: [{ stream: "stdout", text: "in the browser\n" }] });
    },
  };
  const page = standIn({ code: "1 + 2", web });
  await page.session.go();
  assert.deepEqual(page.fetched.map((f) => f.url), ["/play/render"]);
  assert.match(page.session.state.status, /on the server: rawr-web stopped$/);
  assert.equal(page.session.state.where, "browser", "the next run is in the browser again");
  await page.session.go();
  assert.equal(page.loads.length, 2, "the next run loads a fresh module");
  assert.equal(page.fetched.length, 1);
  assert.match(page.session.state.status, /in the browser$/);
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

test("the server answers an iced program with its text and its frame", async () => {
  const page = standIn({ code: "iced::run(update, view)" });
  const result = {
    kind: "text",
    status: 0,
    output: [{ stream: "stdout", text: "counting\n" }],
    frame: { type: "image/gif", data: "R0lGODlh" },
  };
  page.respond(() => Response.json(result));
  page.session.choose("server");
  await new Promise((done) => setTimeout(done, 0));
  const output = page.session.state.output;
  assert.equal(output.kind, "term");
  assert.equal(output.kind === "term" ? output.image : "", "data:image/gif;base64,R0lGODlh");
  assert.deepEqual(output.kind === "term" ? output.chunks : [], result.output);
  assert.equal(page.session.state.status, "0 ms");
});

test("the address restores an example from its hash and the server from ?run=server", () => {
  const page = standIn({ url: "http://localhost/play?run=server#traits", examples: EXAMPLES });
  assert.equal(page.session.state.code, "traits code");
  assert.equal(page.session.state.where, "server");
  assert.deepEqual(page.addresses, ["/play?run=server#traits"]);
  // The browser is the default: ?run=browser is dropped, and the hash kept.
  const browser = standIn({ url: "http://localhost/play?run=browser#word-count", examples: EXAMPLES });
  assert.equal(browser.session.state.code, "word count code");
  assert.equal(browser.session.state.where, "browser");
  assert.deepEqual(browser.addresses, ["/play#word-count"]);
  // A bare address writes nothing.
  assert.deepEqual(standIn({ examples: EXAMPLES }).addresses, []);
});

test("bare /play restores the draft; a share shows the share", () => {
  const draft = JSON.stringify({ code: "my draft", theme: "light" });
  const page = standIn({ code: "first example", draft });
  assert.equal(page.session.state.code, "my draft");
  assert.equal(page.session.state.theme, "dark", "the page's theme, not the draft's");
  const shared = standIn({ code: "shared code", shared: "abcDEF1234", url: "http://localhost/play/abcDEF1234", draft });
  assert.equal(shared.session.state.code, "shared code");
  assert.equal(shared.session.state.status, "shared · abcDEF1234");
});

test("a share's code wins over an example's hash, which the share page drops", () => {
  const page = standIn({ code: "shared code", url: "http://localhost/play/abcDEF1234?run=server#traits", shared: "abcDEF1234", examples: EXAMPLES });
  assert.equal(page.session.state.code, "shared code");
  assert.deepEqual(page.addresses, ["/play/abcDEF1234?run=server"]);
});

test("a share's link keeps ?run=server and drops the example's hash", async () => {
  const page = standIn({ code: "x", url: "http://localhost/play?run=server#traits", examples: EXAMPLES });
  const copied: string[] = [];
  page.session.deps.clipboard = async (text) => void copied.push(text);
  page.respond(() => Response.json({ id: "abcDEF1234" }));
  await page.session.share();
  assert.equal(page.addresses.at(-1), "/play/abcDEF1234?run=server");
  assert.deepEqual(copied, ["http://localhost/play/abcDEF1234?run=server"]);
  assert.equal(page.session.state.status, "link copied · abcDEF1234");
});

test("choosing an example, the run mode and an edit keep the address with replaceState", async () => {
  const web: Partial<Web> = { run_text: () => JSON.stringify({ kind: "text", status: 0, output: [] }) };
  const page = standIn({ code: "x", url: "http://localhost/play/abcDEF1234", shared: "abcDEF1234", examples: EXAMPLES, web });
  page.session.pick("traits");
  assert.equal(page.session.state.code, "traits code");
  assert.equal(page.session.state.mark, "traits");
  assert.equal(page.addresses.at(-1), "/play#traits");
  page.session.choose("server");
  assert.equal(page.addresses.at(-1), "/play?run=server#traits");
  // An edit drops the hash: the code is no longer the example's.
  page.session.setCode("traits code, edited");
  assert.equal(page.addresses.at(-1), "/play?run=server");
  page.session.choose("browser");
  assert.equal(page.addresses.at(-1), "/play");
  page.session.pick("reset");
  assert.equal(page.session.state.code, "word count code", "Reset is the first example");
  await new Promise((done) => setTimeout(done, 10));
});

test("in the browser an edit runs again after a pause; on the server it waits for Run", async () => {
  const ran: string[] = [];
  const page = standIn({ code: "x", web: { run_text: (code: string) => (ran.push(code), JSON.stringify({ kind: "text", status: 0, output: [] })) } });
  page.session.setCode("y");
  page.tick();
  await new Promise((done) => setTimeout(done, 0));
  assert.deepEqual(ran, ["y"]);
  page.session.choose("server");
  await new Promise((done) => setTimeout(done, 0));
  const asked = page.fetched.length;
  page.session.setCode("z");
  page.tick();
  assert.equal(page.fetched.length, asked);
});

test("in the browser, rawr-web loads once the page has loaded, and a restored example runs", async () => {
  const ran: string[] = [];
  const web: Partial<Web> = { run_text: (code: string) => (ran.push(code), JSON.stringify({ kind: "text", status: 0, output: [] })) };
  const page = standIn({ url: "http://localhost/play#word-count", examples: EXAMPLES, web });
  assert.deepEqual(page.loads, [], "nothing loads before the page has");
  page.session.preload();
  await new Promise((done) => setTimeout(done, 10));
  assert.deepEqual(page.loads, [WEB_MODULE]);
  assert.deepEqual(ran, ["word count code"]);
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

test("Turnstile waits for its container instead of throwing before the island renders it", () => {
  // The session starts before React renders #ts; rendering then would throw
  // and blank the playground, as it did live.
  const early = standIn({ code: "x", sitekey: "0xKEY", container: false });
  assert.deepEqual(early.widgets, [], "no render while #ts is missing");
  early.tick();
  assert.deepEqual(early.widgets, [], "still waiting");
});
