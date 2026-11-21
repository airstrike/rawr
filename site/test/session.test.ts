// node --test test/session.test.ts
// The playground's Session against stand-ins for the browser: where a run
// goes in the browser, what it shows, the address and the theme. No run
// ever reaches a server.
import assert from "node:assert/strict";
import { test } from "node:test";
import { onlyHere, Session, WEB_MODULE, type Example, type PlayData, type Universe, type Web } from "../src/play/session.ts";
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
  draft?: string | null;
  theme?: "dark" | "light";
  universe?: Universe;
}

/** A Session on stand-ins, with what it did. */
function standIn(options: Options = {}) {
  const { code = "", web = null, url = "http://localhost/play", shared = null, examples = [], draft = null, theme = "dark", universe } = options;
  const data: PlayData = { snippet: { code }, examples, shared };
  const parsed = new URL(url);
  const loads: string[] = [];
  const fetched: { url: string; body: any }[] = [];
  const addresses: string[] = [];
  const stored = new Map<string, string>(draft ? [["rawr-play-draft", draft]] : []);
  const timers: (() => void)[] = [];
  let respond: (url: string) => Response = () => Response.json({ id: "abcDEF1234" });
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
      setTimeout: (f) => (timers.push(f), timers.length),
      clearTimeout: () => {},
      universe,
      frameHost: () => ({}) as Element,
    },
    theme,
  );
  return {
    session, loads, fetched, addresses, stored,
    respond: (next: (url: string) => Response) => void (respond = next),
    /** Runs the timers set so far, as their time passing would. */
    tick: () => timers.splice(0).forEach((f) => f()),
  };
}

