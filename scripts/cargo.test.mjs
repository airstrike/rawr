// node --test scripts/cargo.test.mjs
//
// The prune of scripts/cargo.mjs over a fake target directory, and the
// rewrites that send cargo through it.

import assert from "node:assert/strict";
import fs from "node:fs";
import os from "node:os";
import { join } from "node:path";
import test from "node:test";

import { messageFormat, prune, rewrite } from "./cargo.mjs";

const WORKSPACE = "aaaaaaaaaaaaaaaa";
const DEPENDENCIES = "bbbbbbbbbbbbbbbb";

// A target directory whose `debug` profile holds `units`, each a crate and a
// hash with a library, a dep-info file and a fingerprint, and whose records
// are `records`, each naming hashes of the current generations.
function target(units, records, { rawr = false, runners = [] } = {}) {
  const root = fs.mkdtempSync(join(os.tmpdir(), "cargo-prune-"));
  const profile = join(root, "debug");
  for (const directory of ["deps", ".fingerprint", "build", "examples", "incremental"]) {
    fs.mkdirSync(join(profile, directory), { recursive: true });
  }
  for (const [name, hash] of units) {
    fs.writeFileSync(join(profile, "deps", `lib${name}-${hash}.rlib`), "");
    fs.writeFileSync(join(profile, "deps", `${name}-${hash}.d`), "");
    fs.mkdirSync(join(profile, ".fingerprint", `${name.replaceAll("_", "-")}-${hash}`));
  }
  const state = join(root, ".rawr-builds");
  const directory = join(state, "debug");
  fs.mkdirSync(directory, { recursive: true });
  records.forEach((units, at) => {
    const entry = { key: String(at), workspace: WORKSPACE, dependencies: DEPENDENCIES, units, crates: ["rawr", "rawr_gui"], incremental: {} };
    fs.writeFileSync(join(directory, `${at}.json`), JSON.stringify(entry));
  });
  fs.writeFileSync(join(directory, "current"), JSON.stringify({ workspace: WORKSPACE, dependencies: DEPENDENCIES }));
  if (rawr) {
    fs.mkdirSync(join(root, "rawr"));
    fs.mkdirSync(join(root, "rawr-units"));
    const record = { version: 2, workspace: root, named: runners, made: runners };
    fs.writeFileSync(join(root, "rawr-units", "runner.json"), JSON.stringify(record));
  }
  return { root, state, profile };
}

const left = (profile) => fs.readdirSync(join(profile, "deps")).sort();

test("a workspace unit no current record names goes; named ones and dependencies stay", () => {
  const units = [
    ["rawr", "1111111111111111"],
    ["rawr", "2222222222222222"],
    ["rawr_gui", "3333333333333333"],
    ["libc", "4444444444444444"],
  ];
  const { root, state, profile } = target(units, [["1111111111111111", "4444444444444444"]]);
  prune(state, profile);
  assert.deepEqual(left(profile), [
    "libc-4444444444444444.d",
    "liblibc-4444444444444444.rlib",
    "librawr-1111111111111111.rlib",
    "rawr-1111111111111111.d",
  ]);
  assert.deepEqual(fs.readdirSync(join(profile, ".fingerprint")).sort(), ["libc-4444444444444444", "rawr-1111111111111111"]);
  fs.rmSync(root, { recursive: true });
});

// A target rawr also builds into used to keep every unit no wrapped run had
// recorded, so a plain cargo's builds of the workspace's crates stayed for
// good (#2314).
test("beside rawr's directory, a plain cargo's superseded units go and the runner's stay", () => {
  const units = [
    ["rawr", "1111111111111111"],
    ["rawr", "2222222222222222"],
    ["rawr", "5555555555555555"],
    ["rawr_gui", "3333333333333333"],
  ];
  const { root, state, profile } = target(units, [["1111111111111111"]], {
    rawr: true,
    runners: ["5555555555555555"],
  });
  prune(state, profile);
  assert.deepEqual(left(profile), [
    "librawr-1111111111111111.rlib",
    "librawr-5555555555555555.rlib",
    "rawr-1111111111111111.d",
    "rawr-5555555555555555.d",
  ]);
  fs.rmSync(root, { recursive: true });
});

test("a newer build of a key supersedes the units its earlier record named", () => {
  const units = [
    ["rawr", "1111111111111111"],
    ["rawr", "2222222222222222"],
  ];
  const { root, state, profile } = target(units, [["1111111111111111"]], { rawr: true });
  prune(state, profile);
  assert.deepEqual(left(profile), ["librawr-1111111111111111.rlib", "rawr-1111111111111111.d"]);
  // The key's next run names the new unit only.
  fs.writeFileSync(join(profile, "deps", "librawr-2222222222222222.rlib"), "");
  const file = join(state, "debug", "0.json");
  const entry = JSON.parse(fs.readFileSync(file, "utf8"));
  fs.writeFileSync(file, JSON.stringify({ ...entry, units: ["2222222222222222"] }));
  prune(state, profile);
  assert.deepEqual(left(profile), ["librawr-2222222222222222.rlib"]);
  fs.rmSync(root, { recursive: true });
});

test("a human or short message format runs as JSON with diagnostics rendered alike", () => {
  assert.deepEqual(messageFormat(["build", "-q"]), {
    options: ["build", "-q"],
    format: "json-render-diagnostics",
    json: false,
  });
  assert.deepEqual(messageFormat(["build", "--message-format=short", "-p", "rawr"]), {
    options: ["build", "-p", "rawr"],
    format: "json-render-diagnostics,json-diagnostic-short",
    json: false,
  });
  assert.deepEqual(messageFormat(["check", "--message-format", "json-diagnostic-rendered-ansi"]), {
    options: ["check"],
    format: "json-diagnostic-rendered-ansi",
    json: true,
  });
});

test("cargo after a prefix command runs the wrapper", () => {
  const script = "/w/cargo.mjs";
  assert.equal(rewrite("timeout 900 cargo test -q", script), "timeout 900 '/w/cargo.mjs' test -q");
  assert.equal(rewrite("cd x && time cargo build", script), "cd x && time '/w/cargo.mjs' build");
  assert.equal(
    rewrite("{ RUST_LOG=1 cargo +nightly clippy; }", script),
    "{ RUST_LOG=1 '/w/cargo.mjs' +nightly clippy; }",
  );
  assert.equal(rewrite("cargo fmt --all", script), "cargo fmt --all");
  assert.equal(rewrite("echo cargo test", script), "echo cargo test");
});
