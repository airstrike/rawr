// node --test plan/plan.test.mjs
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";
import { changelogMarkdown, doneCommits, format, issueRefs, parseTodo, publicText, sentences, shortTitle, siteData, validate } from "./plan.mjs";

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

test("notes name issues as #N; r#for and entities are not issues", () => {
  assert.deepEqual(issueRefs("exit runs. #336, #340, part of #345, r#for, &#123;"), [336, 340, 345]);
});

test("the page's data names each recorded T number as its issue", () => {
  const plan = {
    project: { repo: "airstrike/rawr" },
    items: { items: [{ t: "T009", issue: 2, title: "T009: Fix T010", state: "open", category: "Health", release: null }, { t: "T010", issue: 10, title: "T010: X", state: "closed", category: "Health", release: "0.3.1" }] },
    gantt: { updated: "2026-10-28", version: "0.3.14", examples: {}, groups: [{ id: "g", title: "G", subtitle: "", open: true, rows: [{ id: "r", name: "R", start: 0, size: 1, state: "now", note: "T009. #10", issues: [2] }] }] },
    changelog: { intro: [], unreleased: { notes: ["Aim at T009."], entries: [] }, releases: [{ version: "0.3.1", date: "2026-10-01", entries: [{ text: "Fixed T010.", issues: [10] }] }] },
  };
  const data = siteData(plan);
  assert.equal(data.groups[0].rows[0].note, "#2. #10");
  assert.deepEqual(data.groups[0].rows[0].issues, [{ number: 2, title: "Fix #10", state: "open" }]);
  assert.deepEqual(data.items, [{ issue: 2, title: "Fix #10", state: "open", category: "Health" }, { issue: 10, title: "X", state: "closed", category: "Health", release: "0.3.1" }]);
  assert.equal(data.changelog.unreleased.notes[0], "Aim at #2.");
  assert.equal(data.changelog.releases[0].entries[0].text, "Fixed #10.");
  plan.changelog.intro = ["T999"];
  assert.throws(() => siteData(plan), /T999 is no recorded item/);
});

test("the private documents lose local paths, secrets and the old name, and link T numbers", async () => {
  const { leaks, linkIssues, scrub } = await import("./private.mjs");
  const md = scrub("See /Users/x/projects/rhai/src/a.rs and /Users/x/projects/other/b, ~/.cargo, ../rhai/macros. Rhai's AST; the rhai crate. ghp_" + "a".repeat(30), ["/Users/x/projects/rhai"]);
  assert.equal(md, "See src/a.rs and ../other/b, ~/.cargo, ../rawr/macros. Upstream's AST; the upstream crate. [redacted]");
  assert.deepEqual(leaks(md), []);
  assert.deepEqual(leaks("/Users/x/y rhai"), ["a local path", "the old name"]);
  const items = [{ t: "T002", issue: 2 }];
  assert.equal(
    linkIssues("<p>T002 and <code>T002</code> and <a href=\"#\">T002</a></p><pre><code>T002</code></pre>", items, "o/r"),
    '<p><a href="https://github.com/o/r/issues/2">#2</a> and <code>T002</code> and <a href="#">T002</a></p><pre><code>T002</code></pre>',
  );
  assert.throws(() => linkIssues("<p>T404</p>", items, "o/r"), /T404 is no recorded item/);
});