test("a plain program runs in rawr-web and shows its output as the bot's", async () => {
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

test("a module that does not load says why and runs nothing anywhere else", async () => {
  const failed = standIn({ code: "1 + 2", web: null });
  await failed.session.go();
  assert.deepEqual(failed.fetched, [], "nothing goes to a server");
  assert.deepEqual(failed.session.state.output, { kind: "error", text: "rawr did not load in this browser: 404" });
  assert.equal(failed.session.state.status, "did not run");
  assert.deepEqual(failed.addresses, []);
});

test("a trap in rawr-web says why, and the next run loads a fresh module", async () => {
  let traps = 1;
  const web: Partial<Web> = {
    run_text: () => {
      if (traps-- > 0) throw new Error("RuntimeError: unreachable");
      return JSON.stringify({ kind: "text", status: 0, output: [{ stream: "stdout", text: "in the browser\n" }] });
    },
  };
  const page = standIn({ code: "1 + 2", web });
  await page.session.go();
  assert.deepEqual(page.fetched, [], "nothing goes to a server");
  assert.deepEqual(page.session.state.output, { kind: "error", text: "rawr stopped in the browser: unreachable. The next run loads it again." });
  await page.session.go();
  assert.equal(page.loads.length, 2, "the next run loads a fresh module");
  assert.match(page.session.state.status, /in the browser$/);
});

test("the address restores an example from its hash, and drops an old run mode", () => {
  const page = standIn({ url: "http://localhost/play?run=server#traits", examples: EXAMPLES });
  assert.equal(page.session.state.code, "traits code");
  assert.deepEqual(page.addresses, ["/play#traits"], "the page runs in the browser only");
  const browser = standIn({ url: "http://localhost/play?run=browser#word-count", examples: EXAMPLES });
  assert.equal(browser.session.state.code, "word count code");
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
  const page = standIn({ code: "shared code", url: "http://localhost/play/abcDEF1234#traits", shared: "abcDEF1234", examples: EXAMPLES });
  assert.equal(page.session.state.code, "shared code");
  assert.deepEqual(page.addresses, ["/play/abcDEF1234"]);
});

test("a share after an edit replaces the example's anchor with the share's address", async () => {
  const page = standIn({ url: "http://localhost/play#traits", examples: EXAMPLES });
  page.session.setCode("traits code, edited");
  assert.equal(page.addresses.at(-1), "/play#traits");
  await page.session.share();
  assert.equal(page.addresses.at(-1), "/play/abcDEF1234");
  assert.equal(page.session.state.mark, "");
  assert.deepEqual(page.fetched[0].body, { code: "traits code, edited" });
});

test("a share's link drops the example's hash", async () => {
  const page = standIn({ code: "x", url: "http://localhost/play#traits", examples: EXAMPLES });
  const copied: string[] = [];
  page.session.deps.clipboard = async (text) => void copied.push(text);
  page.respond(() => Response.json({ id: "abcDEF1234" }));
  await page.session.share();
  assert.deepEqual(page.fetched.map((f) => f.url), ["/play/share"]);
  assert.equal(page.addresses.at(-1), "/play/abcDEF1234");
  assert.deepEqual(copied, ["http://localhost/play/abcDEF1234"]);
  assert.equal(page.session.state.status, "link copied · abcDEF1234");
});

test("choosing an example writes its anchor, and an edit keeps it", async () => {
  const web: Partial<Web> = { run_text: () => JSON.stringify({ kind: "text", status: 0, output: [] }) };
  const page = standIn({ code: "x", url: "http://localhost/play/abcDEF1234", shared: "abcDEF1234", examples: EXAMPLES, web });
  page.session.pick("traits");
  assert.equal(page.session.state.code, "traits code");
  assert.equal(page.session.state.mark, "traits");
  assert.equal(page.addresses.at(-1), "/play#traits");
  // An edit keeps the anchor, which names where the code came from.
  const written = page.addresses.length;
  page.session.setCode("traits code, edited");
  assert.equal(page.addresses.length, written, "an edit writes no address");
  assert.equal(page.session.state.mark, "traits");
  // The next pick replaces it.
  page.session.pick("word-count");
  assert.equal(page.addresses.at(-1), "/play#word-count");
  page.session.pick("reset");
  assert.equal(page.session.state.code, "word count code", "Reset is the first example");
  await new Promise((done) => setTimeout(done, 10));
});

test("an edit runs again after a pause", async () => {
  const ran: string[] = [];
  const page = standIn({ code: "x", web: { run_text: (code: string) => (ran.push(code), JSON.stringify({ kind: "text", status: 0, output: [] })) } });
  page.session.setCode("y");
  page.tick();
  await new Promise((done) => setTimeout(done, 0));
  assert.deepEqual(ran, ["y"]);
  assert.deepEqual(page.fetched, []);
});

test("rawr-web loads once the page has loaded, and a restored example runs", async () => {
  const ran: string[] = [];
  const web: Partial<Web> = { run_text: (code: string) => (ran.push(code), JSON.stringify({ kind: "text", status: 0, output: [] })) };
  const page = standIn({ url: "http://localhost/play#word-count", examples: EXAMPLES, web });
  assert.deepEqual(page.loads, [], "nothing loads before the page has");
  page.session.preload();
  await new Promise((done) => setTimeout(done, 10));
  assert.deepEqual(page.loads, [WEB_MODULE]);
  assert.deepEqual(ran, ["word count code"]);
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

test("a snippet that uses a crate the prebuilt runner declares runs in a frame, and a plain one in rawr-web", async () => {
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

test("a report from the browser's prebuilt runner names the snippet play.rs", async () => {
  const { universe } = universeStandIn(() => ({ kind: "error", text: "error[E0425]: cannot find value `x`\n --> /universe/src/main.rs:1:11" }));
  const page = standIn({ code: "iced::run(x)", universe });
  await page.session.go();
  assert.deepEqual(page.session.state.output, { kind: "error", text: "error[E0425]: cannot find value `x`\n --> play.rs:1:11" });
});

test("a prebuilt runner that does not load says why and runs nothing anywhere else", async () => {
  const { universe } = universeStandIn(() => null);
  universe.frame = async () => {
    throw new Error("404");
  };
  const page = standIn({ code: "iced::run(x)", universe });
  await page.session.go();
  assert.deepEqual(page.fetched, [], "nothing goes to a server");
  assert.deepEqual(page.session.state.output, { kind: "error", text: "The browser's prebuilt runner did not load: 404" });
  assert.equal(page.session.state.status, "did not run");
});

/** The browser's manifest, as universe.toml has it: a native table and a browser's. */
const MANIFEST = `[package]
name = "play"
[target.'cfg(not(target_arch = "wasm32"))'.dependencies]
iced = { git = "https://github.com/iced-rs/iced", rev = "be88873", features = ["canvas"] }
iced_test = { git = "https://github.com/iced-rs/iced", rev = "be88873" }
[target.'cfg(target_arch = "wasm32")'.dependencies]
iced = { git = "https://github.com/iced-rs/iced", rev = "be88873", default-features = false }
tokio = { version = "1", features = ["macros", "rt"] }
ratatui = { version = "0.30", default-features = false, features = ["std"] }
`;

test("a frontmatter whose dependencies the browser has runs with its block blanked", async () => {
  const { universe, runs } = universeStandIn(() => ({ kind: "text", status: 0, output: [] }), MANIFEST);
  const ran: string[] = [];
  const web = { run_text: (code: string) => (ran.push(code), JSON.stringify({ kind: "text", status: 0, output: [] })) };
  const code = '---\n[dependencies]\ntokio = { version = "1.40", features = ["rt"] }\n---\nuse tokio::runtime::Builder;\nfn main() {}\n';
  const page = standIn({ code, web, universe });
  await page.session.go();
  const blanked = code.replace(/^[\s\S]*?\n---\n/, (block) => block.replace(/[^\n]/g, " "));
  assert.deepEqual(runs, [blanked], "lines and columns stay the snippet's");
  assert.ok(blanked.startsWith("   \n") && blanked.endsWith("\nuse tokio::runtime::Builder;\nfn main() {}\n"));
  // A frontmatter that names nothing runs in rawr-web, blanked too.
  page.session.setCode('---\n[package]\nedition = "2021"\n---\nprintln!("hi")');
  await page.session.go();
  assert.equal(ran.length, 1);
  assert.ok(ran[0].endsWith('\nprintln!("hi")') && !ran[0].includes("---"), ran[0]);
  assert.deepEqual(page.fetched, []);
});

test("a frontmatter dependency the browser lacks gets one line, and nothing runs", async () => {
  const { universe, runs } = universeStandIn(() => ({ kind: "text", status: 0, output: [] }), MANIFEST);
  const page = standIn({ code: '---\n[dependencies]\nlibc = "0.2.172"\n---\nfn main() {}\n', universe, web: { run_text: () => "{}" } });
  await page.session.go();
  const line = "The browser runs only snippets whose dependencies are among iced, ratatui and tokio. The ?play bot on Discord builds more crates.";
  assert.deepEqual(page.session.state.output, { kind: "error", text: line });
  assert.deepEqual([runs, page.loads, page.fetched], [[], [], []]);
  // A crate it has at another version, or from another source, is not the browser's either.
  for (const dependency of ['tokio = "0.2"', 'iced = "0.13"', 'iced = { git = "https://github.com/iced-rs/iced", branch = "master" }', 'ratatui = { path = "../ratatui" }', 'iced_test = { git = "https://github.com/iced-rs/iced", rev = "be88873" }']) {
    page.session.setCode(`---\n[dependencies]\n${dependency}\n---\nfn main() {}\n`);
    await page.session.go();
    assert.deepEqual(page.session.state.output, { kind: "error", text: line }, dependency);
  }
  assert.deepEqual(runs, []);
  assert.equal(onlyHere([]), "This browser runs no snippet with dependencies. The ?play bot on Discord builds more crates.");
});

test("a frontmatter block is read as cargo reads it: its tables, inline or not, and its fences", async () => {
  const { frontmatter, satisfied, offered } = await import("../src/play/frontmatter.ts");
  const block = frontmatter('#!/usr/bin/env rawr\n\n---cargo\n[dependencies]\nc = { package = "libc", version = "0.2" } # libc\n[dependencies.tokio]\nversion = "1"\nfeatures = ["rt"]\n[dev-dependencies]\nratatui = "0.30.1"\n---\nfn main() {}');
  assert.ok(block);
  assert.deepEqual(
    block.dependencies.map((d) => [d.package, d.version, d.table]),
    [["libc", "0.2", "dependencies"], ["tokio", "1", "dependencies"], ["ratatui", "0.30.1", "dev-dependencies"]],
  );
  assert.ok(block.blanked.endsWith("\nfn main() {}") && block.blanked.split("\n").length === 12);
  // No block, a block another tool reads, and an unclosed one are no frontmatter.
  for (const code of ["fn main() {}", "1 - 2", "---toml\na = 1\n---\n", "---\n[dependencies]\n", "--\nx\n--\n"]) assert.equal(frontmatter(code), null, code);
  // A closing fence with more dashes doesn't close the block.
  assert.equal(frontmatter("---\na = 1\n----\n"), null);
  const has = offered(MANIFEST);
  assert.deepEqual(has.map((d) => d.package), ["iced", "tokio", "ratatui"]);
  const wanted = (version: string) => ({ package: "ratatui", version, other: false, table: "dependencies" });
  assert.ok(satisfied(wanted("0.30.2"), has) && satisfied(wanted("^0.30"), has) && satisfied(wanted("*"), has));
  assert.ok(!satisfied(wanted("0.29"), has) && !satisfied(wanted(">=0.30, <0.31"), has));
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
