// node --test test/todo.test.ts
// /todo's private tabs: the password gate, its limits and cookie, and that
// the private documents never reach the site's build or the Worker's sources.
import assert from "node:assert/strict";
import { existsSync, readFileSync, readdirSync } from "node:fs";
import { test } from "node:test";
import { Limiter } from "../src/play.ts";
import { CLEAR_COOKIE, TODO_COOKIE, UNLOCK_LIMITS, UNLOCK_TOTAL_LIMITS, password, todoRoute, type TodoEnv } from "../src/todo.ts";

const SECRET = "pa55w0rd-for-tests";
const DOC = "<h1>Decisions</h1><p>private words</p>";

function setup(env: Partial<TodoEnv> = {}) {
  const store = new Map([["decisions", DOC], ["code", "<p>code</p>"]]);
  const full: TodoEnv = {
    TODO_PASSWORD: `${SECRET}\n`,
    PLAY_LOG_KEY: "log-key",
    TODO_PRIVATE: { get: async (key: string) => store.get(key) ?? null },
    ...env,
  };
  const perIp = new Limiter(UNLOCK_LIMITS);
  const total = new Limiter(UNLOCK_TOTAL_LIMITS, 1);
  let now = Date.UTC(2026, 9, 28);
  const deps = {
    allow: async (request: Request) => {
      const ip = request.headers.get("cf-connecting-ip") ?? "?";
      return perIp.take(ip, now) || total.take("all", now);
    },
    now: () => now,
  };
  const call = async (method: string, path: string, init: { body?: string; cookie?: string; ip?: string } = {}) => {
    const headers: Record<string, string> = { "cf-connecting-ip": init.ip ?? "1.2.3.4" };
    if (init.cookie) headers.cookie = init.cookie;
    const response = await todoRoute(new Request(`https://rawr.airstrike.fyi${path}`, { method, headers, body: init.body }), full, path, deps);
    assert.ok(response, `${method} ${path} is a /todo route`);
    return response;
  };
  const unlock = (pw: string, ip?: string) => call("POST", "/todo/unlock", { body: JSON.stringify({ password: pw }), ip });
  return { call, unlock, tick: (ms: number) => (now += ms) };
}

/** The cookie's name=value from a Set-Cookie header. */
const pair = (response: Response) => (response.headers.get("set-cookie") ?? "").split(";")[0];

test("a wrong password is refused, with no cookie and no-store", async () => {
  const { unlock } = setup();
  const response = await unlock("guess");
  assert.equal(response.status, 403);
  assert.equal(response.headers.get("set-cookie"), null);
  assert.equal(response.headers.get("cache-control"), "no-store");
});

test("the right password sets an HttpOnly, Secure, SameSite=Strict cookie for /todo that holds no password", async () => {
  const { unlock } = setup();
  for (const given of [SECRET, `${SECRET}\n`]) {
    const response = await unlock(given);
    assert.equal(response.status, 204, `a trailing newline is trimmed on both sides (${JSON.stringify(given.slice(-1))})`);
    const set = response.headers.get("set-cookie") ?? "";
    assert.ok(set.startsWith(`${TODO_COOKIE}=`));
    for (const attribute of ["HttpOnly", "Secure", "SameSite=Strict", "Path=/todo", "Max-Age=2592000"]) assert.ok(set.includes(attribute), attribute);
    assert.ok(!set.includes(SECRET));
    assert.match(pair(response), /^rawr_todo=\d+\.[0-9a-f]{64}$/);
  }
});

test("a private document needs the cookie, and is served with no-store", async () => {
  const { call, unlock } = setup();
  const locked = await call("GET", "/todo/private/decisions");
  assert.equal(locked.status, 401);
  assert.equal(locked.headers.get("cache-control"), "no-store");
  assert.ok(!(await locked.text()).includes("private words"));
  const forged = await call("GET", "/todo/private/decisions", { cookie: `${TODO_COOKIE}=1999999999.${"0".repeat(64)}` });
  assert.equal(forged.status, 401);
  const cookie = pair(await unlock(SECRET));
  const open = await call("GET", "/todo/private/decisions", { cookie });
  assert.equal(open.status, 200);
  assert.equal(open.headers.get("cache-control"), "no-store");
  assert.equal(open.headers.get("content-type"), "text/html; charset=utf-8");
  assert.equal(await open.text(), DOC);
  assert.equal((await call("GET", "/todo/private/code", { cookie })).status, 200);
  assert.equal((await call("GET", "/todo/private/secrets", { cookie })).status, 404);
});

