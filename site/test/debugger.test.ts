// node --test test/debugger.test.ts
// The playground's debugger (src/play/debugger.ts) against a stand-in for
// the debugger build's instance, and the session's debugger icon against
// stand-in universes: the held start, the capabilities said up front, the
// breakpoints a click sets, a pause's frames and bindings, a value the
// bindings panel edits in place, and the icon's gate on the browser's own
// JSPI.
import assert from "node:assert/strict";
import { test } from "node:test";
import { assignment, canPause, Debugger, partPlace, ROOT } from "../src/play/debugger.ts";
import { editable, visible, type Row } from "../src/play/rows.ts";
import { Session, type Example, type State, type Universe } from "../src/play/session.ts";
import type { Frame } from "../src/scripts/universe.ts";

/** A stand-in instance whose debugger answers with `answer`, and the requests it was asked. */
function instance(answer: (request: any) => any) {
  const asked: any[] = [];
  let listener: ((event: string) => void) | null = null;
  return {
    asked,
    /** Tell the page `event`, as the debugger tells one. */
    tell: (event: object) => listener?.(JSON.stringify(event)),
    debuggee: {
      debug: async (request: string) => {
        const parsed = JSON.parse(request);
        asked.push(parsed);
        return JSON.stringify(answer(parsed));
      },
      debug_events: (next: (event: string) => void) => void (listener = next),
    },
  };
}

/** Answers as the debugger gives them, `pause` saying whether this browser can. */
function answers(pause: boolean) {
  const lines = new Set<number>();
  const list = () => [...lines].map((line, id) => ({ id, file: ROOT, line, text: "" }));
  return (request: any) => {
    switch (request.op) {
      case "hello":
        return { debugger: true, pause };
      case "hold":
        return { held: true };
      case "release":
        return { released: true };
      case "break":
        lines.add(request.line);
        return { set: { file: ROOT, line: request.line }, paused: null, breakpoints: list() };
      case "clear":
        lines.delete(request.line);
        return { cleared: 1, left: list() };
      case "continue":
        return { paused: null };
      case "eval":
        return request.code === "seen"
          ? { value: '{"a": 1}', children: [{ name: '"a"', value: "1" }] }
          : { value: "7" };
      default:
        return {
          reached: true,
          frames: [{ function: "inner", line: 2 }, { function: "main", line: 9 }],
          bindings: [{ name: "depth", value: String(3 + (request.frame ?? 0)) }],
          unread: null,
        };
    }
  };
}

const settle = () => new Promise((done) => setTimeout(done, 0));

test("a run is held at its start until the page's breakpoints are set, then released", async () => {
  const debug = new Debugger();
  await debug.toggle(2);
  assert.deepEqual(debug.state.lines, [2], "a click before a run keeps its line");
  const page = instance(answers(true));
  assert.equal(await debug.attach(page.debuggee), true);
  assert.equal(debug.state.pause, true, "hello says a pause can wait");
  assert.equal(debug.state.phase, "held");
  await debug.start();
  assert.deepEqual(page.asked.map((request) => request.op), ["hello", "hold", "break", "release"]);
  assert.deepEqual(page.asked[2], { op: "break", file: ROOT, line: 2 });
  assert.equal(debug.state.phase, "running");
});

test("a browser that can't pause is said so up front, and nothing holds its run", async () => {
  const debug = new Debugger();
  const page = instance(answers(false));
  await debug.attach(page.debuggee);
  assert.equal(debug.state.pause, false);
  await debug.start();
  assert.deepEqual(page.asked.map((request) => request.op), ["hello"]);
});

test("a pause reads its frames and bindings, another frame's on a click, and an eval answers", async () => {
  const debug = new Debugger();
  const page = instance(answers(true));
  await debug.attach(page.debuggee);
  await debug.start();
  page.tell({ event: "paused", file: ROOT, line: 2, function: "inner", reason: "breakpoint" });
  await settle();
  assert.equal(debug.state.phase, "paused");
  assert.deepEqual(debug.state.paused, { file: ROOT, line: 2, function: "inner", reason: "breakpoint" });
  assert.deepEqual(debug.state.frames.map((frame) => frame.function), ["inner", "main"]);
  assert.deepEqual(debug.state.bindings, [{ name: "depth", value: "3" }]);
  await debug.select(1);
  assert.deepEqual(debug.state.bindings, [{ name: "depth", value: "4" }]);
  assert.deepEqual(page.asked.at(-1), { frame: 1, children: true }, "a frame's bindings come with their parts");
  await debug.evaluate("depth + 3");
  assert.deepEqual(page.asked.at(-2), { op: "eval", code: "depth + 3", frame: 1, children: true }, "code runs in the selected frame");
  assert.deepEqual(page.asked.at(-1), { frame: 1, children: true }, "an eval reads the frame's bindings again, since it may write them");
  assert.deepEqual(debug.state.evaluated, [{ code: "depth + 3", value: "7" }]);
  // A value with parts keeps them, for the console's tree.
  await debug.evaluate("seen");
  assert.deepEqual(debug.state.evaluated.at(-1), { code: "seen", value: '{"a": 1}', children: [{ name: '"a"', value: "1" }] });
  await debug.continue();
  page.tell({ event: "resumed" });
  assert.equal(debug.state.phase, "running");
  assert.equal(debug.state.paused, null);
});

