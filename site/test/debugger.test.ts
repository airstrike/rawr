// node --test test/debugger.test.ts
// The playground's debugger (src/play/debugger.ts) against a stand-in for
// the debugger build's instance, and the session's debugger icon against
// stand-in universes: the held start, the capabilities said up front, the
// breakpoints a click sets, and a pause's frames and bindings.
import assert from "node:assert/strict";
import { test } from "node:test";
import { Debugger, ROOT } from "../src/play/debugger.ts";
import { Session, type State, type Universe } from "../src/play/session.ts";
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
  assert.deepEqual(page.asked.at(-1), { op: "eval", code: "depth + 3", children: true });
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

test("the debugger icon runs the program again on the debugger build, held at its start, and back", async () => {
  const plain = universe("plain");
  const page = instance(answers(true));
  const debugged = universe("debugger", page.debuggee);
  const debug = new Debugger();
  const web = { run_text: () => JSON.stringify({ kind: "text", status: 0, output: [{ stream: "stdout", text: "plain\n" }] }) };
  const timers: (() => void)[] = [];
  const session = new Session(
    { snippet: { code: 'fn main() { println!("hi"); }' }, examples: [], sitekey: null, pass: false, shared: null },
    {
      fetch: (async () => Response.json({})) as typeof fetch,
      location: { pathname: "/play", search: "", hash: "", origin: "http://localhost" },
      replaceState: () => {},
      storage: null,
      loadWeb: async () => web,
      now: () => 0,
      clipboard: async () => {},
      turnstile: () => undefined,
      loadTurnstile: () => {},
      hasContainer: () => true,
      setTimeout: (f) => (timers.push(f), timers.length),
      clearTimeout: () => {},
      universe: plain.stand,
      frameHost: () => ({}) as Element,
      debugging: { universe: debugged.stand, debugger: debug },
    },
    "dark",
  );
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