test("the cookie lasts 30 days and ends when the password changes", async () => {
  const first = setup();
  const cookie = pair(await first.unlock(SECRET));
  first.tick(29 * 86_400_000);
  assert.equal((await first.call("GET", "/todo/private/decisions", { cookie })).status, 200);
  first.tick(2 * 86_400_000);
  assert.equal((await first.call("GET", "/todo/private/decisions", { cookie })).status, 401);
  const changed = setup({ TODO_PASSWORD: "another" });
  assert.equal((await changed.call("GET", "/todo/private/decisions", { cookie })).status, 401);
});

test("logging out clears the cookie", async () => {
  const { call } = setup();
  const response = await call("POST", "/todo/lock");
  assert.equal(response.status, 204);
  assert.equal(response.headers.get("set-cookie"), CLEAR_COOKIE);
  assert.ok(CLEAR_COOKIE.includes("Max-Age=0"));
});

test("tries are limited per IP, even with the right password, and in total", async () => {
  const { unlock, tick } = setup();
  for (let i = 0; i < 5; i++) assert.equal((await unlock("guess")).status, 403);
  const limited = await unlock(SECRET);
  assert.equal(limited.status, 429);
  assert.ok(Number(limited.headers.get("retry-after")) > 0);
  assert.equal((await unlock(SECRET, "5.6.7.8")).status, 204, "another IP is not limited");
  tick(61_000);
  assert.equal((await unlock(SECRET)).status, 204, "a minute later the IP may try again");
  // Everyone's tries together stop at 60 an hour.
  const many = setup();
  let refused = 0;
  for (let i = 0; i < 70; i++) if ((await many.unlock("guess", `10.0.0.${i}`)).status === 429) refused++;
  assert.equal(refused, 10);
});

test("unlock wants a POST of JSON, and without the secret nothing opens", async () => {
  const { call } = setup();
  assert.equal((await call("GET", "/todo/unlock")).status, 405);
  assert.equal((await call("POST", "/todo/unlock", { body: "password=x" })).status, 400);
  const bare = setup({ TODO_PASSWORD: undefined });
  assert.equal((await bare.unlock("")).status, 503);
  assert.equal((await bare.call("GET", "/todo/private/decisions", { cookie: `${TODO_COOKIE}=x` })).status, 401);
  assert.equal(password("abc\r\n"), "abc");
  assert.equal(await todoRoute(new Request("https://x/todo/other"), {}, "/todo/other", { allow: async () => 0 }), null);
});

// Lines of the local documents that would give them away. In CI, where the
// documents don't exist, the fixed markers still apply.
function markers(): string[] {
  const out = ["Rawr architectural decisions", "Rawr Decision Code", "Controlling user decisions"];
  const root = new URL("../../../../.Codex/", import.meta.url);
  for (const name of ["DECISIONS.md", "DECISION_CODE.md"]) {
    const file = new URL(name, root);
    if (!existsSync(file)) continue;
    const headings = readFileSync(file, "utf8").split("\n").filter((l) => /^## D\d+ — .{20,}/.test(l));
    for (const h of headings.slice(-5)) out.push(h.slice(3));
  }
  return out;
}

function files(dir: URL): string[] {
  return readdirSync(dir, { recursive: true, withFileTypes: true })
    .filter((e) => e.isFile())
    .map((e) => `${e.parentPath}/${e.name}`);
}

test("no private document is in dist/ or the Worker's sources", () => {
  const found = markers();
  const dirs = [new URL("../dist/", import.meta.url), new URL("../src/", import.meta.url)];
  for (const dir of dirs) {
    if (!existsSync(dir)) continue;
    for (const file of files(dir)) {
      if (!/\.(html|js|mjs|ts|json|txt|md|xml|css)$/.test(file)) continue;
      const text = readFileSync(file, "utf8");
      for (const marker of found) assert.ok(!text.includes(marker), `${file} holds "${marker}"`);
      assert.ok(!/\.Codex\//.test(text) || file.endsWith("todo.ts"), `${file} names .Codex/`);
    }
  }
  assert.ok(existsSync(new URL("../dist/todo.html", import.meta.url)), "the build ran (node build.mjs)");
});
