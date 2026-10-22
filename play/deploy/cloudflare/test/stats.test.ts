// node --test test/stats.test.ts  (Node 23.6+ runs TypeScript directly)
import assert from "node:assert/strict";
import { test } from "node:test";
import { render, state, type Dashboard } from "../src/dashboard.ts";
import {
  MAX_EVENTS,
  bucket,
  emptyStats,
  merge,
  parseEvent,
  parseReport,
  percentile,
  pushEvent,
  summarize,
  type Report,
} from "../src/stats.ts";

const HOUR = 3_600_000;

function report(over: Partial<Report> = {}): Report {
  return {
    version: "abc",
    boot: 100,
    ready: 105,
    servers: 2,
    channels: 3,
    channels_loaded: true,
    ok: 0,
    code: 0,
    limit: 0,
    rate_limited: 0,
    durations_ms: [],
    per_server: [],
    ...over,
  };
}

test("reports merge into hours, totals and the last status", () => {
  const t0 = 1000 * HOUR;
  let stats = emptyStats();
  stats = merge(stats, report({ ok: 2, durations_ms: [40, 120] }), t0);
  stats = merge(stats, report({ code: 1, rate_limited: 3, durations_ms: [9] }), t0 + 60_000);
  stats = merge(stats, report({ limit: 1, servers: 4, durations_ms: [20000] }), t0 + HOUR);
  assert.equal(stats.hours.length, 2);
  assert.deepEqual(stats.totals, { ok: 2, code: 1, limit: 1, rate_limited: 3, web_ok: 0, web_code: 0, web_limit: 0, web_rate_limited: 0 });
  assert.equal(stats.status?.servers, 4);
  assert.equal(stats.status?.seen, (t0 + HOUR) / 1000);
  const s = summarize(stats, t0 + HOUR);
  assert.equal(s.day.renders, 4);
  assert.equal(s.totals.renders, 4);
  assert.equal(s.p50, 50);
  assert.equal(s.p95, Infinity);
});

test("only the last 24 hours count as the day; totals keep everything", () => {
  const t0 = 2000 * HOUR;
  let stats = merge(emptyStats(), report({ ok: 5, durations_ms: [10] }), t0);
  stats = merge(stats, report({ ok: 1, durations_ms: [300] }), t0 + 24 * HOUR);
  assert.equal(stats.hours.length, 1, "the hour a day old is dropped");
  const s = summarize(stats, t0 + 24 * HOUR);
  assert.equal(s.day.ok, 1);
  assert.equal(s.totals.ok, 6);
  assert.equal(s.p50, 300);
  assert.equal(summarize(stats, t0 + 48 * HOUR).day.renders, 0);
});

const TOKEN = "0123456789abcdef0123456789abcdef";

test("per-server rows add up by token, keep the latest channel count and last activity", () => {
  let stats = merge(
    emptyStats(),
    report({ per_server: [{ server: TOKEN, ok: 2, code: 1, limit: 0, rate_limited: 0, channels: 1, last: 50 }] }),
    HOUR,
  );
  stats = merge(
    stats,
    report({ per_server: [{ server: TOKEN, ok: 1, code: 0, limit: 1, rate_limited: 4, channels: 3, last: null }] }),
    HOUR,
  );
  assert.deepEqual(stats.servers?.[TOKEN], { ok: 3, code: 1, limit: 1, rate_limited: 4, channels: 3, last: 50 });
  // An old report without per-server rows still parses.
  const { per_server: _, ...old } = report();
  assert.deepEqual(parseReport(old)?.per_server, []);
  assert.equal(parseReport({ ...report(), per_server: [{ server: "123", ok: 0 }] }), null, "only tokens");
  assert.equal(
    parseReport({ ...report(), per_server: [{ server: TOKEN.toUpperCase(), ok: 0, code: 0, limit: 0, rate_limited: 0, channels: 0, last: null }] }),
    null,
  );
});

test("percentiles are bucket upper bounds", () => {
  assert.equal(bucket(0), 0);
  assert.equal(bucket(10), 0);
  assert.equal(bucket(11), 1);
  assert.equal(percentile([0, 0, 0], 0.5), null);
  const hist = [1, 1, 1, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
  assert.equal(percentile(hist, 0.5), 15);
  assert.equal(percentile(hist, 0.95), 30);
});

test("bad reports and events are refused", () => {
  assert.ok(parseReport(report()));
  assert.equal(parseReport(null), null);
  assert.equal(parseReport({ ...report(), ok: -1 }), null);
  assert.equal(parseReport({ ...report(), ok: 1.5 }), null);
  assert.equal(parseReport({ ...report(), durations_ms: ["1"] }), null);
  assert.equal(parseReport({ ...report(), version: "x".repeat(65) }), null);
  assert.equal(parseReport({ ...report(), ready: "soon" }), null);
  assert.deepEqual(parseEvent({ phase: "ready", detail: "2 servers" }, 7), {
    t: 7,
    source: "bot",
    phase: "ready",
    detail: "2 servers",
  });
  assert.equal(parseEvent({ phase: "", detail: "" }, 7), null);
  assert.equal(parseEvent({ phase: "x" }, 7), null);
  assert.equal(parseEvent({ phase: "x", detail: "y".repeat(900) }, 7)?.detail.length, 500);
});

test("events keep the last MAX_EVENTS", () => {
  let events = [] as ReturnType<typeof pushEvent>;
  for (let i = 0; i < MAX_EVENTS + 3; i++) {
    events = pushEvent(events, { t: i, source: "worker", phase: "p", detail: "" });
  }
  assert.equal(events.length, MAX_EVENTS);
  assert.equal(events[0].t, 3);
});

test("the dashboard states and escapes", () => {
  const stats = merge(emptyStats(), report({ ok: 1, durations_ms: [12] }), 5000 * HOUR);
  const now = 5000 * 3600 + 30;
  const d: Dashboard = {
    summary: summarize(stats, 5000 * HOUR),
    status: stats.status,
    events: [{ t: now, source: "bot", phase: "error", detail: "<script>x</script>" }],
    servers: { [TOKEN]: { ok: 1, code: 0, limit: 0, rate_limited: 0, channels: 2, last: now } },
    running: true,
    deployed: "def",
    started: "def",
    now,
  };
  assert.equal(state(d), "ready");
  assert.equal(state({ ...d, running: false }), "stopped");
  assert.equal(state({ ...d, now: now + 600 }), "silent");
  assert.equal(state({ ...d, status: { ...d.status!, ready: null } }), "starting");
  const html = render(d);
  assert.ok(html.includes("&#60;script&#62;"));
  assert.ok(!html.includes("<script>"));
  assert.ok(html.includes("≠ deployed def"), "an image other than the deployed one is flagged");
  assert.ok(html.includes('href="/bot/invite"'));
  assert.ok(html.includes(TOKEN), "servers are listed by token");
});