test("a click on a line sets a breakpoint on the running program, and a second clears it", async () => {
  const debug = new Debugger();
  const page = instance(answers(true));
  await debug.attach(page.debuggee);
  await debug.start();
  await debug.toggle(3);
  assert.deepEqual(debug.state.lines, [3]);
  await debug.toggle(3);
  assert.deepEqual(debug.state.lines, []);
  assert.deepEqual(page.asked.slice(-2).map((request) => request.op), ["break", "clear"]);
  // A compile that moved a breakpoint says where it is now.
  page.tell({ event: "breakpoints", revision: 4, breakpoints: [{ id: 1, file: ROOT, line: 5, text: "" }] });
  assert.deepEqual(debug.state.lines, [5]);
});

test("a detached instance's events are dropped", async () => {
  const debug = new Debugger();
  const page = instance(answers(true));
  await debug.attach(page.debuggee);
  debug.detach();
  page.tell({ event: "paused", file: ROOT, line: 2, function: "inner", reason: "breakpoint" });
  assert.equal(debug.state.phase, "idle");
});

/** A universe stand-in whose frames carry `debuggee`, and the runs it was given. */
function universe(name: string, debuggee: object = {}) {
  const runs: { code: string; held: boolean }[] = [];
  const removed: string[] = [];
  const stand: Universe = {
    manifest: async () => '[dependencies]\niced = "0.14"\n',
    frame: async () => ({ element: { remove: () => void removed.push(name) }, instance: debuggee }) as unknown as Frame,
    run: async (_frame, code, debugged) => {
      runs.push({ code, held: debugged !== undefined });
      await debugged?.started();
      debugged?.ended(3);
      return { kind: "text", status: 3, output: [] };
    },
    replace: () => ({ kind: "restart", reason: "a stand-in" }),
  };
  return { stand, runs, removed };
}

/** A session whose debugger icon has stand-in universes, in a browser that can pause or can't. */
function debuggable(pausable: boolean, examples: Example[] = []) {
  const plain = universe("plain");
  const page = instance(answers(true));
  const debugged = universe("debugger", page.debuggee);
  const debug = new Debugger();
  const web = { run_text: () => JSON.stringify({ kind: "text", status: 0, output: [{ stream: "stdout", text: "plain\n" }] }) };
  const timers: (() => void)[] = [];
  const session = new Session(
    { snippet: { code: 'fn main() { println!("hi"); }' }, examples, shared: null },
    {
      fetch: (async () => Response.json({})) as typeof fetch,
      location: { pathname: "/play", search: "", hash: "", origin: "http://localhost" },
      replaceState: () => {},
      storage: null,
      loadWeb: async () => web,
      now: () => 0,
      clipboard: async () => {},
      hasContainer: () => true,
      setTimeout: (f) => (timers.push(f), timers.length),
      clearTimeout: () => {},
      universe: plain.stand,
      frameHost: () => ({}) as Element,
      debugging: { universe: debugged.stand, debugger: debug, pausable },
    },
    "dark",
  );
  return { session, plain, debugged, page, debug };
}

test("the debugger icon runs the program again on the debugger build, held at its start, and back", async () => {
  const { session, plain, debugged, page, debug } = debuggable(true);
  await session.go();
  assert.equal(session.state.output.kind, "term", "a plain program runs in rawr-web");
  await session.debug();
  assert.equal(session.state.debugging, true);
  assert.deepEqual(debugged.runs, [{ code: 'fn main() { println!("hi"); }', held: true }], "it runs on the debugger build, held");
  assert.deepEqual(plain.runs, []);
  assert.equal(session.state.output.kind, "frame");
  assert.deepEqual(page.asked.map((request) => request.op), ["hello", "hold", "release"]);
  assert.equal(debug.state.phase, "ended", "the panel shows the end of main");
  assert.equal(debug.state.status, 3, "with its exit status");
  await session.debug();
  assert.equal(session.state.debugging, false);
  assert.ok(debugged.removed.includes("debugger"), "the debugger's frame goes");
  assert.equal((session.state as State).output.kind, "term", "and the program runs as before");
  assert.equal(debug.state.phase, "idle");
});

test("picking a different example clears the breakpoints, and the next run sets none", async () => {
  const examples: Example[] = [
    { name: "One", code: "fn main() {}", slug: "one" },
    { name: "Two", code: 'fn main() { println!("two"); }', slug: "two" },
  ];
  const { session, page, debug } = debuggable(true, examples);
  await session.debug();
  await debug.toggle(1);
  assert.deepEqual(debug.state.lines, [1]);
  session.pick("one");
  assert.deepEqual(debug.state.lines, [], "a different snippet's lines are not the old code's");
  await settle();
  await debug.toggle(1);
  // Picking the snippet the editor already holds keeps them.
  session.pick("one");
  assert.deepEqual(debug.state.lines, [1]);
  await settle();
  const asked = page.asked.length;
  session.pick("two");
  await settle();
  assert.deepEqual(debug.state.lines, []);
  assert.ok(!page.asked.slice(asked).some((request) => request.op === "break"), "the run after the pick sets no breakpoint");
});

