// node --test test/web.test.ts
// Where a page finds rawr-web's files (scripts/web.ts): the pointer read
// once, the set's addresses, and a set of another protocol refused.
import assert from "node:assert/strict";
import { test } from "node:test";
import { mismatch, parsePointer, POINTER, WEB_PROTOCOL, webFiles } from "../src/scripts/web.ts";

const VERSION = "0123456789abcdef";

test("a pointer names a 16-hex-digit version, a commit and a protocol", () => {
  assert.deepEqual(parsePointer({ version: VERSION, commit: "abc", protocol: 1, uploaded: "x" }), { version: VERSION, commit: "abc", protocol: 1 });
  assert.equal(parsePointer({ version: "../etc", commit: "", protocol: 1 }), null);
  assert.equal(parsePointer({ version: VERSION, commit: "", protocol: "1" }), null);
  assert.equal(parsePointer({ version: VERSION, protocol: 1 }), null);
  assert.equal(parsePointer(null), null);
});

test("a set of another protocol says which side is behind", () => {
  const at = (protocol: number) => ({ version: VERSION, commit: "", protocol });
  assert.equal(mismatch(at(WEB_PROTOCOL)), null);
  assert.match(mismatch(at(WEB_PROTOCOL - 1))!, /this page needs rawr-web protocol \d+, and the site's modules are still protocol \d+\. They update with the next module build\./);
  assert.match(mismatch(at(WEB_PROTOCOL + 1))!, /newer than this page's protocol \d+\. Reload the page\./);
});

/** A fetch that answers the pointer with each of `answers` in turn. */
function site(...answers: (object | number)[]) {
  const fetched: string[] = [];
  const fetch = (async (url: string) => {
    fetched.push(url);
    const answer = answers[Math.min(fetched.length, answers.length) - 1];
    return typeof answer === "number" ? new Response("no", { status: answer }) : Response.json(answer);
  }) as typeof globalThis.fetch;
  return { fetch, fetched };
}

test("the pointer is read once, and every file comes from the set it names", async () => {
  const page = site({ version: VERSION, commit: "abc", protocol: WEB_PROTOCOL });
  const files = webFiles(page.fetch);
  assert.equal(await files("rawr_web.js"), `/web/${VERSION}/rawr_web.js`);
  assert.equal(await files("universe_bg.wasm"), `/web/${VERSION}/universe_bg.wasm`);
  assert.deepEqual(page.fetched, [POINTER]);
});

test("a pointer that failed, or named a set of another protocol, is read again on the next call", async () => {
  const page = site(503, { version: VERSION, commit: "", protocol: WEB_PROTOCOL + 1 }, { version: VERSION, commit: "", protocol: WEB_PROTOCOL });
  const files = webFiles(page.fetch);
  await assert.rejects(files("rawr_web.js"), /rawr-web's pointer \(\/web\/current\.json\) answered 503/);
  await assert.rejects(files("rawr_web.js"), /Reload the page/);
  assert.equal(await files("rawr_web.js"), `/web/${VERSION}/rawr_web.js`);
  assert.equal(page.fetched.length, 3);
});
