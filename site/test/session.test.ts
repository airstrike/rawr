// node --test test/session.test.ts
// The playground's Session against stand-ins for the browser: where a run
// goes, what it shows, the address and the theme.
import assert from "node:assert/strict";
import { test } from "node:test";
import { Session, WEB_MODULE, type Example, type PlayData, type Universe, type Web } from "../src/play/session.ts";
import type { Frame } from "../src/scripts/universe.ts";

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
  universe?: Universe;
}

/** A Session on stand-ins, with what it did. */
function standIn(options: Options = {}) {
  const { code = "", web = null, url = "http://localhost/play", shared = null, examples = [], sitekey = null, pass = false, draft = null, theme = "dark", container = true, universe } = options;
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
      universe,
      frameHost: () => ({}) as Element,
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

/** A universe stand-in whose frames record their removal, and the runs it was given. */
function universeStandIn(answer: (code: string) => any, manifest = '[dependencies]\niced = "0.14"\nrust-decimal = "1"\n') {
  const frames: { removed: boolean }[] = [];
  const runs: string[] = [];
  const universe: Universe = {
    manifest: async () => manifest,
    frame: async () => {
      const record = { removed: false };
      frames.push(record);
      return { element: { remove: () => void (record.removed = true) }, instance: {} } as unknown as Frame;
    },
    run: async (_frame, code) => (runs.push(code), answer(code)),
    replace: () => ({ kind: "restart", reason: "a stand-in" }),
  };
  return { universe, frames, runs };
}

test("Run in browser runs a snippet that uses a crate the universe declares in a frame, and a plain one in rawr-web", async () => {
  const { universe, frames, runs } = universeStandIn(() => ({ kind: "text", status: 0, output: [{ stream: "stdout", text: "drawn\n" }] }));
  const web = { run_text: () => JSON.stringify({ kind: "text", status: 0, output: [{ stream: "stdout", text: "plain\n" }] }) };
  const page = standIn({ code: "use iced::widget::button;\nfn main() {}", web, universe });
  await page.session.go();
  assert.deepEqual(runs, ["use iced::widget::button;\nfn main() {}"]);
  assert.deepEqual(page.loads, [], "rawr-web is not loaded for it");
  assert.deepEqual(page.session.state.output, { kind: "frame", chunks: [{ stream: "stdout", text: "drawn\n" }], status: 0 });
  assert.match(page.session.state.status, /in the browser$/);
  assert.equal(frames.length, 2, "the next frame is made ahead");
  // A crate's name with `-` is a path with `_`.
  page.session.setCode("let d = rust_decimal::Decimal::ZERO;");
  await page.session.go();
  assert.equal(runs.length, 2);
  assert.equal(frames[0].removed, true, "a new run's frame replaces the last one");
  // A plain program goes to rawr-web, and its output ends the frame.
  page.session.setCode('println!("plain")');
  await page.session.go();
  assert.equal(runs.length, 2);
  assert.deepEqual(page.loads, [WEB_MODULE]);
  assert.equal(frames[1].removed, true);
  assert.equal(page.session.state.output.kind, "term");
});

test("a universe snippet that does not compile shows its report in place of its frame", async () => {
  const { universe, frames } = universeStandIn(() => ({ kind: "error", text: "error[E0425]: cannot find value `x`" }));
  const page = standIn({ code: "iced::run(x)", universe });
  await page.session.go();
  assert.deepEqual(page.session.state.output, { kind: "error", text: "error[E0425]: cannot find value `x`" });
  assert.equal(frames[0].removed, true);
});

test("a universe that does not load runs the snippet on the server", async () => {
  const { universe } = universeStandIn(() => null);
  universe.frame = async () => {
    throw new Error("404");
  };
  const page = standIn({ code: "iced::run(x)", universe });
  await page.session.go();
  assert.deepEqual(page.fetched.map((f) => f.url), ["/play/render"]);
  assert.match(page.session.state.status, /on the server: the universe did not load: 404$/);
  assert.equal(page.session.state.where, "browser", "the next run tries the browser again");
});

test("a universe snippet is one that names a crate of the manifest's dependency tables", async () => {
  const { crates, usesCrate } = await import("../src/scripts/universe.ts");
  const names = crates(`[package]\nname = "play"\n[target.'cfg(target_arch = "wasm32")'.dependencies]\niced = { git = "x" }\n[dependencies]\nserde-json = "1"\n[dev-dependencies]\ntest-only = "1"\n[profile.dev.package.gif]\nopt-level = 3\n`);
  assert.deepEqual(names, ["iced", "serde_json"]);
  assert.ok(usesCrate("use iced::widget;", names));
  assert.ok(usesCrate("fn main() { ::serde_json::to_string(&1); }", names));
  assert.ok(usesCrate("extern crate iced;", names));
  assert.ok(!usesCrate('let iced = 1; println!("{iced}");', names));
  assert.ok(!usesCrate("use my::iced::thing;", names));
  assert.ok(!usesCrate("let x = opt_level;", names));
});

/** A universe stand-in whose frames' programs take edits as `edit` answers them. */
function reloading(edit: (code: string) => any) {
  const stand = universeStandIn(() => ({ kind: "text", status: 0, output: [{ stream: "stdout", text: "started\n" }] }));
  const edits: string[] = [];
  stand.universe.replace = (_frame, code) => (edits.push(code), edit(code));
  return { ...stand, edits };
}

/** An edit to `code`, run once its pause has passed. */
async function edited(page: ReturnType<typeof standIn>, code: string) {
  page.session.setCode(code);
  page.tick();
  await new Promise((done) => setTimeout(done, 0));
}

test("an edit reloads the program its frame runs in place, and Run starts it over", async () => {
  const { universe, frames, runs, edits } = reloading(() => ({ kind: "swapped", output: [{ stream: "stdout", text: "reloaded\n" }] }));
  const page = standIn({ code: 'use iced::widget::button;\nfn main() { button("+"); }', universe });
  await page.session.go();
  await edited(page, 'use iced::widget::button;\nfn main() { button("Add"); }');
  assert.deepEqual(edits, ['use iced::widget::button;\nfn main() { button("Add"); }']);
  assert.equal(runs.length, 1, "the edit runs nothing again");
  assert.equal(frames[0].removed, false, "the program keeps its frame and its state");
  assert.deepEqual(page.session.state.output, {
    kind: "frame",
    chunks: [{ stream: "stdout", text: "started\n" }, { stream: "stdout", text: "reloaded\n" }],
    status: 0,
  });
  assert.match(page.session.state.status, /reloaded in place$/);
  // Run starts the program over in the next frame.
  await page.session.go();
  assert.equal(runs.length, 2);
  assert.equal(frames[0].removed, true);
  assert.equal(edits.length, 1);
});

test("an edit that does not compile shows its error under the frame, and the program runs on", async () => {
  const { universe, frames, runs } = reloading((code) =>
    code.includes("oops") ? { kind: "error", text: "error: expected `;`" } : { kind: "unchanged" },
  );
  const start = "use iced::widget::button;\nfn main() {}";
  const page = standIn({ code: start, universe });
  await page.session.go();
  await edited(page, start + " oops");
  assert.deepEqual(page.session.state.output, {
    kind: "frame",
    chunks: [{ stream: "stdout", text: "started\n" }],
    status: 0,
    error: "error: expected `;`",
  });
  assert.match(page.session.state.status, /^error/);
  assert.equal(frames[0].removed, false);
  // Back to the code it runs: the error goes, and nothing runs again.
  await edited(page, start);
  assert.deepEqual(page.session.state.output, { kind: "frame", chunks: [{ stream: "stdout", text: "started\n" }], status: 0 });
  assert.equal(runs.length, 1);
});

test("an edit that cannot reload in place runs in the next frame, and says why", async () => {
  const { universe, frames, runs } = reloading(() => ({ kind: "restart", reason: "`main` changed" }));
  const page = standIn({ code: "use iced::widget::button;\nfn main() {}", universe });
  await page.session.go();
  await edited(page, "use iced::widget::button;\nfn main() { let _ = 1; }");
  assert.deepEqual(runs, ["use iced::widget::button;\nfn main() {}", "use iced::widget::button;\nfn main() { let _ = 1; }"]);
  assert.equal(frames[0].removed, true, "the fresh frame replaces it");
  assert.match(page.session.state.status, /restarted: `main` changed$/);
});

test("an edit after a compile error, with no program running, runs in a fresh frame", async () => {
  let compiles = false;
  const stand = universeStandIn(() => (compiles ? { kind: "text", status: 0, output: [] } : { kind: "error", text: "error[E0425]" }));
  const edits: string[] = [];
  stand.universe.replace = (_frame, code) => (edits.push(code), { kind: "swapped", output: [] });
  const page = standIn({ code: "iced::run(x)", universe: stand.universe });
  await page.session.go();
  compiles = true;
  await edited(page, "iced::run(y)");
  assert.deepEqual(edits, [], "no program runs to reload");
  assert.equal(stand.runs.length, 2);
  assert.equal(page.session.state.output.kind, "frame");
});

test("why a load failed is its message's first line, without the error's name, cut short", async () => {
  const { why } = await import("../src/play/session.ts");
  assert.equal(why(new Error("LinkError: Tag import is not a Tag\nat line 2")), "Tag import is not a Tag");
  assert.equal(why("plain"), "plain");
  assert.equal(why(new Error("")), "no reason given");
  assert.equal(why(new Error("x".repeat(200))).length, 120);
});
