import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { stripTypeScriptTypes } from "node:module";
import { runInNewContext } from "node:vm";
import { test } from "node:test";

// Execute the actual Worker lifecycle methods against a controllable container.
const source = readFileSync(new URL("../src/index.ts", import.meta.url), "utf8");
const starts = source.slice(source.indexOf("  // The SDK calls onStart"), source.indexOf("  // The bot gets no inbound requests"));
const report = source.slice(source.indexOf("  async reportedImage()"), source.indexOf("  /** /bot/health:"));

function fixture() {
  let now = 100;
  let writes = 0;
  let fail = false;
  let release: (() => void) | undefined;
  let pending: Promise<void> | undefined;
  const values = new Map<string, unknown>();
  const ctx = {
    container: { running: false },
    storage: {
      async put(record: Record<string, unknown>) {
        writes++;
        await pending;
        for (const [key, value] of Object.entries(record)) values.set(key, value);
      },
      async get(key: string) { return values.get(key); },
    },
  };
  class Container {
    ctx = ctx;
    configuration = "first";
    settings() { return this.configuration; }
    async start() {
      if (fail) { fail = false; throw new Error("start failed"); }
      ctx.container.running = true;
    }
    async startAndWaitForPorts() {
      ctx.container.running = true;
    }
  }
  const Bot = runInNewContext(stripTypeScriptTypes(`class Bot extends Container { ${starts} ${report} }; Bot`), {
    Container,
    Date: { now: () => now * 1000 },
    STARTED_KEY: "settings",
    STARTED_AT_KEY: "started",
    STATS_KEY: "stats",
  });
  const bot = new Bot();
  return {
    bot, ctx, values,
    at(time: number) { now = time; },
    report(boot: number, version = "current") { values.set("stats", { status: { boot, version } }); },
    writes: () => writes,
    fail() { fail = true; },
    hold() { pending = new Promise<void>((done) => { release = done; }); },
    release() { release?.(); pending = undefined; },
  };
}

test("port readiness preserves a running bot's original boot evidence and settings", async () => {
  const f = fixture();
  await f.bot.start();
  f.report(100);
  assert.equal(await f.bot.reportedImage(), "current");
  f.at(120);
  f.bot.configuration = "second";
  await f.bot.startAndWaitForPorts(8080);
  await f.bot.start();
  assert.equal(f.values.get("started"), 100);
  assert.equal(f.values.get("settings"), "first");
  assert.equal(f.writes(), 1);
  assert.equal(await f.bot.reportedImage(), "current");
});

test("a real restart through either entry point rejects the previous boot report", async () => {
  for (const method of ["start", "startAndWaitForPorts"]) {
    const f = fixture();
    await f.bot.start();
    f.report(100, "previous");
    f.ctx.container.running = false;
    f.at(200);
    await f.bot[method]();
    assert.equal(await f.bot.reportedImage(), null);
    assert.equal(f.values.get("started"), 200);
    f.report(201);
    assert.equal(await f.bot.reportedImage(), "current");
  }
});

test("overlapping cold starts record one boundary before the SDK starts", async () => {
  const f = fixture();
  f.hold();
  const first = f.bot.start();
  f.at(101);
  const second = f.bot.startAndWaitForPorts(8080);
  assert.equal(f.writes(), 1);
  assert.equal(f.ctx.container.running, false);
  f.release();
  await Promise.all([first, second]);
  assert.equal(f.values.get("started"), 100);
});

test("a failed start releases the recording attempt so a retry gets its own boundary", async () => {
  const f = fixture();
  f.fail();
  await assert.rejects(f.bot.start(), /start failed/);
  f.report(100, "previous");
  f.at(200);
  await f.bot.start();
  assert.equal(f.writes(), 2);
  assert.equal(await f.bot.reportedImage(), null);
});
