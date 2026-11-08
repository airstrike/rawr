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
