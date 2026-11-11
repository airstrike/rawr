#!/usr/bin/env node
// Remove from a Cargo profile directory what the builds that just ran there
// neither produced nor used (#1731), as `importer/src/deps.rs` does for a
// project's runner (`prune_superseded`, #1722):
//
//   node scripts/prune-build.mjs <profile directory> <marker> <messages>
//
// `messages` holds the `--message-format=json` output of every build the
// profile directory serves, and `marker` a file made just before the first
// of them. The profile directory must belong to
// those builds alone: whatever they did not name is removed.
//
// A unit is `<name>-<16 hex>` in `deps`, `.fingerprint` and `build`. A unit
// is current when its hash is in a path the messages give, or when its file
// in `deps` is the one Cargo uplifted to a path they give (a binary's and a
// workspace library's messages name only the uplifted copy). Every
// other unit goes.
//
// An incremental directory is `<crate>-<stable crate id>`, which no message
// names, but the messages say how many units of each crate the builds have.
// One a build compiled into is current. The units the builds used fresh
// are the crate's newest other directories, as many as the crate has units
// beyond those compiled. Every other one goes: each version gives each
// unit a new stable crate id, so a new directory.

import { readdirSync, readFileSync, rmSync, statSync } from "node:fs";
import { basename, dirname, join, resolve } from "node:path";

const [directory, marker, messages] = process.argv.slice(2);
if (!directory || !marker || !messages) {
  console.error("usage: prune-build.mjs <profile directory> <marker> <messages>");
  process.exit(2);
}
const profile = resolve(directory);
const since = statSync(marker).mtimeMs;

// The `<16 hex>` hash a Cargo output name carries after a dash, at its end
// or before an extension.
const hash = (name) => name.match(/-([0-9a-f]{16})(?:\.|$)/)?.[1];

const entries = (child) => {
  try {
    return readdirSync(join(profile, child)).map((name) => join(profile, child, name));
  } catch {
    return [];
  }
};

const size = (path) => {
  try {
    return statSync(path).size;
  } catch {
    return -1;
  }
};

// The `<name>.<extension>` a file in `deps` carries with its hash taken
// out, as Cargo uplifts it: `demo-<hash>` is `demo`, `libdemo-<hash>.rlib`
// is `libdemo.rlib`, with dashes as underscores (a binary `rawr-import` is
// `rawr_import-<hash>` in `deps`).
const unhashed = (name) => name.replace(/-[0-9a-f]{16}(?=\.|$)/, "").replaceAll("-", "_");

const live = new Set();
// The files a message names without a hash: Cargo's uplifted copies.
const uplifted = [];
// The distinct units of each crate of a path package, which rustc compiles
// incrementally: by the hash the unit's files carry, or by its target, kind
// and features when they carry none (an uplifted binary).
const units = new Map();
for (const line of readFileSync(messages, "utf8").split("\n")) {
  if (!line.startsWith("{")) continue;
  const message = JSON.parse(line);
  const paths = [...(message.filenames ?? []), message.executable, message.out_dir].filter(Boolean);
  const hashes = [];
  for (const path of paths) {
    const named = [basename(path), basename(dirname(path))].map(hash).filter(Boolean);
    hashes.push(...named);
    if (named.length === 0 && dirname(path) === profile) uplifted.push(path);
  }
  hashes.forEach((found) => live.add(found));
  if (message.reason === "compiler-artifact" && message.package_id.includes("path+file:")) {
    const { package_id, target, features } = message;
    const crate = target.name.replaceAll("-", "_");
    const unit = hashes[0] ?? JSON.stringify([package_id, target.name, target.kind, features]);
    units.set(crate, (units.get(crate) ?? new Set()).add(unit));
  }
}
// An uplifted copy's unit is the file in `deps` of its name and its bytes.
const deps = entries("deps");
for (const path of uplifted) {
  const name = unhashed(basename(path));
  const bytes = size(path);
  const candidates = deps.filter(
    (candidate) => hash(basename(candidate)) && unhashed(basename(candidate)) === name && size(candidate) === bytes,
  );
  if (candidates.length === 0) continue;
  const held = readFileSync(path);
  for (const candidate of candidates) {
    if (held.equals(readFileSync(candidate))) live.add(hash(basename(candidate)));
  }
}

if (live.size === 0) process.exit(0);

let removed = 0;
const remove = (path) => {
  rmSync(path, { recursive: true, force: true });
  removed += 1;
};

for (const directory of ["deps", ".fingerprint", "build"]) {
  for (const path of entries(directory)) {
    const found = hash(basename(path));
    if (found && !live.has(found)) remove(path);
  }
}

const crate = (name) => name.slice(0, name.lastIndexOf("-"));
const directories = new Map();
for (const path of entries("incremental")) {
  const name = crate(basename(path));
  directories.set(name, [...(directories.get(name) ?? []), { path, time: statSync(path).mtimeMs }]);
}
for (const [name, found] of directories) {
  const wanted = units.get(name)?.size ?? 0;
  const compiled = found.filter(({ time }) => time >= since);
  const rest = found.filter(({ time }) => time < since).sort((a, b) => b.time - a.time);
  rest.slice(Math.max(0, wanted - compiled.length)).forEach(({ path }) => remove(path));
}
console.log(`pruned ${removed} superseded entries from ${profile}`);
