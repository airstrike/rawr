import assert from "node:assert/strict";
import { test } from "node:test";

import { secured } from "../src/https.ts";

test("a plain http request moves to https, keeping its path and query", () => {
  const page = secured(new Request("http://rawr.airstrike.fyi/todo?x=1"));
  assert.equal(page?.status, 301);
  assert.equal(page?.headers.get("location"), "https://rawr.airstrike.fyi/todo?x=1");
  const post = secured(new Request("http://rawr.airstrike.fyi/play/run", { method: "POST" }));
  assert.equal(post?.status, 308);
  assert.equal(post?.headers.get("location"), "https://rawr.airstrike.fyi/play/run");
  assert.equal(secured(new Request("https://rawr.airstrike.fyi/")), null);
});
