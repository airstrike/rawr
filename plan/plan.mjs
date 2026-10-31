#!/usr/bin/env node
// The work plan's one script. Every TODO item is a GitHub issue, and plan/
// holds the records that tie the items, the issues, the /todo chart and the
// changelog together. Changes go through a change file
// (schema/change.schema.json). The script checks it against the schemas and
// against GitHub before it writes anything, creates the issues it says are
// "new", and writes the outputs: TODO.md's lines, the chart's data
// (site/src/lib/todo.json) and CHANGELOG.md.
//
//   node plan/plan.mjs check [--offline]          report drift; exit 1 if any
//   node plan/plan.mjs apply <file> [--dry-run]   apply a change file
//   node plan/plan.mjs render                     rewrite the outputs from the records
//   node plan/plan.mjs sync                       set the Project's fields from the records
//   node plan/plan.mjs setup                      create the labels, the Project and its fields
//
// TODO.md is local and untracked: .Codex/TODO.md in the main checkout, which
// worktrees share, or $RAWR_TODO. Needs node 22 or later and an authenticated gh.
import { execFileSync } from "node:child_process";
import { existsSync, readFileSync, renameSync, writeFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const PLAN = dirname(fileURLToPath(import.meta.url));
const ROOT = dirname(PLAN);
const FILES = {
  items: join(PLAN, "items.json"),
  gantt: join(PLAN, "gantt.json"),
  changelog: join(PLAN, "changelog.json"),
  project: join(PLAN, "project.json"),
  site: join(ROOT, "site/src/lib/todo.json"),
  changelogMd: join(ROOT, "CHANGELOG.md"),
};

// ---------------------------------------------------------------- files

const readJson = (path) => JSON.parse(readFileSync(path, "utf8"));
const isObj = (v) => v !== null && typeof v === "object" && !Array.isArray(v);

// Arrays of records print one record per line, so diffs and merges are per record.
export function format(value, indent = "") {
  const inner = indent + "  ";
  if (Array.isArray(value)) {
    if (value.length && value.every(isObj)) {
      return `[\n${value.map((v) => inner + format(v, inner)).join(",\n")}\n${indent}]`;
    }
    return JSON.stringify(value);
  }
  if (isObj(value)) {
    const nested = Object.values(value).some((v) => Array.isArray(v) && v.some(isObj)) ||
      (indent.length <= 2 && JSON.stringify(value).length > 120);
    if (!nested) return JSON.stringify(value);
    const entries = Object.entries(value).map(([k, v]) => `${inner}${JSON.stringify(k)}: ${format(v, inner)}`);
    return `{\n${entries.join(",\n")}\n${indent}}`;
  }
  return JSON.stringify(value);
}

function writeAtomic(path, text) {
  writeFileSync(`${path}.tmp`, text);
  renameSync(`${path}.tmp`, path);
}
const writeJson = (path, value) => writeAtomic(path, format(value) + "\n");

function todoPath() {
  if (process.env.RAWR_TODO) return resolve(process.env.RAWR_TODO);
  const common = git("rev-parse", "--path-format=absolute", "--git-common-dir");
  return join(dirname(common), ".Codex/TODO.md");
}

const git = (...args) => execFileSync("git", ["-C", ROOT, ...args], { encoding: "utf8" }).trim();
const today = () => new Date().toLocaleDateString("sv-SE");

function cargoVersion() {
  const text = readFileSync(join(ROOT, "Cargo.toml"), "utf8");
  return /^\[package\][^[]*?^version = "([^"]+)"/m.exec(text)[1];
}

const cmpVersion = (a, b) => {
  const [x, y] = [a, b].map((v) => v.split(".").map(Number));
  return x[0] - y[0] || x[1] - y[1] || x[2] - y[2];
};

// ---------------------------------------------------------------- schemas

const SCHEMAS = {};
for (const name of ["defs", "items", "gantt", "changelog", "change", "project"]) {
  SCHEMAS[`${name}.schema.json`] = readJson(join(PLAN, "schema", `${name}.schema.json`));
}

function pointer(doc, ptr) {
  return ptr.split("/").filter(Boolean).reduce((node, key) => node[key.replace(/~1/g, "/").replace(/~0/g, "~")], doc);
}

function deref(schema, doc) {
  while (schema.$ref) {
    const [file, ptr = ""] = schema.$ref.split("#");
    if (file) doc = SCHEMAS[file];
    schema = pointer(doc, ptr);
  }
  return [schema, doc];
}

const typeOf = (v) => (v === null ? "null" : Array.isArray(v) ? "array" : Number.isInteger(v) ? "integer" : typeof v);
const fits = (type, v) => type === typeOf(v) || (type === "number" && typeof v === "number");

// The subset of JSON Schema 2020-12 the plan's schemas use. A oneOf whose
// branches each fix one property with a const picks its branch by that value.
export function validate(schema, value, doc, path = "$", errors = []) {
  [schema, doc] = deref(schema, doc);
  if (schema.oneOf) {
    const branches = schema.oneOf.map((b) => deref(b, doc));
    const key = isObj(value) && branches.every(([b]) => b.properties) &&
      Object.keys(branches[0][0].properties).find((k) => branches.every(([b]) => b.properties[k]?.const !== undefined));
    if (key) {
      const hit = branches.find(([b]) => b.properties[key].const === value[key]);
      if (hit) validate(hit[0], value, hit[1], path, errors);
      else errors.push(`${path}.${key}: expected one of ${branches.map(([b]) => JSON.stringify(b.properties[key].const)).join(", ")}`);
    } else {
      const passing = branches.filter(([b, d]) => validate(b, value, d, path).length === 0).length;
      if (passing !== 1) errors.push(`${path}: ${JSON.stringify(value)} fits ${passing} of its ${branches.length} alternatives, not one`);
    }
  }
  if ("const" in schema && JSON.stringify(schema.const) !== JSON.stringify(value)) {
    errors.push(`${path}: expected ${JSON.stringify(schema.const)}`);
  }
  if (schema.enum && !schema.enum.includes(value)) errors.push(`${path}: expected one of ${schema.enum.join(", ")}`);
  if (schema.type && ![].concat(schema.type).some((t) => fits(t, value))) {
    errors.push(`${path}: expected ${[].concat(schema.type).join(" or ")}, found ${typeOf(value)}`);
    return errors;
  }
  if (typeof value === "string") {
    if (schema.minLength !== undefined && value.length < schema.minLength) errors.push(`${path}: shorter than ${schema.minLength}`);
    if (schema.maxLength !== undefined && value.length > schema.maxLength) errors.push(`${path}: longer than ${schema.maxLength}`);
    if (schema.pattern && !new RegExp(schema.pattern, "u").test(value)) errors.push(`${path}: ${JSON.stringify(value)} doesn't match ${schema.pattern}`);
  }
  if (typeof value === "number") {
    if (schema.minimum !== undefined && value < schema.minimum) errors.push(`${path}: below ${schema.minimum}`);
  }
  if (Array.isArray(value)) {
    if (schema.minItems !== undefined && value.length < schema.minItems) errors.push(`${path}: fewer than ${schema.minItems} items`);
    if (schema.uniqueItems && new Set(value.map((v) => JSON.stringify(v))).size !== value.length) errors.push(`${path}: repeats an item`);
    if (schema.items) value.forEach((v, i) => validate(schema.items, v, doc, `${path}[${i}]`, errors));
  }
  if (isObj(value)) {
    if (schema.minProperties !== undefined && Object.keys(value).length < schema.minProperties) {
      errors.push(`${path}: needs at least ${schema.minProperties} properties`);
    }
    for (const key of schema.required ?? []) if (!(key in value)) errors.push(`${path}: missing "${key}"`);
    for (const [key, v] of Object.entries(value)) {
      const sub = schema.properties?.[key];
      if (sub) validate(sub, v, doc, `${path}.${key}`, errors);
      else if (schema.additionalProperties === false) errors.push(`${path}: unknown property "${key}"`);
      else if (isObj(schema.additionalProperties)) validate(schema.additionalProperties, v, doc, `${path}.${key}`, errors);
    }
  }
  return errors;
}

