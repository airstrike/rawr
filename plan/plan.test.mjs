// node --test plan/plan.test.mjs
import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { cpSync, mkdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { test } from "node:test";
import { fileURLToPath } from "node:url";
import { changelogMarkdown, doneCommits, drift, format, issueRefs, issueState, parseTodo, publicText, sentences, shortTitle, siteData, validate } from "./plan.mjs";

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

test("a retire names one of four kinds and what retired it, with an optional one-line note", () => {
  const change = schema("change");
  const ok = { changes: [
    { op: "retire", t: "T031", issue: 31, kind: "not-applicable", by: "D662" },
    { op: "retire", t: "T032", issue: 32, kind: "superseded", by: "#1234", note: "The newer issue names the real cause." },
    { op: "retire", t: "T033", issue: 33, kind: "fixed", by: "607e319ab" },
  ] };
  assert.deepEqual(validate(change, ok, change), []);
  const bad = { changes: [
    { op: "retire", t: "T031", issue: 31, kind: "superseded" },
    { op: "retire", t: "T031", issue: 31, kind: "gone", by: "#1" },
    { op: "retire", t: "T031", issue: 31, kind: "fixed", by: "1234" },
    { op: "retire", t: "T031", issue: 31, kind: "duplicate", by: "#2", note: "x".repeat(301) },
    { op: "retire", t: "T031", issue: 31, kind: "duplicate", by: "#2", note: "one\ntwo" },
    { op: "retire", t: "T031", issue: 31, kind: "duplicate", by: "#2", reason: "free text" },
  ] };
  const errors = validate(change, bad, change);
  for (const [i, start] of [[0, '$.changes[0]: missing "by"'], [1, "$.changes[1].kind"], [2, "$.changes[2].by"], [3, "$.changes[3].note: longer than 300"], [4, "$.changes[4].note"], [5, '$.changes[5]: unknown property "reason"']]) {
    assert.ok(errors.some((e) => e.startsWith(start)), `${i}: ${errors.join("\n")}`);
  }
  const items = schema("items");
  const item = { t: "T031", issue: 31, title: "T031: X", state: "retired", category: "Health", labels: [], gantt: null, decisions: [], commit: null, release: null };
  assert.deepEqual(validate(items, { items: [item] }, items), []);
});

test("an issue closed as not planned or as a duplicate is retired; one closed as completed is closed", () => {
  assert.equal(issueState({ state: "open", state_reason: null }), "open");
  assert.equal(issueState({ state: "open", state_reason: "reopened" }), "open");
  assert.equal(issueState({ state: "closed", state_reason: "completed" }), "closed");
  assert.equal(issueState({ state: "closed", state_reason: null }), "closed");
  assert.equal(issueState({ state: "closed", state_reason: "not_planned" }), "retired");
  assert.equal(issueState({ state: "closed", state_reason: "duplicate" }), "retired");
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
  plan.items.items[0].state = "retired";
  assert.equal(siteData(plan).items[0].state, "retired");
  assert.deepEqual(siteData(plan).groups[0].rows[0].issues[0].state, "retired");
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

// A scratch repository under target/: the plan's script and schemas, every
// crate's manifest, Cargo.lock, the bump script and a plan of three items.
// The `gh` on its PATH fails, so a release that went online would fail.
function scratchRepo(name) {
  const root = fileURLToPath(new URL("..", import.meta.url));
  const dir = join(root, "target", "plan-test", `${name}-${process.pid}`);
  rmSync(dir, { recursive: true, force: true });
  const copy = (path) => cpSync(join(root, path), join(dir, path), { recursive: true });
  for (const path of ["plan/plan.mjs", "plan/schema", "plan/project.json", "scripts/bump-version.sh", "Cargo.toml", "Cargo.lock"]) copy(path);
  for (const crate of ["codegen", "canonical", "cli", "importer", "directory", "toml", "import-cli", "hot", "macros", "location", "play", "reload", "watch", "web"]) {
    copy(`${crate}/Cargo.toml`);
  }
  execFileSync("git", ["init", "-q", dir]);
  mkdirSync(join(dir, "bin"));
  mkdirSync(join(dir, "site/src/lib"), { recursive: true });
  writeFileSync(join(dir, "bin/gh"), "#!/bin/sh\necho \"gh $*: offline\" >&2\nexit 1\n", { mode: 0o755 });
  const project = JSON.parse(readFileSync(join(dir, "plan/project.json")));
  const category = Object.keys(project.categories)[0];
  const version = /^\[package\][^[]*?^version = "([^"]+)"/m.exec(readFileSync(join(dir, "Cargo.toml"), "utf8"))[1];
  const [a, b, p] = version.split(".").map(Number);
  const item = (n, state, commit = null) => ({
    t: `T00${n}`, issue: n, title: `T00${n}: Item ${n}`, state, category, labels: [], gantt: { group: "g", row: "r" }, decisions: [], commit, release: null,
  });
  const write = (path, value) => {
    mkdirSync(dirname(join(dir, path)), { recursive: true });
    writeFileSync(join(dir, path), format(value) + "\n");
  };
  write("plan/items.json", { items: [item(1, "open"), item(2, "closed", "abc1234"), item(3, "open")] });
  write("plan/gantt.json", {
    updated: "2026-11-01", version, examples: { running: 1, total: 2, note: "" },
    groups: [{ id: "g", title: "G", subtitle: "", open: true, rows: [{ id: "r", name: "R", start: 0, size: 1, state: "now", note: "", issues: [1, 2, 3] }] }],
  });
  write("plan/changelog.json", {
    intro: [], tracked_from: `${a}.${b}.${p + 1}`,
    unreleased: { notes: [], entries: [{ text: "Shipped one.", issues: [1] }, { text: "Shipped two.", issues: [2] }] },
    releases: [{ version, date: "2026-11-01", entries: [{ text: "Old.", issues: [] }] }],
  });
  writeFileSync(join(dir, "TODO.md"), `## ${category}\n\n- [ ] **T001** Item 1. (#1)\n- [x] **T002** Done in \`abc1234\`. Item 2. (#2)\n- [ ] **T003** Item 3. (#3)\n`);
  const env = { ...process.env, PATH: `${join(dir, "bin")}:${process.env.PATH}`, RAWR_TODO: join(dir, "TODO.md") };
  const run = (...args) => execFileSync("node", [join(dir, "plan/plan.mjs"), ...args], { cwd: dir, env, encoding: "utf8", stdio: ["ignore", "pipe", "pipe"] });
  run("render");
  const read = (path) => readFileSync(join(dir, path), "utf8");
  return { dir, run, read, version, patch: `${a}.${b}.${p + 1}`, milestone: `${a}.${b + 1}.0` };
}

const escaped = (s) => s.replace(/[.()]/g, "\\$&");

test("a patch release is local: it bumps, writes the section and records the release without GitHub", () => {
  const { dir, run, read, version, patch } = scratchRepo("patch");
  const started = performance.now();
  const out = run("release");
  console.log(`release ${version} -> ${patch}: ${Math.round(performance.now() - started)} ms`);
  assert.match(out, new RegExp(`released ${escaped(patch)} with 2 entries`));
  for (const manifest of ["Cargo.toml", "macros/Cargo.toml", "web/Cargo.toml"]) assert.match(read(manifest), new RegExp(`^version = "${escaped(patch)}"$`, "m"));
  assert.match(read("Cargo.lock"), new RegExp(`name = "rawr"\\nversion = "${escaped(patch)}"`));
  assert.match(read("Cargo.lock"), new RegExp(`name = "rawr-watch"\\nversion = "${escaped(patch)}"`));
  const items = JSON.parse(read("plan/items.json")).items;
  assert.deepEqual(items.map((i) => [i.state, i.release]), [["closed", patch], ["closed", patch], ["open", null]]);
  const changelog = JSON.parse(read("plan/changelog.json"));
  assert.deepEqual(changelog.unreleased.entries, []);
  assert.equal(changelog.releases[0].version, patch);
  assert.match(read("CHANGELOG.md"), new RegExp(`## ${escaped(patch)} \\(\\d{4}-\\d\\d-\\d\\d\\)\\n\\n- Shipped one\\. \\(#1\\)\\n- Shipped two\\. \\(#2\\)`));
  assert.equal(JSON.parse(read("site/src/lib/todo.json")).version, patch);
  assert.match(read("TODO.md"), new RegExp(`- \\[x\\] \\*\\*T001\\*\\* Done in ${escaped(patch)}\\. Item 1\\. \\(#1\\)`));
  assert.match(run("check", "--offline"), /check: no drift/);
  assert.throws(() => run("release"), /nothing is unreleased/);
  rmSync(dir, { recursive: true, force: true });
});

test("a milestone release takes its version and lede, offline too", () => {
  const { dir, run, read, milestone } = scratchRepo("milestone");
  run("release", milestone, "--lede", "The milestone.");
  assert.match(read("Cargo.toml"), new RegExp(`^version = "${escaped(milestone)}"$`, "m"));
  assert.match(read("CHANGELOG.md"), new RegExp(`## ${escaped(milestone)} \\(\\d{4}-\\d\\d-\\d\\d\\)\\n\\nThe milestone\\.\\n\\n- Shipped one`));
  assert.match(run("check", "--offline"), /check: no drift/);
  rmSync(dir, { recursive: true, force: true });
});

test("an issue a release closed locally waits for sync as a note, not drift", () => {
  const plan = {
    project: { repo: "o/r", labels: [], categories: { Health: "health" }, project: {} },
    items: { items: [
      { t: "T001", issue: 1, title: "T001: X", state: "closed", category: "Health", labels: [], gantt: null, decisions: [], commit: null, release: "0.3.5" },
      { t: "T002", issue: 2, title: "T002: Y", state: "closed", category: "Health", labels: [], gantt: null, decisions: [], commit: "abc1234", release: null },
    ] },
    gantt: { version: "0.3.5", groups: [] },
    changelog: { intro: [], tracked_from: "0.3.0", unreleased: { notes: [], entries: [] }, releases: [{ version: "0.3.5", date: "2026-11-01", entries: [{ text: "X.", issues: [1] }] }] },
  };
  const issues = new Map([
    [1, { number: 1, title: "T001: X", state: "open", state_reason: null, labels: [], milestone: null }],
    [2, { number: 2, title: "T002: Y", state: "open", state_reason: null, labels: [], milestone: null }],
  ]);
  const { problems, warnings } = drift(plan, null, issues, null);
  assert.ok(warnings.some((w) => w === "1 released issues wait for GitHub (#1); run sync"), warnings.join("\n"));
  assert.ok(!problems.some((p) => p.startsWith("#1 ")), problems.join("\n"));
  assert.ok(problems.includes("#2 (T002) is open on GitHub, recorded closed"), problems.join("\n"));
});