test("a browser can pause only with its own WebAssembly.Suspending", () => {
  // A browser's own function prints as native code; the frame's stand-in prints its source.
  assert.equal(canPause({ Suspending: Math.max }), true);
  assert.equal(canPause({ Suspending: function () {} }), false, "a stand-in, as a page without JSPI has in its frame");
  assert.equal(canPause({}), false, "no JSPI");
  assert.equal(canPause(null), false, "no WebAssembly");
});

test("a browser that can't pause never turns the debugger on, nor runs its build", async () => {
  const { session, debugged, page } = debuggable(false);
  await session.go();
  await session.debug();
  assert.equal(session.state.debugging, false);
  assert.deepEqual(debugged.runs, [], "the debugger build never runs");
  assert.deepEqual(page.asked, []);
  assert.equal(session.state.output.kind, "term", "the program's output stays");
});

test("a part's place is named as Rust reaches it, as an editor's setVariable names one", () => {
  assert.equal(partPlace("self", "Counter { value: 1 }", "value"), "self.value");
  assert.equal(partPlace("self.last", "(1, 2)", "0"), "self.last.0");
  assert.equal(partPlace("items", "[1, 2]", "[1]"), "items[1]");
  assert.equal(partPlace("names", '{"a": 1}', '"a"'), '(*names.get_mut("a").unwrap())');
  assert.equal(partPlace("ids", "{3: 1}", "3"), "(*ids.get_mut(&3).unwrap())");
  assert.equal(assignment("self.value", "100"), "self.value = 100; self.value");
});

test("an edit in place assigns through eval in the selected frame and reads the frame again", async () => {
  let value = 1;
  const refusal = "error[E0308]: mismatched types\n --> <eval>:1:14";
  const page = instance((request: any) => {
    if (request.op === "eval") {
      if (request.code.includes('"a"')) return { error: refusal };
      value = Number(request.code.match(/= (\d+);/)?.[1]);
      return { value: String(value) };
    }
    if (request.op !== undefined) return answers(true)(request);
    return {
      reached: true,
      frames: [{ function: "update", line: 31 }, { function: "main", line: 10 }],
      bindings: [{ name: "self", value: `Counter { value: ${value} }`, children: [{ name: "value", value: String(value) }] }],
    };
  });
  const debug = new Debugger();
  await debug.attach(page.debuggee);
  await debug.start();
  page.tell({ event: "paused", file: ROOT, line: 31, function: "update", reason: "breakpoint" });
  await settle();
  const place = partPlace("self", debug.state.bindings[0].value!, "value");
  assert.deepEqual(await debug.assign(place, "100"), { value: "100" });
  const asked = page.asked.slice(-2);
  assert.deepEqual(asked[0], { op: "eval", code: "self.value = 100; self.value", frame: 0 });
  assert.deepEqual(asked[1], { frame: 0, children: true }, "a write reads the frame's bindings again");
  assert.equal(debug.state.bindings[0].children?.[0].value, "100");
  // A write Rust refuses gives rustc's error, and reads nothing again.
  const before = page.asked.length;
  assert.deepEqual(await debug.assign(place, '"a"'), { error: refusal });
  assert.equal(page.asked.length, before + 1);
  assert.equal(debug.state.evaluated.length, 0, "an edit isn't one of the console's evals");
});

test("an edit while the program runs assigns nothing", async () => {
  const debug = new Debugger();
  const page = instance(answers(true));
  await debug.attach(page.debuggee);
  await debug.start();
  const before = page.asked.length;
  assert.ok("error" in (await debug.assign("x", "1")));
  assert.equal(page.asked.length, before);
});

test("the bindings panel's rows carry their places, and a value without parts edits in place", () => {
  const rows: Row[] = [
    {
      path: "self", depth: 0, name: "self", value: "Counter { value: 1, items: [2] }", place: "self", children: [
        { name: "value", value: "1" },
        { name: "items", value: "[2]", children: [{ name: "[0]", value: "2" }] },
      ],
    },
    { path: "message", depth: 0, name: "message", error: "it isn't read here", place: "message" },
  ];
  const shown = visible(rows, new Set(["self", "self/items"]));
  assert.deepEqual(shown.map((row) => [row.path, row.place]), [
    ["self", "self"],
    ["self/value", "self.value"],
    ["self/items", "self.items"],
    ["self/items/[0]", "self.items[0]"],
    ["message", "message"],
  ]);
  // A value with parts opens instead, and one that wasn't read has no text to edit.
  assert.deepEqual(shown.map(editable), [false, true, false, true, false]);
  // The console's trees have no place: nothing in them is assigned to.
  assert.equal(editable({ path: "a/x", depth: 1, name: "x", value: "1" }), false);
});
