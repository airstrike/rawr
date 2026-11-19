// node --test test/run.test.ts
// The site's Run buttons: a run that warming started ahead of its Run.
import assert from "node:assert/strict";
import { test } from "node:test";
import { ahead } from "../src/scripts/run.ts";

test("the first Run takes the warmed answer, and a later Run runs again", async () => {
  let runs = 0;
  const served = ahead(async () => ({ kind: "text", run: ++runs }));
  await served.warm();
  await served.warm();
  assert.equal(runs, 1, "warming twice fetches once");
  assert.deepEqual(await served.take(), { kind: "text", run: 1 });
  assert.deepEqual(await served.take(), { kind: "text", run: 2 });
});

test("a Run before warming finishes waits for it; one with no warming runs", async () => {
  let runs = 0;
  let finish!: () => void;
  const served = ahead(() => new Promise((done) => (finish = () => done({ run: ++runs }))));
  served.warm();
  const taken = served.take();
  finish();
  assert.deepEqual(await taken, { run: 1 });
  const cold = ahead(async () => ({ run: "cold" }));
  assert.deepEqual(await cold.take(), { run: "cold" });
});

test("a warmed run that gave nothing, failed or threw is run again at Run", async () => {
  const given: any[] = [null, { kind: "text" }];
  const nothing = ahead(async () => given.shift());
  await nothing.warm();
  assert.deepEqual(await nothing.take(), { kind: "text" }, "rawr-web that did not load gives nothing");
  const answers: any[] = [{ failed: "too many runs; try again in 3 s" }, { kind: "text" }];
  const failed = ahead(async () => answers.shift());
  await failed.warm();
  assert.deepEqual(await failed.take(), { kind: "text" });
  let calls = 0;
  const thrown = ahead(async () => {
    if (++calls === 1) throw new TypeError("network");
    return { kind: "text" };
  });
  await thrown.warm().catch(() => {});
  assert.deepEqual(await thrown.take(), { kind: "text" });
});

test("a universe frame imports the glue, hides WebKit's WebGPU and takes the module from the page", async () => {
  const { frameDocument } = await import("../src/scripts/universe.ts");
  const html = frameDocument("https://example.org/web/universe.js");
  assert.match(html, /import \* as glue from "https:\/\/example\.org\/web\/universe\.js";/);
  assert.match(html, /navigator\.vendor === "Apple Computer, Inc\."[\s\S]*defineProperty\(navigator, "gpu"/);
  assert.match(html, /glue\.default\(\{ module_or_path: event\.data \}\)/, "a module this large instantiates asynchronously");
  assert.match(html, /<div id="iced"><\/div>/, "iced's web shell puts its canvas in place of #iced");
});

test("a key the focused program reads doesn't scroll the frame or the page", async () => {
  const { frameDocument } = await import("../src/scripts/universe.ts");
  const script = frameDocument("https://example.org/web/universe.js").match(/const scrolls[\s\S]*?\n\}\);/)![0];
  let listener: (event: { key: string; preventDefault(): void }) => void = () => {};
  const document = { body: {}, activeElement: {} as object };
  new Function("addEventListener", "document", script)((_: string, f: typeof listener) => (listener = f), document);
  const pressed = (key: string) => {
    let prevented = false;
    listener({ key, preventDefault: () => (prevented = true) });
    return prevented;
  };
  assert.ok(pressed("ArrowDown") && pressed(" ") && pressed("PageDown"), "the focused program's keys don't scroll");
  assert.ok(!pressed("Tab") && !pressed("a"), "Tab still moves focus, and other keys keep their defaults");
  document.activeElement = document.body;
  assert.ok(!pressed("ArrowDown"), "with the program unfocused, the arrows scroll as usual");
});