const check = (name, value) => validate(SCHEMAS[`${name}.schema.json`], value, SCHEMAS[`${name}.schema.json`]);

// ---------------------------------------------------------------- TODO.md

const ITEM = /^- \[([ x])\] \*\*(T\d{3,})\*\* ?(.*)$/;
const SUFFIX = / \(#(\d+)\)$/;

export function parseTodo(text) {
  const lines = text.split("\n");
  const items = new Map();
  const sections = new Map();
  let category = null;
  for (let i = 0; i < lines.length; i++) {
    const heading = /^## (.+)$/.exec(lines[i]);
    if (heading) {
      category = heading[1];
      sections.set(category, { start: i, end: i });
      continue;
    }
    const m = ITEM.exec(lines[i]);
    if (!m) continue;
    let end = i;
    while (end + 1 < lines.length && /^ {2}\S/.test(lines[end + 1])) end++;
    let body = [m[3], ...lines.slice(i + 1, end + 1)].join("\n");
    const suffix = SUFFIX.exec(body);
    if (suffix) body = body.slice(0, suffix.index);
    if (items.has(m[2])) throw new Error(`TODO.md lists ${m[2]} twice`);
    items.set(m[2], { t: m[2], done: m[1] === "x", category, start: i, end, text: body, issue: suffix ? +suffix[1] : null });
    if (category) sections.get(category).end = end;
    i = end;
  }
  return { lines, items, sections };
}

const itemLines = ({ done, t, text, issue }) => {
  const lines = `- [${done ? "x" : " "}] **${t}** ${text}${issue ? ` (#${issue})` : ""}`.split("\n");
  return lines;
};

// Others edit TODO.md too, so a live Todo reads the file before each change
// and writes it right after. A dry one keeps its changes in memory.
class Todo {
  constructor(path, dry = false) {
    Object.assign(this, { path, dry });
    this.reload();
  }
  reload() {
    this.text = existsSync(this.path) ? readFileSync(this.path, "utf8") : null;
    this._parsed = null;
  }
  get parsed() {
    if (!this.dry) this.reload();
    if (this.text === null) throw new Error(`no TODO.md at ${this.path}; set RAWR_TODO`);
    return (this._parsed ??= parseTodo(this.text));
  }
  set(item) {
    const { lines, items } = this.parsed;
    const old = items.get(item.t);
    lines.splice(old.start, old.end - old.start + 1, ...itemLines(item));
    this.update(lines);
  }
  append(category, item) {
    const { lines, sections } = this.parsed;
    const section = sections.get(category);
    lines.splice(section.end + 1, 0, ...itemLines(item));
    this.update(lines);
  }
  update(lines) {
    this.text = lines.join("\n");
    this._parsed = null;
    if (!this.dry) writeAtomic(this.path, this.text);
  }
}

// ---------------------------------------------------------------- text

// Sentences end at ". " outside backticks and parentheses.
export function sentences(text) {
  const out = [];
  let tick = false, depth = 0, start = 0;
  for (let i = 0; i < text.length; i++) {
    const c = text[i];
    if (c === "`") tick = !tick;
    else if (!tick && c === "(") depth++;
    else if (!tick && c === ")") depth = Math.max(0, depth - 1);
    else if (!tick && depth === 0 && c === "." && /\s/.test(text[i + 1] ?? " ")) {
      out.push(text.slice(start, i + 1).trim());
      start = i + 1;
    }
  }
  if (text.slice(start).trim()) out.push(text.slice(start).trim());
  return out;
}

export function shortTitle(text) {
  const all = sentences(text.split("\n")[0]);
  const rest = all.filter((s) => !/^Done\b/.test(s));
  let s = (rest[0] ?? all[0]).replace(/\s*\((?:[DT]\d+[^)]*)\)\s*\.?$/, "").replace(/[.:]$/, "");
  if (s.length <= 100) return s;
  const words = s.split(" ");
  let out = "";
  for (const w of words) {
    if ((out + " " + w).length > 96) break;
    out = out ? `${out} ${w}` : w;
  }
  if ((out.match(/`/g) ?? []).length % 2) out = out.slice(0, out.lastIndexOf("`")).trimEnd();
  return out.replace(/[,;:(]$/, "") + "…";
}

export const tRefs = (text) => [...new Set(text.match(/\bT\d{3,}\b/g) ?? [])];
// The issues a text names as #N. `r#for` and `&#123;` are not issues.
export const ISSUE_REF = /(?<![\w&])#(\d+)\b/g;
export const issueRefs = (text) => [...new Set([...text.matchAll(ISSUE_REF)].map((m) => +m[1]))];
// Each T number written as its issue (#N), through the items. A T number no
// item records is an error.
export const tAsIssues = (text, items) => text
  .replace(/\b(T\d{3,}) \(#(\d+)\)/g, (m, t, n) => (items.find((i) => i.t === t)?.issue === +n ? t : m))
  .replace(/\bT\d{3,}\b/g, (t) => {
  const item = items.find((i) => i.t === t);
  if (!item) throw new Error(`${t} is no recorded item: ${text.slice(0, 120)}`);
  return `#${item.issue}`;
});
export const dRefs = (text) => [...new Set((text.match(/\bD\d{3,}/g) ?? []))];
// The commits of the first "Done in `abc1234`" in the text.
export const doneCommits = (text) => {
  for (const s of sentences(text)) {
    const at = s.search(/\bDone in\b/);
    const hits = at >= 0 && s.slice(at).match(/`[0-9a-f]{7,40}`/g);
    if (hits) return hits.map((c) => c.slice(1, -1));
  }
  return [];
};

// Local paths become relative ones: ../<project>/ or ~.
export const withoutLocalPaths = (text) => text
  .replace(/\/(?:Users|home)\/[^/\s`]+\/projects\//g, "../")
  .replace(/\/(?:Users|home)\/[^/\s`]+/g, "~")
  .replace(/~\/projects\//g, "../");
// Tokens and keys: GitHub, OpenAI-style, AWS, Slack, Discord bot tokens, PEM keys.
export const SECRET = /\bgh[pousr]_[A-Za-z0-9]{20,}|\bsk-[A-Za-z0-9-]{20,}|\bAKIA[0-9A-Z]{16}\b|\bxox[bpas]-[A-Za-z0-9-]*|\b[MN][A-Za-z\d]{23,25}\.[\w-]{6}\.[\w-]{27,}|-----BEGIN [A-Z ]*PRIVATE KEY-----[\s\S]*?-----END [A-Z ]*PRIVATE KEY-----/g;

// The project's old name, which only the licenses and the README may name.
export const OLD_NAME = /rhai/gi;

// The repository is public: no local paths, no tokens, never the old name.
export function publicText(text) {
  const out = withoutLocalPaths(text);
  if (new RegExp(OLD_NAME.source, "i").test(out)) throw new Error(`text names the old project: ${out.slice(0, 120)}`);
  if (new RegExp(SECRET.source).test(out)) {
    throw new Error(`text looks like it holds a secret: ${out.slice(0, 120)}`);
  }
  return out;
}

const issueBody = (t, category, text) => `${publicText(text)}\n\n---\n${t} in the work plan, under ${category}.\n`;

// ---------------------------------------------------------------- GitHub

const sleep = (ms) => Atomics.wait(new Int32Array(new SharedArrayBuffer(4)), 0, 0, ms);
let lastWrite = 0;

function gh(args, input, { write = false } = {}) {
  for (let attempt = 0; ; attempt++) {
    if (write) {
      // GitHub's secondary limits: at most 80 content writes a minute, and a
      // second between writes. Keep under both.
      const wait = lastWrite + 1100 - Date.now();
      if (wait > 0) sleep(wait);
      lastWrite = Date.now();
    }
    try {
      return execFileSync("gh", args, { input, encoding: "utf8", maxBuffer: 1 << 28, stdio: ["pipe", "pipe", "pipe"] });
    } catch (error) {
      const message = `${error.stderr ?? ""}${error.stdout ?? ""}`;
      if (attempt < 6 && /secondary rate limit|rate limit exceeded|submitted too quickly|abuse|HTTP 50[234]|was a problem/i.test(message)) {
        const ms = 60_000 * 2 ** attempt;
        console.error(`gh: ${message.trim().split("\n")[0]}; waiting ${ms / 1000} s`);
        sleep(ms);
        continue;
      }
      throw new Error(`gh ${args.slice(0, 4).join(" ")}: ${message.trim()}`);
    }
  }
}

export function graphql(query, variables = {}, write = /^\s*mutation/.test(query)) {
  const out = JSON.parse(gh(["api", "graphql", "--input", "-"], JSON.stringify({ query, variables }), { write }));
  if (out.errors) throw new Error(`graphql: ${JSON.stringify(out.errors)}`);
  return out.data;
}

export function rest(method, path, body) {
  const args = ["api", "-X", method, path];
  if (body) args.push("--input", "-");
  const out = gh(args, body ? JSON.stringify(body) : undefined, { write: method !== "GET" });
  return out.trim() ? JSON.parse(out) : null;
}

export function fetchIssues(repo) {
  const out = gh(["api", "--paginate", `repos/${repo}/issues?state=all&per_page=100`, "--jq",
    ".[] | select(.pull_request == null) | {number, title, state, labels: [.labels[].name], node_id, milestone: .milestone.title}"]);
  const issues = new Map();
  for (const line of out.split("\n").filter(Boolean)) {
    const issue = JSON.parse(line);
    issues.set(issue.number, issue);
  }
  return issues;
}

const q = (s) => JSON.stringify(s);

/** The milestone a version ships in: its minor series, as iced's 0.13 holds 0.13.0, 0.13.1 and so on. */
export const milestoneOf = (version) => version.split(".").slice(0, 2).join(".");

/** The repository's milestones by title, each with its number. */
function fetchMilestones(repo) {
  const out = gh(["api", "--paginate", `repos/${repo}/milestones?state=all&per_page=100`, "--jq", "[.[] | {title, number}]"]);
  return new Map(JSON.parse(out.replace(/\]\s*\[/g, ",") || "[]").map((m) => [m.title, m.number]));
}

/** Puts an issue in a milestone, making the milestone first if the repository has none of that title. */
function setMilestone(repo, milestones, number, title) {
  let id = milestones.get(title);
  if (!id) {
    id = rest("POST", `repos/${repo}/milestones`, { title }).number;
    milestones.set(title, id);
  }
  rest("PATCH", `repos/${repo}/issues/${number}`, { milestone: id });
}

export function fetchProjectItems(project) {
  const items = new Map();
  let after = null;
  do {
    const data = graphql(`query($id: ID!, $after: String) { node(id: $id) { ... on ProjectV2 {
      items(first: 100, after: $after) { pageInfo { hasNextPage endCursor } nodes { id
        content { ... on Issue { number } }
        fieldValues(first: 30) { nodes {
          ... on ProjectV2ItemFieldTextValue { text field { ... on ProjectV2FieldCommon { id } } }
          ... on ProjectV2ItemFieldSingleSelectValue { optionId field { ... on ProjectV2FieldCommon { id } } } } } } } } } }`,
    { id: project.project.id, after });
    const page = data.node.items;
    for (const node of page.nodes) {
      if (!node.content?.number) continue;
      const values = new Map(node.fieldValues.nodes.filter((v) => v.field).map((v) => [v.field.id, v.text ?? v.optionId]));
      items.set(node.content.number, { id: node.id, values });
    }
    after = page.pageInfo.hasNextPage ? page.pageInfo.endCursor : null;
  } while (after);
  return items;
}

// ---------------------------------------------------------------- the plan

export function load() {
  const plan = {
    items: existsSync(FILES.items) ? readJson(FILES.items) : { $schema: "schema/items.schema.json", items: [] },
    gantt: readJson(FILES.gantt),
    changelog: readJson(FILES.changelog),
    project: readJson(FILES.project),
  };
  const errors = [];
  for (const name of ["items", "gantt", "changelog", "project"]) {
    errors.push(...check(name, plan[name]).map((e) => `plan/${name}.json: ${e}`));
  }
  return { plan, errors };
}

const byT = (plan, t) => plan.items.items.find((i) => i.t === t);
const byIssue = (plan, n) => plan.items.items.find((i) => i.issue === n);
const findRow = (plan, ref) => {
  const group = plan.gantt.groups.find((g) => g.id === ref.group);
  return group && { group, row: group.rows.find((r) => r.id === ref.row) };
};
const tNumber = (t) => +t.slice(1);
const rowLabel = (plan, ref) => {
  const found = ref && findRow(plan, ref);
  return found?.row ? `${found.group.title} · ${found.row.name}` : null;
};

export function labelsFor(project, category, text) {
  const labels = [project.categories[category]];
  for (const { label, pattern } of project.keywords) if (new RegExp(pattern, "i").test(text)) labels.push(label);
  return [...new Set(labels.filter(Boolean))];
}

function desiredFields(plan, item) {
  const row = item.gantt && findRow(plan, item.gantt)?.row;
  return {
    status: item.state === "closed" ? "Done" : row?.state === "now" ? "In Progress" : "Todo",
    category: item.category,
    t: item.t,
    gantt: rowLabel(plan, item.gantt),
    decisions: item.decisions.join(", ") || null,
    commit: item.commit,
    release: item.release,
  };
}

// The mutations that bring one Project item's fields to the record.
function fieldMutations(plan, item, itemId, current, prefix) {
  const { fields, project } = plan.project;
  const out = [];
  for (const [role, value] of Object.entries(desiredFields(plan, item))) {
    const field = fields?.[role];
    if (!field) continue;
    const want = value == null ? null : field.options ? field.options[value] : String(value);
    if (value != null && field.options && !want) throw new Error(`the Project's ${field.name} has no option "${value}"; run setup`);
    if (current && (current.get(field.id) ?? null) === want) continue;
    const target = `projectId: ${q(project.id)}, itemId: ${q(itemId)}, fieldId: ${q(field.id)}`;
    out.push(want == null
      ? `${prefix}${role}: clearProjectV2ItemFieldValue(input: { ${target} }) { clientMutationId }`
      : `${prefix}${role}: updateProjectV2ItemFieldValue(input: { ${target}, value: { ${field.options ? "singleSelectOptionId" : "text"}: ${q(want)} } }) { clientMutationId }`);
  }
  return out;
}

function syncItems(plan, items, projectItems) {
  const batch = [];
  const flush = () => {
    if (batch.length) graphql(`mutation { ${batch.join("\n")} }`);
    batch.length = 0;
  };
  items.forEach((item, i) => {
    let entry = projectItems.get(item.issue);
    if (!entry) {
      // A new issue's Project item can be missing from the answer that made it.
      // Adding one that is there but not shown yet is refused, so look again.
      const [owner, name] = plan.project.repo.split("/");
      let id = null;
      for (let attempt = 0; !id; attempt++) {
        const { issue } = graphql(`query { repository(owner: ${q(owner)}, name: ${q(name)}) { issue(number: ${item.issue}) { id
          projectItems(first: 20) { nodes { id project { id } } } } } }`).repository;
        id = issue.projectItems.nodes.find((n) => n.project.id === plan.project.project.id)?.id;
        if (id) break;
        try {
          id = graphql(`mutation { addProjectV2ItemById(input: { projectId: ${q(plan.project.project.id)}, contentId: ${q(issue.id)} }) { item { id } } }`).addProjectV2ItemById.item.id;
        } catch (error) {
          if (attempt >= 5 || !/already exists/.test(error.message)) throw error;
          sleep(3000 * (attempt + 1));
        }
      }
      entry = { id, values: new Map() };
      projectItems.set(item.issue, entry);
    }
    batch.push(...fieldMutations(plan, item, entry.id, entry.values, `i${i}_`));
    if (batch.length >= 40) flush();
  });
  flush();
}

// ---------------------------------------------------------------- outputs

const entryLine = (e) => {
  const refs = [e.issues.map((n) => `#${n}`).join(", "), e.refs?.length ? `part of ${e.refs.map((n) => `#${n}`).join(", ")}` : ""];
  const said = refs.filter(Boolean).join("; ");
  return `- ${e.text}${said ? ` (${said})` : ""}`;
};

export function changelogMarkdown(c) {
  const out = ["# Changelog", ""];
  for (const p of c.intro) out.push(p, "");
  out.push("## Unreleased", "");
  const unreleased = [...c.unreleased.notes.map((n) => `- ${n}`), ...c.unreleased.entries.map(entryLine)];
  if (unreleased.length) out.push(...unreleased, "");
  for (const r of c.releases) {
    out.push(`## ${r.version} (${r.date})`, "");
    if (r.lede) out.push(r.lede, "");
    out.push(...r.entries.map(entryLine), "");
  }
  return out.join("\n");
}

// The /todo page's data: the chart, the changelog and the items. Issues are
// named as #N throughout, so a T number becomes its issue.
export function siteData(plan) {
  const items = new Map(plan.items.items.map((i) => [i.issue, i]));
  const text = (t) => tAsIssues(t, plan.items.items);
  const title = (i) => text(i.title.replace(/^T\d{3,}: /, ""));
  const entry = (e) => ({ text: text(e.text), issues: e.issues, ...(e.refs?.length ? { refs: e.refs } : {}) });
  const { updated, version, examples, groups } = plan.gantt;
  const c = plan.changelog;
  return {
    repo: plan.project.repo,
    updated,
    version,
    examples,
    changelog: {
      intro: c.intro.map(text),
      unreleased: { notes: c.unreleased.notes.map(text), entries: c.unreleased.entries.map(entry) },
      releases: c.releases.map((r) => ({ version: r.version, date: r.date, ...(r.lede ? { lede: text(r.lede) } : {}), entries: r.entries.map(entry) })),
    },
    items: plan.items.items.map((i) => ({
      issue: i.issue, title: title(i), state: i.state, category: text(i.category), ...(i.release ? { release: i.release } : {}),
    })),
    groups: groups.map((g) => ({
      ...g,
      rows: g.rows.map((r) => ({
        ...r,
        note: text(r.note),
        issues: r.issues.map((n) => {
          const i = items.get(n);
          return { number: n, title: i ? title(i) : `#${n}`, state: i?.state ?? "open" };
        }),
      })),
    })),
  };
}

// `before` is the CHANGELOG.md an apply started from. If the file changed on
// disk since, someone else edited it, and it isn't overwritten.
function render(plan, todo, before) {
  writeJson(FILES.site, siteData(plan));
  const md = changelogMarkdown(plan.changelog);
  const disk = existsSync(FILES.changelogMd) ? readFileSync(FILES.changelogMd, "utf8") : null;
  if (disk !== md) {
    if (before !== undefined && disk !== before) {
      throw new Error("CHANGELOG.md changed on disk during the apply; fold that edit into plan/changelog.json and rerun");
    }
    writeAtomic(FILES.changelogMd, md);
  }
  if (todo.text != null) {
    for (const item of plan.items.items) {
      const line = todo.parsed.items.get(item.t);
      if (line && line.issue !== item.issue) todo.set({ ...line, issue: item.issue });
    }
  }
  return md;
}

function save(plan, todo, before) {
  plan.items.items.sort((a, b) => tNumber(a.t) - tNumber(b.t));
  writeJson(FILES.items, plan.items);
  writeJson(FILES.gantt, plan.gantt);
  writeJson(FILES.changelog, plan.changelog);
  return render(plan, todo, before);
}

// ---------------------------------------------------------------- check

export function drift(plan, todo, issues, projectItems) {
  const problems = [];
  const warnings = [];
  const say = (s) => problems.push(s);
  const { items } = plan.items;
  const known = new Set(items.map((i) => i.t));
  const todoItems = todo?.text != null ? todo.parsed.items : null;
  const tExists = (t) => known.has(t) || todoItems?.has(t);

  const seenT = new Set(), seenIssue = new Set();
  const labels = new Set(plan.project.labels.map((l) => l.name));
  const versions = new Set(plan.changelog.releases.map((r) => r.version));
  for (const item of items) {
    if (seenT.has(item.t)) say(`${item.t} is recorded twice`);
    if (seenIssue.has(item.issue)) say(`#${item.issue} is recorded for two items`);
    seenT.add(item.t);
    seenIssue.add(item.issue);
    if (!item.title.startsWith(`${item.t}: `)) say(`${item.t}'s title doesn't start with "${item.t}: "`);
    if (!(item.category in plan.project.categories)) say(`${item.t}: category "${item.category}" isn't in plan/project.json`);
    for (const l of item.labels) if (!labels.has(l)) say(`${item.t}: label "${l}" isn't in plan/project.json`);
    if (item.gantt) {
      const found = findRow(plan, item.gantt);
      if (!found) say(`${item.t}: no chart group "${item.gantt.group}"`);
      else if (!found.row) say(`${item.t}: group "${item.gantt.group}" has no row "${item.gantt.row}"`);
      else if (!found.row.issues.includes(item.issue)) say(`${item.t}: row ${item.gantt.group}/${item.gantt.row} doesn't list #${item.issue}`);
    }
    if (item.release && !versions.has(item.release)) say(`${item.t}: release ${item.release} isn't in the changelog`);
    if (item.state === "open" && (item.commit || item.release)) say(`${item.t} is open but names a commit or release`);
  }

  if (todoItems) {
    for (const line of todoItems.values()) {
      const item = byT(plan, line.t);
      if (!item) { say(`TODO.md: ${line.t} has no issue; adopt it with an "add" change`); continue; }
      if (line.issue !== item.issue) say(`TODO.md: ${line.t} names ${line.issue ? `#${line.issue}` : "no issue"}, not #${item.issue}; run render`);
      if (line.done !== (item.state === "closed")) say(`TODO.md: ${line.t} is ${line.done ? "ticked" : "open"} but #${item.issue} is recorded ${item.state}`);
      if (line.category !== item.category) say(`TODO.md: ${line.t} sits under "${line.category}", recorded under "${item.category}"`);
      for (const t of tRefs(line.text)) if (!tExists(t)) say(`TODO.md: ${line.t} names ${t}, which doesn't exist`);
    }
    for (const item of items) if (!todoItems.has(item.t)) say(`${item.t} (#${item.issue}) is recorded but not in TODO.md`);
  } else {
    warnings.push(`no TODO.md at ${todo?.path}; its lines weren't checked`);
  }

  const groupIds = new Set();
  for (const g of plan.gantt.groups) {
    if (groupIds.has(g.id)) say(`chart: group id "${g.id}" twice`);
    groupIds.add(g.id);
    const rowIds = new Set();
    for (const r of g.rows) {
      if (rowIds.has(r.id)) say(`chart: ${g.id} has row id "${r.id}" twice`);
      rowIds.add(r.id);
      for (const t of tRefs(r.note)) if (!tExists(t)) say(`chart: ${g.id}/${r.id} names ${t}, which doesn't exist`);
      for (const n of issueRefs(r.note)) if (!byIssue(plan, n)) say(`chart: ${g.id}/${r.id} names #${n}, which is no item's issue`);
      for (const n of r.issues) {
        const item = byIssue(plan, n);
        if (!item) say(`chart: ${g.id}/${r.id} lists #${n}, which is no item's issue`);
        else if (!item.gantt) say(`chart: ${g.id}/${r.id} lists #${n}, but ${item.t} names no row`);
        else if (r.state === "done" && item.state === "open") warnings.push(`chart: ${g.id}/${r.id} is done, but ${item.t} (#${n}) is open`);
      }
    }
  }
  if (plan.gantt.version !== cargoVersion()) say(`chart: version ${plan.gantt.version}, but the crates are at ${cargoVersion()}`);

  const c = plan.changelog;
  const latest = c.releases[0]?.version;
  if (latest && latest !== cargoVersion()) say(`changelog: latest release ${latest}, but the crates are at ${cargoVersion()}`);
  for (let i = 1; i < c.releases.length; i++) {
    if (cmpVersion(c.releases[i - 1].version, c.releases[i].version) <= 0) say(`changelog: ${c.releases[i - 1].version} isn't newer than ${c.releases[i].version}`);
  }
  const sections = [{ version: null, entries: c.unreleased.entries }, ...c.releases];
  for (const { version, entries } of sections) {
    const tracked = version === null || cmpVersion(version, c.tracked_from) >= 0;
    const name = version ?? "Unreleased";
    for (const e of entries) {
      if (tracked && !e.issues.length) say(`changelog: an entry of ${name} names no issue: "${e.text.slice(0, 60)}…"`);
      for (const t of tRefs(e.text)) if (!tExists(t)) say(`changelog: ${name} names ${t}, which doesn't exist`);
      for (const n of e.issues) {
        const item = byIssue(plan, n);
        if (!item) say(`changelog: ${name} names #${n}, which is no item's issue`);
        else if (tracked && item.release !== version) say(`changelog: ${name} names ${item.t} (#${n}), recorded as shipped in ${item.release ?? "nothing yet"}`);
      }
      for (const n of e.refs ?? []) if (!byIssue(plan, n)) say(`changelog: ${name} refers to #${n}, which is no item's issue`);
    }
  }
  const md = existsSync(FILES.changelogMd) ? readFileSync(FILES.changelogMd, "utf8") : "";
  if (md !== changelogMarkdown(c)) say("CHANGELOG.md differs from plan/changelog.json; change the JSON and run render");
  const site = existsSync(FILES.site) ? readFileSync(FILES.site, "utf8") : "";
  if (site !== format(siteData(plan)) + "\n") say("site/src/lib/todo.json differs from the records; run render");

  if (issues) {
    const mine = new Set(labels);
    for (const item of items) {
      const issue = issues.get(item.issue);
      if (!issue) { say(`#${item.issue} (${item.t}) doesn't exist on GitHub`); continue; }
      if (issue.title !== item.title) say(`#${item.issue}: GitHub's title is "${issue.title}", recorded "${item.title}"`);
      if (issue.state !== item.state) say(`#${item.issue} (${item.t}) is ${issue.state} on GitHub, recorded ${item.state}`);
      const have = issue.labels.filter((l) => mine.has(l)).sort().join(", ");
      if (have !== [...item.labels].sort().join(", ")) say(`#${item.issue}: GitHub's labels are [${have}], recorded [${[...item.labels].sort().join(", ")}]`);
      const milestone = item.release ? milestoneOf(item.release) : null;
      if (milestone && issue.milestone !== milestone) say(`#${item.issue} (${item.t}) shipped in ${item.release} but its milestone is ${issue.milestone ?? "none"}; run milestones`);
    }
    for (const issue of issues.values()) {
      const t = /^(T\d{3,}): /.exec(issue.title)?.[1];
      if (t && !byIssue(plan, issue.number)) say(`#${issue.number} "${issue.title}" is on GitHub but not recorded`);
    }
  }
  if (projectItems) {
    for (const item of items) {
      const entry = projectItems.get(item.issue);
      if (!entry) { say(`#${item.issue} (${item.t}) isn't in the Project; run sync`); continue; }
      if (fieldMutations(plan, item, entry.id, entry.values, "").length) say(`#${item.issue} (${item.t}): the Project's fields differ; run sync`);
    }
  }
  return { problems, warnings };
}

// ---------------------------------------------------------------- apply

const fail = (message) => {
  throw new Error(message);
};

class Context {
  constructor(plan, todo, issues, dry, saveChange) {
    Object.assign(this, { plan, todo, issues, dry, saveChange, fake: 900000, synced: new Set() });
    this.projectItems = null;
  }
  get repo() {
    return this.plan.project.repo;
  }
  issueByT(t) {
    return [...this.issues.values()].find((i) => i.title.startsWith(`${t}: `));
  }
  labelIds(names) {
    return names.map((n) => this.plan.project.labels.find((l) => l.name === n)?.id ?? fail(`label "${n}" has no id; run setup`));
  }
  createIssue(title, body, labels) {
    if (this.dry) return ++this.fake;
    const { project, repoId } = this.plan.project;
    const data = graphql(`mutation($input: CreateIssueInput!) { createIssue(input: $input) { issue { number id state
      projectItems(first: 10) { nodes { id project { id } } } } } }`,
    { input: { repositoryId: repoId, title, body, labelIds: this.labelIds(labels), projectV2Ids: [project.id] } });
    const issue = data.createIssue.issue;
    const entry = issue.projectItems.nodes.find((n) => n.project.id === project.id);
    this.issues.set(issue.number, { number: issue.number, title, state: "open", labels, node_id: issue.id });
    if (entry) (this.projectItems ??= new Map()).set(issue.number, { id: entry.id, values: new Map() });
    return issue.number;
  }
  comment(number, body) {
    if (!this.dry) rest("POST", `repos/${this.repo}/issues/${number}/comments`, { body });
  }
  milestone(number, title) {
    if (this.dry) return;
    this.milestones ??= fetchMilestones(this.repo);
    setMilestone(this.repo, this.milestones, number, title);
    this.issues.get(number).milestone = title;
  }
  close(number) {
    if (this.dry) return;
    rest("PATCH", `repos/${this.repo}/issues/${number}`, { state: "closed", state_reason: "completed" });
    this.issues.get(number).state = "closed";
  }
  patch(number, fields) {
    if (this.dry) return;
    rest("PATCH", `repos/${this.repo}/issues/${number}`, fields);
    Object.assign(this.issues.get(number), fields);
  }
  sync(item) {
    this.synced.add(item.t);
  }
  flushSync() {
    if (this.dry || !this.synced.size || !this.plan.project.project.id) return;
    const items = [...this.synced].map((t) => byT(this.plan, t)).filter(Boolean);
    this.projectItems ??= fetchProjectItems(this.plan.project);
    syncItems(this.plan, items, this.projectItems);
    this.synced.clear();
  }
}

function entryRefs(ctx, refs = []) {
  for (const n of refs) {
    const item = byIssue(ctx.plan, n) ?? fail(`#${n} is no item's issue`);
    if (item.state === "closed") fail(`${item.t} (#${n}) is closed; name it in "issues" if the entry ships it`);
  }
}

function checkRefs(ctx, text, self) {
  const todoItems = ctx.todo.text != null ? ctx.todo.parsed.items : new Map();
  for (const t of tRefs(text)) if (t !== self && !byT(ctx.plan, t) && !todoItems.has(t)) fail(`the text names ${t}, which doesn't exist`);
}

// A chart note names issues as #N, each an item's.
function checkNote(ctx, text) {
  checkRefs(ctx, text);
  for (const n of issueRefs(text)) if (!byIssue(ctx.plan, n)) fail(`the note names #${n}, which is no item's issue`);
}

function itemFor(ctx, op) {
  const item = byT(ctx.plan, op.t) ?? fail(`${op.t} isn't recorded`);
  if (item.issue !== op.issue) fail(`${op.t} is #${item.issue}, not #${op.issue}`);
  return item;
}

function labelsChecked(ctx, labels) {
  for (const l of labels) if (!ctx.plan.project.labels.some((x) => x.name === l)) fail(`unknown label "${l}"`);
  return labels;
}

function rowChecked(ctx, ref) {
  if (!ref) return null;
  const found = findRow(ctx.plan, ref);
  if (!found) fail(`no chart group "${ref.group}"`);
  if (!found.row) fail(`group "${ref.group}" has no row "${ref.row}"; its rows are ${found.group.rows.map((r) => r.id).join(", ")}`);
  return found.row;
}

function setRow(ctx, item, ref) {
  const old = item.gantt && findRow(ctx.plan, item.gantt)?.row;
  if (old) old.issues = old.issues.filter((n) => n !== item.issue);
  item.gantt = ref ?? null;
  const row = rowChecked(ctx, ref);
  if (row && !row.issues.includes(item.issue)) row.issues.push(item.issue);
  ctx.plan.gantt.updated = today();
}

// A row's issues changed: an item takes the row as its own if it had none,
// and loses it if the row no longer lists it.
function rowIssues(ctx, group, row, issues) {
  for (const n of issues) if (!byIssue(ctx.plan, n)) fail(`#${n} is no item's issue`);
  const before = row.issues;
  row.issues = issues;
  for (const n of issues) {
    const item = byIssue(ctx.plan, n);
    if (!item.gantt) { item.gantt = { group: group.id, row: row.id }; ctx.sync(item); }
  }
  for (const n of before.filter((n) => !issues.includes(n))) {
    const item = byIssue(ctx.plan, n);
    if (item.gantt?.group === group.id && item.gantt.row === row.id) {
      const other = ctx.plan.gantt.groups.flatMap((g) => g.rows.map((r) => ({ g, r }))).find(({ r }) => r.issues.includes(n));
      item.gantt = other ? { group: other.g.id, row: other.r.id } : null;
      ctx.sync(item);
    }
  }
}

function addItem(ctx, op, { category, text, title, labels, decisions, gantt, release, t }) {
  const { plan, todo } = ctx;
  const line = t ? todo.parsed.items.get(t) : null;
  if (t && byT(plan, t)) fail(`${t} is already recorded as #${byT(plan, t).issue}`);
  if (t && !line) fail(`${t} isn't in TODO.md; leave out "t" to add a new item`);
  if (line?.issue) fail(`${t} already names #${line.issue} in TODO.md`);
  if (line && text) fail(`${t} keeps its wording from TODO.md; leave out "text", or edit it afterwards`);
  if (line && category && category !== line.category) fail(`${t} sits under "${line.category}", not "${category}"`);
  text ??= line?.text ?? fail(`a new item needs "text"`);
  category ??= line?.category ?? fail(`a new item needs "category"`);
  if (!todo.parsed.sections.has(category)) fail(`"${category}" isn't a heading of TODO.md: ${[...todo.parsed.sections.keys()].join("; ")}`);
  if (!(category in plan.project.categories)) fail(`"${category}" has no label in plan/project.json`);
  publicText(text);
  if (!t) {
    const numbers = [...todo.parsed.items.keys(), ...plan.items.items.map((i) => i.t)].map(tNumber);
    t = `T${String(Math.max(0, ...numbers) + 1).padStart(3, "0")}`;
    if (op.op === "add") {
      op.t = t;
      ctx.saveChange();
    }
  }
  checkRefs(ctx, text, t);
  const done = line?.done ?? false;
  if (release && !done) fail(`"release" is only for adopting a done line`);
  if (release && !plan.changelog.releases.some((r) => r.version === release)) fail(`release ${release} isn't in the changelog`);
  labels = labelsChecked(ctx, labels ?? labelsFor(plan.project, category, text));
  rowChecked(ctx, gantt);
  const fullTitle = `${t}: ${title ?? shortTitle(publicText(text))}`;

  let number;
  const existing = ctx.issueByT(t);
  if (op.issue === "new" || op.op === "changelog") {
    if (existing) number = existing.number; // a rerun after the issue was made
    else number = ctx.createIssue(fullTitle, issueBody(t, category, text), labels);
  } else {
    number = op.issue;
    const issue = ctx.issues.get(number) ?? fail(`#${number} doesn't exist`);
    if (byIssue(plan, number)) fail(`#${number} is already ${byIssue(plan, number).t}'s issue`);
    const keep = issue.labels.filter((l) => !plan.project.labels.some((x) => x.name === l));
    ctx.patch(number, { title: fullTitle, labels: [...keep, ...labels] });
  }
  const commits = done ? doneCommits(text) : [];
  if (done && ctx.issues.get(number)?.state !== "closed") {
    const said = commits.length ? `Done in ${commits.join(" and ")}.` : "Done. The description says how it was settled.";
    ctx.comment(number, said);
    ctx.close(number);
    if (release) ctx.milestone(number, milestoneOf(release));
  }
  const item = {
    t, issue: number, title: fullTitle, state: done ? "closed" : "open", category, labels,
    gantt: null, decisions: decisions ?? dRefs(text), commit: commits[0] ?? null, release: release ?? null,
  };
  plan.items.items.push(item);
  if (gantt) setRow(ctx, item, gantt);
  if (line) todo.set({ ...line, issue: number });
  else todo.append(category, { done: false, t, text, issue: number });
  ctx.sync(item);
  return item;
}

const OPS = {
  add(ctx, op) {
    const item = addItem(ctx, op, op);
    op.issue = item.issue;
    return `${item.t} is #${item.issue}: ${item.title}`;
  },

  close(ctx, op) {
    const item = itemFor(ctx, op);
    if (item.state === "closed") fail(`${op.t} is already closed`);
    try { git("cat-file", "-e", `${op.commit}^{commit}`); } catch { fail(`commit ${op.commit} isn't in this repository`); }
    const line = ctx.todo.parsed.items.get(op.t) ?? fail(`${op.t} isn't in TODO.md`);
    // "Done in `abc` (D640)." as the file's other done items say it.
    const said = (commit) => (op.note?.startsWith("(") ? `Done in ${commit} ${op.note}.` : `Done in ${commit}.${op.note ? ` ${op.note}` : ""}`);
    ctx.comment(item.issue, op.comment ? `${said(op.commit)}\n\n${publicText(op.comment)}` : said(op.commit));
    ctx.close(item.issue);
    ctx.todo.set({ ...line, done: true, text: `${said(`\`${op.commit}\``)} ${line.text}` });
    Object.assign(item, { state: "closed", commit: op.commit });
    ctx.sync(item);
    return `${item.t} (#${item.issue}) closed in ${op.commit}`;
  },

  edit(ctx, op) {
    const item = itemFor(ctx, op);
    const fields = {};
    if (op.title) fields.title = item.title = `${item.t}: ${op.title}`;
    if (op.text) {
      checkRefs(ctx, op.text, item.t);
      const line = ctx.todo.parsed.items.get(op.t) ?? fail(`${op.t} isn't in TODO.md`);
      ctx.todo.set({ ...line, text: op.text });
      fields.body = issueBody(item.t, item.category, op.text);
    }
    if (op.labels) {
      item.labels = labelsChecked(ctx, op.labels);
      const keep = (ctx.issues.get(item.issue)?.labels ?? []).filter((l) => !ctx.plan.project.labels.some((x) => x.name === l));
      fields.labels = [...keep, ...op.labels];
    }
    if (op.decisions) item.decisions = op.decisions;
    if ("gantt" in op) setRow(ctx, item, op.gantt);
    if (Object.keys(fields).length) ctx.patch(item.issue, fields);
    ctx.sync(item);
    return `${item.t} (#${item.issue}) edited`;
  },

  row(ctx, op) {
    const { group, row } = findRow(ctx.plan, op) ?? fail(`no chart group "${op.group}"`);
    if (!row) fail(`group "${op.group}" has no row "${op.row}"`);
    for (const key of ["name", "start", "size", "state", "note", "milestone"]) if (key in op) row[key] = op[key];
    if ("note" in op) checkNote(ctx, op.note);
    if (op.issues) rowIssues(ctx, group, row, op.issues);
    for (const item of ctx.plan.items.items) if (item.gantt?.group === group.id && item.gantt.row === row.id) ctx.sync(item);
    ctx.plan.gantt.updated = today();
    return `row ${group.id}/${row.id} changed`;
  },

  "add-row"(ctx, op) {
    const group = ctx.plan.gantt.groups.find((g) => g.id === op.group) ?? fail(`no chart group "${op.group}"`);
    if (group.rows.some((r) => r.id === op.row.id)) fail(`${op.group} already has a row "${op.row.id}"`);
    checkNote(ctx, op.row.note);
    const at = op.after ? group.rows.findIndex((r) => r.id === op.after) + 1 || fail(`no row "${op.after}"`) : group.rows.length;
    const row = { ...op.row, issues: [] };
    group.rows.splice(at, 0, row);
    rowIssues(ctx, group, row, op.row.issues);
    ctx.plan.gantt.updated = today();
    return `row ${group.id}/${row.id} added`;
  },

  "remove-row"(ctx, op) {
    const { group, row } = findRow(ctx.plan, op) ?? fail(`no chart group "${op.group}"`);
    if (!row) fail(`group "${op.group}" has no row "${op.row}"`);
    rowIssues(ctx, group, row, []);
    group.rows = group.rows.filter((r) => r !== row);
    ctx.plan.gantt.updated = today();
    return `row ${group.id}/${row.id} removed`;
  },

  group(ctx, op) {
    const group = ctx.plan.gantt.groups.find((g) => g.id === op.id) ?? fail(`no chart group "${op.id}"`);
    for (const key of ["title", "subtitle", "open"]) if (key in op) group[key] = op[key];
    for (const item of ctx.plan.items.items) if (item.gantt?.group === group.id) ctx.sync(item);
    ctx.plan.gantt.updated = today();
    return `group ${group.id} changed`;
  },

  "add-group"(ctx, op) {
    const groups = ctx.plan.gantt.groups;
    if (groups.some((g) => g.id === op.group.id)) fail(`a group "${op.group.id}" exists`);
    const at = op.after ? groups.findIndex((g) => g.id === op.after) + 1 || fail(`no group "${op.after}"`) : groups.length;
    const group = { ...op.group, rows: [] };
    groups.splice(at, 0, group);
    for (const row of op.group.rows) {
      checkNote(ctx, row.note);
      const copy = { ...row, issues: [] };
      group.rows.push(copy);
      rowIssues(ctx, group, copy, row.issues);
    }
    ctx.plan.gantt.updated = today();
    return `group ${group.id} added`;
  },

  "remove-group"(ctx, op) {
    const group = ctx.plan.gantt.groups.find((g) => g.id === op.id) ?? fail(`no chart group "${op.id}"`);
    if (group.rows.length) fail(`group ${op.id} still has rows; remove or move them first`);
    ctx.plan.gantt.groups = ctx.plan.gantt.groups.filter((g) => g !== group);
    return `group ${op.id} removed`;
  },

  meta(ctx, op) {
    for (const key of ["updated", "version", "examples"]) if (key in op) ctx.plan.gantt[key] = op[key];
    if (!op.updated) ctx.plan.gantt.updated = today();
    return "chart header changed";
  },

  changelog(ctx, op) {
    checkRefs(ctx, op.text);
    const issues = [];
    for (const ref of op.issues) {
      if (ref === "new") {
        if (!op.category) fail(`an entry with a "new" issue needs "category", the TODO.md heading its item goes under`);
        const item = addItem(ctx, op, { category: op.category, text: op.text, title: op.title });
        issues.push(item.issue);
        op.issues = op.issues.map((r) => (r === "new" ? item.issue : r));
        ctx.saveChange();
      } else {
        const item = byIssue(ctx.plan, ref) ?? fail(`#${ref} is no item's issue`);
        if (item.release) fail(`${item.t} (#${ref}) already shipped in ${item.release}`);
        issues.push(ref);
      }
    }
    entryRefs(ctx, op.refs);
    ctx.plan.changelog.unreleased.entries.push({ text: op.text, issues, ...(op.refs?.length ? { refs: op.refs } : {}) });
    return `entry for ${issues.map((n) => `#${n}`).join(", ")} added under Unreleased`;
  },

  entry(ctx, op) {
    const entries = ctx.plan.changelog.unreleased.entries;
    const entry = entries[op.index] ?? fail(`Unreleased has ${entries.length} entries; there is no entry ${op.index}`);
    if (op.remove) {
      entries.splice(op.index, 1);
      return `entry ${op.index} removed from Unreleased`;
    }
    if (op.text) {
      checkRefs(ctx, op.text);
      entry.text = op.text;
    }
    if (op.issues) {
      for (const n of op.issues) {
        const item = byIssue(ctx.plan, n) ?? fail(`#${n} is no item's issue`);
        if (item.release) fail(`${item.t} (#${n}) already shipped in ${item.release}`);
      }
      entry.issues = op.issues;
    }
    if (op.refs) {
      entryRefs(ctx, op.refs);
      if (op.refs.length) entry.refs = op.refs;
      else delete entry.refs;
    }
    return `entry ${op.index} under Unreleased changed`;
  },

  note(ctx, op) {
    const notes = ctx.plan.changelog.unreleased.notes;
    if (op.remove) {
      if (!notes.includes(op.text)) fail(`no such note under Unreleased`);
      ctx.plan.changelog.unreleased.notes = notes.filter((n) => n !== op.text);
    } else notes.push(op.text);
    return op.remove ? "note removed" : "note added";
  },

  release(ctx, op) {
    const c = ctx.plan.changelog;
    const entries = c.unreleased.entries;
    if (!entries.length) fail("nothing is unreleased");
    const current = cargoVersion();
    const [a, b, p] = current.split(".").map(Number);
    const version = op.version ?? (op.bump === false ? current : `${a}.${b}.${p + 1}`);
    if (c.releases.some((r) => r.version === version)) fail(`${version} is already released`);
    if (op.bump === false && version !== current) fail(`the crates are at ${current}, not ${version}`);
    if (op.bump !== false && cmpVersion(version, current) <= 0) fail(`${version} isn't newer than the crates' ${current}`);
    const date = op.date ?? today();
    if (!ctx.dry && op.bump !== false) {
      execFileSync(join(ROOT, "scripts/bump-version.sh"), [version], { cwd: ROOT, stdio: "inherit" });
    }
    for (const n of new Set(entries.flatMap((e) => e.issues))) {
      const item = byIssue(ctx.plan, n);
      ctx.milestone(n, milestoneOf(version));
      if (item.state === "open") {
        ctx.close(n);
        const line = ctx.todo.parsed.items.get(item.t);
        if (line) ctx.todo.set({ ...line, done: true, text: `Done in ${version}. ${line.text}` });
        item.state = "closed";
      }
      item.release = version;
      ctx.sync(item);
    }
    c.releases.unshift({ version, date, ...(op.lede ? { lede: op.lede } : {}), entries });
    c.unreleased.entries = [];
    Object.assign(ctx.plan.gantt, { version, updated: date });
    return `released ${version} with ${entries.length} entries`;
  },
};

function apply(file, dry) {
  const change = readJson(file);
  const errors = check("change", change);
  if (errors.length) return { errors };
  const { plan, errors: planErrors } = load();
  if (planErrors.length) return { errors: planErrors };
  const pending = change.changes.filter((c) => !c.applied);
  // Outputs edited by hand would be overwritten: fold them into the JSON first.
  if (existsSync(FILES.changelogMd) && readFileSync(FILES.changelogMd, "utf8") !== changelogMarkdown(plan.changelog)) {
    return { errors: ["CHANGELOG.md has edits plan/changelog.json lacks; move them into the JSON with a change file first"] };
  }
  const issues = fetchIssues(plan.project.repo);

  // A dry run first, on copies, so a bad change stops before anything is written.
  const dryCtx = new Context(structuredClone(plan), new Todo(todoPath(), true), structuredClone(issues), true, () => {});
  const dryOps = structuredClone(pending);
  for (const [i, op] of dryOps.entries()) {
    try {
      console.log(`${dry ? "would: " : "check: "}${OPS[op.op](dryCtx, op)}`);
    } catch (error) {
      return { errors: [`change ${change.changes.indexOf(pending[i])} (${op.op}): ${error.message}`] };
    }
  }
  if (dry) return { errors: [] };

  const todo = new Todo(todoPath());
  const saveChange = () => writeJson(file, change);
  const ctx = new Context(plan, todo, issues, false, saveChange);
  let before = changelogMarkdown(plan.changelog);
  for (const op of pending) {
    console.log(OPS[op.op](ctx, op));
    op.applied = true;
    before = save(plan, todo, before);
    saveChange();
    try {
      ctx.flushSync();
    } catch (error) {
      ctx.synced.clear();
      console.error(`note: the Project's fields weren't set (${error.message.split("\n")[0]}); run sync`);
    }
  }
  return { errors: [] };
}

// ---------------------------------------------------------------- setup

function setup() {
  const project = readJson(FILES.project);
  const [owner, name] = project.repo.split("/");
  const existing = new Map(JSON.parse(gh(["api", "--paginate", `repos/${project.repo}/labels?per_page=100`, "--jq", "[.[] | {name, color, description, node_id}]"]).replace(/\]\s*\[/g, ",")).map((l) => [l.name, l]));
  for (const label of project.labels) {
    const have = existing.get(label.name);
    let node;
    if (!have) node = rest("POST", `repos/${project.repo}/labels`, { name: label.name, color: label.color, description: label.description });
    else if (have.color !== label.color || have.description !== label.description) {
      node = rest("PATCH", `repos/${project.repo}/labels/${encodeURIComponent(label.name)}`, { color: label.color, description: label.description });
    } else node = have;
    label.id = node.node_id;
  }
  const ids = graphql(`query { repository(owner: ${q(owner)}, name: ${q(name)}) { id } user(login: ${q(project.project.owner)}) { id } }`);
  project.repoId = ids.repository.id;
  if (!project.project.id) {
    const made = graphql(`mutation { createProjectV2(input: { ownerId: ${q(ids.user.id)}, title: ${q(project.project.title)}, repositoryId: ${q(project.repoId)} }) { projectV2 { id number url } } }`).createProjectV2.projectV2;
    Object.assign(project.project, { id: made.id, number: made.number, url: made.url });
    graphql(`mutation { updateProjectV2(input: { projectId: ${q(made.id)}, shortDescription: ${q(project.project.description ?? "")} }) { clientMutationId } }`);
  }
  const wanted = {
    status: { name: "Status" },
    category: { name: "Category", options: Object.keys(project.categories) },
    t: { name: "T number" },
    gantt: { name: "Chart row" },
    decisions: { name: "Decisions" },
    commit: { name: "Commit" },
    release: { name: "Shipped in" },
  };
  const read = () => graphql(`query { node(id: ${q(project.project.id)}) { ... on ProjectV2 { fields(first: 50) { nodes {
    ... on ProjectV2FieldCommon { id name }
    ... on ProjectV2SingleSelectField { options { id name color description } } } } } } }`).node.fields.nodes;
  let fields = read();
  project.fields = {};
  for (const [role, want] of Object.entries(wanted)) {
    let field = fields.find((f) => f.name === want.name);
    if (!field) {
      const options = want.options?.map((n) => `{ name: ${q(n)}, color: GRAY, description: "" }`);
      graphql(`mutation { createProjectV2Field(input: { projectId: ${q(project.project.id)}, name: ${q(want.name)}, dataType: ${options ? "SINGLE_SELECT" : "TEXT"}${options ? `, singleSelectOptions: [${options.join(", ")}]` : ""} }) { clientMutationId } }`);
      fields = read();
      field = fields.find((f) => f.name === want.name);
    } else if (want.options && want.options.some((n) => !field.options.some((o) => o.name === n))) {
      const keep = field.options.map((o) => `{ id: ${q(o.id)}, name: ${q(o.name)}, color: ${o.color}, description: ${q(o.description)} }`);
      const add = want.options.filter((n) => !field.options.some((o) => o.name === n)).map((n) => `{ name: ${q(n)}, color: GRAY, description: "" }`);
      graphql(`mutation { updateProjectV2Field(input: { fieldId: ${q(field.id)}, singleSelectOptions: [${[...keep, ...add].join(", ")}] }) { clientMutationId } }`);
      fields = read();
      field = fields.find((f) => f.name === want.name);
    }
    project.fields[role] = { id: field.id, name: field.name, ...(field.options ? { options: Object.fromEntries(field.options.map((o) => [o.name, o.id])) } : {}) };
  }
  writeJson(FILES.project, project);
  console.log(`labels: ${project.labels.map((l) => l.name).join(", ")}`);
  console.log(`project: ${project.project.url}`);
}

// ---------------------------------------------------------------- main

function main([command, ...args]) {
  const offline = args.includes("--offline");
  if (command === "setup") return setup();
  if (command === "apply") {
    const file = args.find((a) => !a.startsWith("--")) ?? fail("apply needs a change file");
    const { errors } = apply(resolve(file), args.includes("--dry-run"));
    for (const e of errors) console.error(`error: ${e}`);
    return errors.length ? 1 : 0;
  }
  const { plan, errors } = load();
  if (errors.length) {
    for (const e of errors) console.error(`error: ${e}`);
    return 1;
  }
  const todo = new Todo(todoPath());
  if (command === "render") {
    render(plan, todo);
    console.log("render: wrote site/src/lib/todo.json, CHANGELOG.md and TODO.md's issue numbers");
    return 0;
  }
  if (command === "milestones") {
    const issues = fetchIssues(plan.project.repo);
    const milestones = fetchMilestones(plan.project.repo);
    let n = 0;
    for (const item of plan.items.items) {
      if (!item.release || issues.get(item.issue)?.milestone === milestoneOf(item.release)) continue;
      setMilestone(plan.project.repo, milestones, item.issue, milestoneOf(item.release));
      n++;
    }
    console.log(`milestones: put ${n} shipped issues in their milestones`);
    return 0;
  }
  if (command === "sync") {
    syncItems(plan, plan.items.items, fetchProjectItems(plan.project));
    console.log(`sync: ${plan.items.items.length} items`);
    return 0;
  }
  if (command === "check") {
    const issues = offline ? null : fetchIssues(plan.project.repo);
    const projectItems = offline || !plan.project.project.id ? null : fetchProjectItems(plan.project);
    const { problems, warnings } = drift(plan, todo, issues, projectItems);
    for (const w of warnings) console.log(`note: ${w}`);
    for (const p of problems) console.log(`drift: ${p}`);
    const open = plan.items.items.filter((i) => i.state === "open").length;
    const summary = `${plan.items.items.length} items (${open} open, ${plan.items.items.length - open} closed), ` +
      `${plan.gantt.groups.reduce((n, g) => n + g.rows.length, 0)} chart rows, ${plan.changelog.releases.length} releases` +
      (offline ? ", GitHub not checked" : ", GitHub and the Project checked");
    console.log(problems.length ? `check: ${problems.length} problems; ${summary}` : `check: no drift; ${summary}`);
    return problems.length ? 1 : 0;
  }
  console.error("usage: node plan/plan.mjs check [--offline] | apply <change.json> [--dry-run] | render | sync | milestones | setup");
  return 2;
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  try {
    process.exitCode = main(process.argv.slice(2));
  } catch (error) {
    console.error(`error: ${error.message}`);
    process.exitCode = 1;
  }
}
