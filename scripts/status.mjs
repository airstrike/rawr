#!/usr/bin/env node
// One-shot status of rawr's work: open items, what still gates the current
// minor series, and the version each surface runs.
//
//   node scripts/status.mjs
//
// The gating rows are the chart rows D739 names for 0.3: every iced example
// (the "no-glue" group) and all rustlings code (the "rustlings" row).

import { execFileSync } from "node:child_process";
import { readFileSync } from "node:fs";

const read = (path) => JSON.parse(readFileSync(new URL(`../${path}`, import.meta.url)));
const run = (command, args) => {
  try {
    return execFileSync(command, args, { encoding: "utf8", stdio: ["ignore", "pipe", "ignore"], timeout: 30_000 }).trim();
  } catch {
    return null;
  }
};
const fetchText = async (url) => {
  try {
    const response = await fetch(url, { signal: AbortSignal.timeout(15_000) });
    return response.ok ? await response.text() : null;
  } catch {
    return null;
  }
};

const items = read("plan/items.json").items;
const gantt = read("plan/gantt.json");
const byIssue = new Map(items.map((item) => [item.issue, item]));
// A retired item was closed as not planned: it is not open.
const open = items.filter((item) => item.state === "open").length;
const retired = items.filter((item) => item.state === "retired").length;
const percent = ((100 * open) / items.length).toFixed(1);

const repoVersion = /^version = "([^"]+)"/m.exec(readFileSync(new URL("../Cargo.toml", import.meta.url), "utf8"))?.[1];
const minor = repoVersion?.split(".").slice(0, 2).join(".");

console.log(`Items: ${open} open of ${items.length} (${percent}% open${retired ? `, ${retired} retired` : ""})`);
console.log("");

// What gates the minor series (D739).
const gating = [];
for (const group of gantt.groups) {
  for (const row of group.rows) {
    if (row.state === "done") continue;
    if (group.id !== "no-glue" && row.id !== "rustlings") continue;
    const left = row.issues.filter((n) => byIssue.get(n)?.state === "open");
    if (left.length) gating.push({ group, row, left });
  }
}
const { running, total } = gantt.examples;
console.log(`Gating ${minor}.0 (D739: every iced example runs, all rustlings code works)`);
console.log(`  iced examples running: ${running} of ${total} (chart, as of: ${gantt.examples.note})`);
for (const { row, left } of gating) {
  console.log(`  ${row.name}: ${left.length} open (${row.state})`);
  for (const n of left.slice(0, 5)) console.log(`    #${n} ${byIssue.get(n).title.replace(/^T\d+: /, "").slice(0, 90)}`);
  if (left.length > 5) console.log(`    and ${left.length - 5} more`);
}
console.log("");

// The version each surface runs.
const subjectOf = (sha) => run("git", ["log", "-1", "--format=%s", sha]);
const releaseOf = (sha) => {
  const described = run("git", ["log", "-1", "--format=%h %s", sha]);
  const version = /release (\d+\.\d+\.\d+)/.exec(subjectOf(sha) ?? "")?.[1];
  return version ?? described ?? sha;
};
const tool = (name, args = ["--version"]) => run(name, args)?.split("\n")[0] ?? "not installed";
const driver = tool("rawr-import-driver");
const driverProtocol = driver.split(" ")[2];
const mainProtocol = /const PROTOCOL_SCHEMA: u32 = (\d+);/.exec(readFileSync(new URL("../importer/src/driver.rs", import.meta.url), "utf8"))?.[1];

const todo = await fetchText("https://rawr.airstrike.fyi/todo");
// The header's meta block: "Updated <b>date</b> ... <b>version</b>".
const meta = todo?.slice(todo.indexOf('class="meta"'));
const workplan = meta ? /<b>(\d+\.\d+\.\d+)<\/b>/.exec(meta)?.[1] : null;

const health = await fetchText("https://rawr.airstrike.fyi/bot/health");
const image = health ? JSON.parse(health).deployed : null;
const bot = image ? `${releaseOf(image)} (image ${image}${JSON.parse(health).running ? ", running" : ", not running"})` : "unreachable";

// rawr-web reaches the playground only through a release run's site stage,
// whose deploy step deploys first and checks after: a run whose step took
// this run's rawr-web and started deploying deployed it, even when the
// checks after it failed or timed out.
let playground = "unknown";
const runs = run("gh", ["run", "list", "--workflow", "release.yml", "-L", "20", "--json", "databaseId,headSha"]);
for (const entry of runs ? JSON.parse(runs) : []) {
  const steps = run("gh", ["run", "view", String(entry.databaseId), "--json", "jobs", "-q", '.jobs[]|select(.name=="site / site")|.steps[]|.name+"="+.conclusion']);
  const step = (name) => steps?.split("\n").find((line) => line.startsWith(`${name}=`))?.split("=")[1];
  const took = step("Use this run's rawr-web") === "success";
  const deploy = step("Test, deploy and check the site");
  if (took && deploy && deploy !== "skipped") {
    playground = `${releaseOf(entry.headSha)}${deploy === "success" ? "" : ` (deployed; its checks ended ${deploy})`}`;
    break;
  }
}

console.log("Versions");
console.log(`  repo (main):  ${repoVersion}`);
console.log(`  local tools:  ${tool("rawr")}, ${tool("rawr-import")}, driver protocol ${driverProtocol}${driverProtocol === mainProtocol ? "" : ` (main is ${mainProtocol}: reinstall the driver)`}`);
console.log(`  work plan:    ${workplan ?? "unreachable"}`);
console.log(`  playground:   ${playground} (last release run whose site stage deployed rawr-web)`);
console.log(`  bot:          ${bot}`);