test("a universe program starts after its shown frame's next animation frame, which follows the frame's resize", async () => {
  const { universe } = await import("../src/scripts/universe.ts");
  const events: string[] = [];
  let frameCallback: (() => void) | undefined;
  const contentWindow = {
    get innerWidth() {
      events.push("laid out");
      return 700;
    },
    requestAnimationFrame: (callback: () => void) => (frameCallback = callback),
  };
  const page = universe({
    document: {} as Document,
    fetch: (async () => new Response("[dependencies]\n")) as typeof fetch,
    compile: async () => ({}) as WebAssembly.Module,
  });
  const frame = {
    element: { hidden: true, contentWindow } as unknown as HTMLIFrameElement,
    instance: { run: async () => (events.push("ran"), '{"kind":"text","status":0}'), sleeping: () => false } as any,
  };
  const ran = page.run(frame, "fn main() {}");
  await new Promise((next) => setTimeout(next, 10));
  assert.equal(frame.element.hidden, false);
  assert.deepEqual(events, ["laid out"], "the program waits for the frame's rendering update");
  frameCallback!();
  assert.deepEqual(await ran, { kind: "text", status: 0 });
  assert.deepEqual(events, ["laid out", "ran"]);
});

test("a universe program whose main sleeps runs on, printing to a terminal in its frame", async () => {
  const { universe } = await import("../src/scripts/universe.ts");
  const drawn: (() => void)[] = [];
  const appended: { className: string; textContent: string }[] = [];
  const document = {
    createElement: () => ({ className: "", textContent: "" }),
    body: { append: (node: { className: string; textContent: string }) => appended.push(node) },
    scrollingElement: null,
  };
  const contentWindow = {
    innerWidth: 700,
    document,
    requestAnimationFrame: (callback: () => void) => drawn.push(callback),
  };
  const page = universe({
    document: {} as Document,
    fetch: (async () => new Response("[dependencies]\n")) as typeof fetch,
    compile: async () => ({}) as WebAssembly.Module,
  });
  let sink: (stream: string, text: string) => void = () => {};
  const frame = {
    element: { hidden: true, contentWindow } as unknown as HTMLIFrameElement,
    instance: {
      // main has slept: its answer waits for it to return.
      run: () => new Promise<string>(() => {}),
      sleeping: () => true,
      set_output: (write: typeof sink) => {
        sink = write;
        write("stdout", "\x1b[H\x1b[2Jframe 1\n");
      },
    } as any,
  };
  const ran = page.run(frame, "fn main() {}");
  await new Promise((next) => setTimeout(next, 10));
  drawn.shift()!();
  assert.deepEqual(await ran, { kind: "text", status: 0, output: [] });
  assert.equal(appended[0].className, "terminal");
  drawn.shift()!();
  assert.equal(appended[0].textContent, "frame 1", "what it printed before shows first");
  sink("stdout", "\x1b[H\x1b[2Jframe 2\n");
  sink("stdout", "\x1b[H\x1b[2Jframe 3\n");
  assert.equal(drawn.length, 1, "writes between animation frames draw once");
  drawn.shift()!();
  assert.equal(appended[0].textContent, "frame 3", "a frame redraws in place");
});

test("without JSPI the frame stands in for it: calls run to their end, and a sleep refuses with NO_JSPI", async () => {
  const { frameDocument, NO_JSPI } = await import("../src/scripts/universe.ts");
  const script = frameDocument("https://example.org/web/universe.js").match(/if \(typeof WebAssembly\.Suspending[\s\S]*?\n\}\n/)![0];
  const WebAssembly: any = {};
  new Function("WebAssembly", script)(WebAssembly);
  const bridge = new WebAssembly.Suspending((promise: Promise<unknown>) => promise);
  assert.throws(() => bridge(Promise.resolve()), (thrown: unknown) => thrown === NO_JSPI);
  assert.equal(await WebAssembly.promising((a: number, b: number) => a + b)(2, 3), 5);
  await assert.rejects(WebAssembly.promising(() => { throw NO_JSPI; })(), (thrown: unknown) => thrown === NO_JSPI);
});

