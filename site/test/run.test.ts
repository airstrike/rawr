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
