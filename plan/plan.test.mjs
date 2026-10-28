// node --test plan/plan.test.mjs
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";
import { changelogMarkdown, doneCommits, format, parseTodo, publicText, sentences, shortTitle, validate } from "./plan.mjs";

const schema = (name) => JSON.parse(readFileSync(new URL(`schema/${name}.schema.json`, import.meta.url)));

test("an item keeps its continuation lines and its issue suffix", () => {
  const { items, sections } = parseTodo([
    "## No glue (D593)",
    "",
    "- [ ] **T334** No-glue stops after T313:",
    "  - loupe: `Shell` expected 1 type argument, found 0.",
    "  (D628) (#334)",
    "- [x] **T338** clock: no binding. (D630) Done in `1311da14b` (D632). (#338)",
  ].join("\n"));
  assert.deepEqual(items.get("T334"), {
    t: "T334", done: false, category: "No glue (D593)", start: 2, end: 4, issue: 334,
    text: "No-glue stops after T313:\n  - loupe: `Shell` expected 1 type argument, found 0.\n  (D628)",
  });
  assert.equal(items.get("T338").done, true);
  assert.deepEqual(doneCommits(items.get("T338").text), ["1311da14b"]);
  assert.equal(sections.get("No glue (D593)").end, 5);
});

test("sentences end outside backticks and parentheses; titles drop a leading Done", () => {
  assert.deepEqual(sentences("A `x.y()` call. Then (D571.5) more."), ["A `x.y()` call.", "Then (D571.5) more."]);
  assert.equal(shortTitle("Done in `607e319ab` (D560). A panic printed rawr's text. More."), "A panic printed rawr's text");
  assert.ok(shortTitle("word ".repeat(40)).endsWith("…"));
  assert.ok(shortTitle("word ".repeat(40)).length <= 100);
});

test("public text loses local paths and refuses the old name", () => {
  assert.equal(publicText("a folder (`~/projects/rawr-foo-home`)"), "a folder (`../rawr-foo-home`)");
  assert.throws(() => publicText("the rhai crate"));
});

test("a change picks its schema by op, and names a real issue or \"new\"", () => {
  const change = schema("change");
  const ok = { changes: [{ op: "close", t: "T001", issue: 1, commit: "607e319ab" }, { op: "add", issue: "new", category: "Health", text: "x" }] };
  assert.deepEqual(validate(change, ok, change), []);
  const bad = { changes: [{ op: "close", t: "T001", issue: "new", commit: "607e319ab" }, { op: "shout" }, { op: "add", issue: "soon" }] };
  const errors = validate(change, bad, change);
  assert.ok(errors.some((e) => e.startsWith("$.changes[0].issue")), errors.join("\n"));
  assert.ok(errors.some((e) => e.startsWith("$.changes[1].op: expected one of")), errors.join("\n"));
  assert.ok(errors.some((e) => e.startsWith("$.changes[2].issue")), errors.join("\n"));
});

test("the changelog renders entries with their issues, and the committed JSON is CHANGELOG.md", () => {
  const md = changelogMarkdown({
    intro: ["Intro."],
    tracked_from: "0.3.12",
    unreleased: { notes: ["Aim."], entries: [{ text: "Shipped.", issues: [312, 338], refs: [344] }] },
    releases: [{ version: "0.3.11", date: "2026-10-28", lede: "Lede.", entries: [{ text: "Old.", issues: [] }] }],
  });
  assert.equal(md, "# Changelog\n\nIntro.\n\n## Unreleased\n\n- Aim.\n- Shipped. (#312, #338; part of #344)\n\n## 0.3.11 (2026-10-28)\n\nLede.\n\n- Old.\n");
  const json = JSON.parse(readFileSync(new URL("changelog.json", import.meta.url)));
  assert.equal(changelogMarkdown(json), readFileSync(new URL("../CHANGELOG.md", import.meta.url), "utf8"));
});

test("records print one per line", () => {
  assert.equal(format({ items: [{ t: "T001", issue: 1 }, { t: "T002", issue: 2 }] }), '{\n  "items": [\n    {"t":"T001","issue":1},\n    {"t":"T002","issue":2}\n  ]\n}');
});