test("a program stopped for want of JSPI says so in its frame", async () => {
  const { universe, NO_JSPI } = await import("../src/scripts/universe.ts");
  const appended: { className: string; textContent: string }[] = [];
  const contentWindow = {
    innerWidth: 700,
    document: {
      createElement: () => ({ className: "", textContent: "" }),
      body: { append: (node: { className: string; textContent: string }) => appended.push(node) },
    },
    requestAnimationFrame: (callback: () => void) => callback(),
  };
  const page = universe({
    document: {} as Document,
    fetch: (async () => new Response("[dependencies]\n")) as typeof fetch,
    compile: async () => ({}) as WebAssembly.Module,
  });
  const frame = {
    element: { hidden: true, contentWindow } as unknown as HTMLIFrameElement,
    instance: { run: async () => { throw NO_JSPI; }, sleeping: () => false, panic_message: () => NO_JSPI } as any,
  };
  assert.deepEqual(await page.run(frame, "fn main() {}"), { kind: "text", status: 101, output: [] });
  assert.equal(appended[0].className, "terminal");
  assert.equal(appended[0].textContent, NO_JSPI);
});

/** A frame whose instance answers `run` with `answer`, and says `sleeping` is false. */
function heldFrame(answer: () => Promise<string>) {
  const appended: unknown[] = [];
  const document = {
    createElement: () => ({ className: "", textContent: "" }),
    body: { append: (node: unknown) => appended.push(node) },
    scrollingElement: null,
  };
  const contentWindow = { innerWidth: 700, document, requestAnimationFrame: (callback: () => void) => callback() };
  // A hold in the program's top-level items comes before `main` runs.
  const instance = { run: answer, sleeping: () => false, set_output: () => {} };
  return { frame: { element: { hidden: true, contentWindow } as unknown as HTMLIFrameElement, instance: instance as any }, appended };
}

/** A universe over stand-ins, for the frames `heldFrame` makes. */
async function heldUniverse() {
  const { universe } = await import("../src/scripts/universe.ts");
  return universe({ document: {} as Document, fetch: (async () => new Response("[dependencies]\n")) as typeof fetch, compile: async () => ({}) as WebAssembly.Module });
}

test("a debugged run that ends at its release shows its answer, and tells its end", async () => {
  const page = await heldUniverse();
  let release: (text: string) => void = () => {};
  const { frame, appended } = heldFrame(() => new Promise<string>((resolve) => (release = resolve)));
  const ended: number[] = [];
  const answer = await page.run(frame, "fn main() {}", {
    started: async () => release('{"kind":"text","status":2,"output":[{"stream":"stdout","text":"hi\\n"}]}'),
    ended: (status) => void ended.push(status),
  });
  assert.deepEqual(answer, { kind: "text", status: 2, output: [{ stream: "stdout", text: "hi\n" }] });
  assert.deepEqual(ended, [2]);
  assert.equal(appended.length, 0, "no terminal for a program that ended");
});

test("a debugged run held before main that runs on prints to its terminal, and tells its end later", async () => {
  const page = await heldUniverse();
  let finish: (text: string) => void = () => {};
  const { frame, appended } = heldFrame(() => new Promise<string>((resolve) => (finish = resolve)));
  const ended: number[] = [];
  const answer = await page.run(frame, "fn main() {}", { started: async () => {}, ended: (status) => void ended.push(status) });
  assert.deepEqual(answer, { kind: "text", status: 0, output: [] }, "though sleeping() is false, it runs on");
  assert.equal(appended.length, 1, "its output goes to a terminal in its frame");
  assert.deepEqual(ended, []);
  finish('{"kind":"text","status":0,"output":[]}');
  await new Promise((next) => setTimeout(next, 0));
  assert.deepEqual(ended, [0], "its end is told once main returns");
});

test("a debugged run that panics tells its end with status 101", async () => {
  const page = await heldUniverse();
  let fail: (error: Error) => void = () => {};
  const { frame } = heldFrame(() => new Promise<string>((_, reject) => (fail = reject)));
  const ended: number[] = [];
  await page.run(frame, "fn main() {}", { started: async () => {}, ended: (status) => void ended.push(status) });
  fail(new Error("unreachable"));
  await new Promise((next) => setTimeout(next, 0));
  assert.deepEqual(ended, [101]);
});
