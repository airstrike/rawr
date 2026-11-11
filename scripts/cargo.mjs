#!/usr/bin/env node
// Cargo, followed by a prune of what no current build in its profile
// directory names (#1732, after #1731's install prune):
//
//   scripts/cargo.mjs [+toolchain] <subcommand> <arguments>
//
// For `build`, `check`, `clippy`, `test`, `bench`, `run` and `doc` the build's
// JSON messages are read from Cargo's stdout (everything else on it, a test's
// or a program's output, passes through) and recorded as the build's key:
// its arguments before `--`, the directory it ran in, its toolchain and its
// flags. A run of a key replaces the key's record. A build that did not
// finish adds what it reached to the record instead, so a compile error
// costs no rebuild. Any other subcommand is plain Cargo.
//
// Records live in `<target>/.rawr-builds/<profile>`. Each carries two
// generations: the workspace's, Cargo.lock as it is (every version bump
// moves it), and the dependencies', the lockfile's registry and git packages
// with the toolchain's `rustc -vV`.
//
// - A unit of a workspace crate stays while a record of the current
//   workspace generation names it. So a version bump makes every older unit
//   unnamed, and each key's first run after it replaces its record.
// - A dependency's unit is left alone until the dependencies' generation
//   changes. Then it stays only when a record of the new or the previous
//   generation names it, and records of older ones are deleted. A version bump
//   leaves dependencies' hashes as they were, so it removes none of them, and
//   the keys not yet rerun keep theirs for one more generation.
//
// - Of a current unit's codegen objects, the set its rlib holds or its
//   binary's debug map names stays, and the sets earlier incremental
//   compiles left go.
//
// Nothing is removed by age. The prune holds Cargo's locks on the profile
// directory, and it waits for the last of the wrapped runs going at once:
// until a run records, it may name anything.
//
// `scripts/cargo.mjs --hook` is a Claude Code PreToolUse hook for Bash. It
// sends a command's `cargo <subcommand>` for the subcommands above through
// this script, so sessions keep typing plain `cargo`.

import { spawn, spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import fs from "node:fs";
import { basename, dirname, join, relative, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const RECORDED = new Set(["build", "b", "check", "c", "clippy", "test", "t", "bench", "run", "r", "doc", "d"]);
const DIRECTORIES = ["deps", ".fingerprint", "build", "examples"];

function wrap(args) {
  const toolchain = args[0]?.startsWith("+") ? [args[0]] : [];
  const rest = args.slice(toolchain.length);
  const separator = rest.indexOf("--");
  const options = separator < 0 ? rest : rest.slice(0, separator);
  const plain = !RECORDED.has(rest[0]) || options.some((arg) => arg.startsWith("--message-format"));
  const metadata = plain ? null : workspace(toolchain);
  if (!metadata) {
    const { status, signal } = spawnSync("cargo", args, { stdio: "inherit" });
    exit(status, signal);
  }
  const state = join(targetDirectory(options, metadata.target_directory), ".rawr-builds");
  const running = join(state, "running", String(process.pid));
  fs.mkdirSync(dirname(running), { recursive: true });
  fs.writeFileSync(running, "");

  // Ctrl-C reaches Cargo too; wait for it to stop, then record what it reached.
  process.on("SIGINT", () => {});
  const cargo = spawn("cargo", [...toolchain, rest[0], "--message-format=json-render-diagnostics", ...rest.slice(1)], {
    stdio: ["inherit", "pipe", "inherit"],
  });
  const messages = [];
  let line = "";
  // A line that may still become a Cargo message waits for its end; any other
  // goes out as it comes, so a test's partial line shows at once.
  const take = (text) => {
    for (const part of text.split(/(?<=\n)/)) {
      line += part;
      if (line.endsWith("\n")) {
        if (line.startsWith('{"reason":')) messages.push(JSON.parse(line));
        else process.stdout.write(line);
        line = "";
      } else if (!'{"reason":'.startsWith(line.slice(0, 10))) {
        process.stdout.write(line);
        line = "";
      }
    }
  };
  cargo.stdout.setEncoding("utf8").on("data", take);
  cargo.on("close", (status, signal) => {
    if (line) process.stdout.write(line);
    const profile = record(state, metadata.workspace_root, toolchain, options, messages);
    fs.rmSync(running, { force: true });
    if (profile) locked(profile, [process.execPath, fileURLToPath(import.meta.url), "--prune", state, profile]);
    exit(status, signal);
  });
}

function exit(status, signal) {
  if (signal) {
    process.removeAllListeners(signal);
    process.kill(process.pid, signal);
  }
  process.exit(status ?? 1);
}

// The workspace's root and target directory, or none outside a workspace.
function workspace(toolchain) {
  const found = spawnSync("cargo", [...toolchain, "metadata", "--no-deps", "--format-version", "1"], {
    encoding: "utf8",
    stdio: ["ignore", "pipe", "ignore"],
    maxBuffer: 1 << 28,
  });
  return found.status === 0 ? JSON.parse(found.stdout) : null;
}

function targetDirectory(options, configured) {
  const at = options.findIndex((arg) => arg === "--target-dir" || arg.startsWith("--target-dir="));
  if (at < 0) return configured;
  return resolve(options[at].includes("=") ? options[at].split("=")[1] : options[at + 1]);
}

const digest = (...parts) => {
  const hash = createHash("sha256");
  parts.forEach((part) => hash.update(part));
  return hash.digest("hex").slice(0, 16);
};

// The `<16 hex>` hash a Cargo output name carries after a dash, at its end
// or before an extension.
const hash = (name) => name.match(/-([0-9a-f]{16})(?:\.|$)/)?.[1];

// A unit's crate, as `importer/src/deps.rs`'s `unit_key` spells it: a
// library's file carries `lib` (`liblibc-<hash>.rlib`), its dep-info does not.
const crate = (name) => {
  const at = name.search(/-[0-9a-f]{16}(?:\.|$)/);
  if (at <= 0) return null;
  const library = /\.(rlib|rmeta|dylib|so|a)$/.test(name) && name.startsWith("lib");
  return name.slice(library ? 3 : 0, at).replaceAll("-", "_");
};

// The `<name>.<extension>` a hashed file carries with its hash taken out, as
// Cargo uplifts it, with dashes as underscores.
const unhashed = (name) => name.replace(/-[0-9a-f]{16}(?=\.|$)/, "").replaceAll("-", "_");

const entries = (directory) => {
  try {
    return fs.readdirSync(directory).map((name) => join(directory, name));
  } catch {
    return [];
  }
};
const size = (path) => fs.statSync(path, { throwIfNoEntry: false })?.size ?? -1;

// Write this build's record and return its profile directory, or none when
// the build produced no unit.
function record(state, root, toolchain, options, messages) {
  const lock = fs.readFileSync(join(root, "Cargo.lock"), "utf8");
  const rustc = spawnSync(process.env.RUSTC ?? "rustc", [...toolchain, "-vV"], { encoding: "utf8" }).stdout ?? "";
  const dependencies = lock
    .split("\n[[package]]\n")
    .filter((block) => /^source = /m.test(block))
    .join("\n");
  const generations = { workspace: digest(lock), dependencies: digest(dependencies, rustc) };

  let profile = null;
  const units = new Set();
  const crates = new Set();
  const incremental = {};
  const uplifted = [];
  for (const message of messages) {
    const paths = [...(message.filenames ?? []), message.executable, message.out_dir].filter(Boolean);
    const hashes = [];
    for (const path of paths) {
      profile ??= ancestor(path);
      const named = [basename(path), basename(dirname(path))].map(hash).filter(Boolean);
      hashes.push(...named);
      if (named.length === 0) uplifted.push(path);
    }
    hashes.forEach((found) => units.add(found));
    // A path package's crates are the workspace's: its package's name, which
    // `build` and `.fingerprint` spell, and its targets', which `deps` does.
    const { package_id, target, features } = message;
    if (!package_id?.includes("path+file:")) continue;
    const named = package_id.match(/#([^@#]+)@[^#]*$/)?.[1] ?? package_id.split("#")[0].split("/").pop();
    crates.add(named.replaceAll("-", "_"));
    if (message.reason === "compiler-artifact") {
      const name = target.name.replaceAll("-", "_");
      crates.add(name);
      const unit = hashes[0] ?? JSON.stringify([package_id, target.name, target.kind, features]);
      incremental[name] = [...new Set([...(incremental[name] ?? []), unit])];
    }
  }
  if (!profile) return null;
  // An uplifted copy's unit is the hashed file beside its original of its
  // name and its bytes: a binary's and a workspace library's messages name
  // only the copy.
  const hashed = [...entries(join(profile, "deps")), ...entries(join(profile, "examples"))].filter((path) =>
    hash(basename(path)),
  );
  for (const path of uplifted) {
    const name = unhashed(basename(path));
    const bytes = size(path);
    const candidates = hashed.filter((candidate) => unhashed(basename(candidate)) === name && size(candidate) === bytes);
    if (candidates.length === 0) continue;
    const held = fs.readFileSync(path);
    for (const candidate of candidates) {
      if (held.equals(fs.readFileSync(candidate))) units.add(hash(basename(candidate)));
    }
  }

  const finished = messages.some((message) => message.reason === "build-finished" && message.success);
  const key = JSON.stringify({
    directory: relative(root, process.cwd()),
    toolchain: toolchain[0] ?? process.env.RUSTUP_TOOLCHAIN ?? null,
    flags: ["RUSTFLAGS", "CARGO_ENCODED_RUSTFLAGS", "RUSTDOCFLAGS"].map((name) => process.env[name] ?? null),
    options,
  });
  const records = join(state, relative(dirname(state), profile).replaceAll("/", "+"));
  const file = join(records, `${digest(key)}.json`);
  const entry = { key, profile, ...generations, units: [...units], crates: [...crates], incremental };
  const earlier = !finished && read(file);
  if (earlier && earlier.workspace === entry.workspace) {
    entry.units = [...new Set([...earlier.units, ...entry.units])];
    entry.crates = [...new Set([...earlier.crates, ...entry.crates])];
    for (const [name, found] of Object.entries(earlier.incremental)) {
      entry.incremental[name] = [...new Set([...found, ...(entry.incremental[name] ?? [])])];
    }
  }
  fs.mkdirSync(records, { recursive: true });
  fs.writeFileSync(`${file}.${process.pid}`, JSON.stringify(entry));
  fs.renameSync(`${file}.${process.pid}`, file);
  fs.writeFileSync(join(records, "current"), JSON.stringify(generations));
  return profile;
}

// The profile directory a Cargo output lies in: the nearest directory with
// a `.fingerprint`.
function ancestor(path) {
  for (let directory = dirname(path); directory !== dirname(directory); directory = dirname(directory)) {
    if (fs.existsSync(join(directory, ".fingerprint"))) return directory;
  }
  return null;
}

function read(file) {
  try {
    return JSON.parse(fs.readFileSync(file, "utf8"));
  } catch {
    return null;
  }
}

// Run `command` holding Cargo's locks on `profile`, as a build does, in
// Cargo's order: the build directory's, then the artifact directory's two.
function locked(profile, command) {
  const locks = [".cargo-build-lock", ".cargo-lock", ".cargo-artifact-lock"]
    .map((name) => join(profile, name))
    .filter((path) => fs.existsSync(path));
  const holder = `
import fcntl, subprocess, sys
at = sys.argv.index("--")
held = [open(path, "a") for path in sys.argv[1:at]]
for file in held:
    fcntl.flock(file, fcntl.LOCK_EX)
sys.exit(subprocess.call(sys.argv[at + 1:]))
`;
  spawnSync("python3", ["-I", "-c", holder, ...locks, "--", ...command], { stdio: "inherit" });
}

function prune(state, profile) {
  for (const path of entries(join(state, "running"))) {
    try {
      process.kill(Number(basename(path)), 0);
      return; // Another wrapped run is going: the last to finish prunes.
    } catch (error) {
      if (error.code === "EPERM") return;
      fs.rmSync(path, { force: true });
    }
  }
  const records = join(state, relative(dirname(state), profile).replaceAll("/", "+"));
  const current = read(join(records, "current"));
  if (!current) return;
  const seen = read(join(records, "dependencies")) ?? [current.dependencies];
  const changed = seen[0] !== current.dependencies;
  const kept = changed ? [current.dependencies, seen[0]] : seen;
  fs.writeFileSync(join(records, "dependencies"), JSON.stringify(kept));

  const workspace = new Set();
  const dependencies = new Set();
  const crates = new Set();
  const incremental = new Map();
  for (const file of entries(records).filter((path) => path.endsWith(".json"))) {
    const entry = read(file);
    if (!entry || !kept.includes(entry.dependencies)) {
      fs.rmSync(file, { force: true });
      continue;
    }
    entry.crates.forEach((name) => crates.add(name));
    entry.units.forEach((unit) => dependencies.add(unit));
    if (entry.workspace !== current.workspace) continue;
    entry.units.forEach((unit) => workspace.add(unit));
    for (const [name, units] of Object.entries(entry.incremental)) {
      incremental.set(name, new Set([...(incremental.get(name) ?? []), ...units]));
    }
  }
  if (workspace.size === 0) return;

  let removed = 0;
  const remove = (path) => {
    fs.rmSync(path, { recursive: true, force: true });
    removed += 1;
  };
  for (const directory of DIRECTORIES) {
    for (const path of entries(join(profile, directory))) {
      const name = basename(path);
      const found = hash(name);
      if (!found) continue;
      const stale = crates.has(crate(name)) ? !workspace.has(found) : changed && !dependencies.has(found);
      if (stale) remove(path);
    }
  }
  // Each incremental compile of a unit leaves a new set of codegen objects
  // beside its output (macOS keeps debug info in them), and the earlier sets
  // stay. The current set is the one the unit's rlib holds or its binary's
  // debug map names. A unit with one set is left as it is.
  for (const directory of ["deps", "examples"].map((name) => join(profile, name))) {
    const objects = new Map();
    for (const name of entries(directory).map((path) => basename(path))) {
      if (!name.endsWith(".rcgu.o")) continue;
      const unit = name.slice(0, name.indexOf("."));
      objects.set(unit, [...(objects.get(unit) ?? []), name]);
    }
    for (const [unit, names] of objects) {
      if (new Set(names.map((name) => name.split(".").at(-3))).size < 2) continue;
      const current = archived(join(directory, `lib${unit}.rlib`)) ?? linked(join(directory, unit));
      names.filter((name) => current && !current.has(name)).forEach((name) => remove(join(directory, name)));
    }
  }
  // An incremental directory is `<crate>-<stable crate id>`, which no message
  // names, and each version gives each unit a new one. A crate keeps its
  // newest directories, one for each unit current records give it.
  const directories = new Map();
  for (const path of entries(join(profile, "incremental"))) {
    const name = basename(path).slice(0, basename(path).lastIndexOf("-"));
    directories.set(name, [...(directories.get(name) ?? []), { path, time: fs.statSync(path).mtimeMs }]);
  }
  for (const [name, found] of directories) {
    found.sort((a, b) => b.time - a.time);
    found.slice(incremental.get(name)?.size ?? 0).forEach(({ path }) => remove(path));
  }
  if (removed > 0) console.error(`pruned ${removed} superseded entries from ${profile}`);
}

// Read `length` bytes of the open file `fd` at `offset`.
const bytes = (fd, offset, length) => {
  const buffer = Buffer.alloc(length);
  fs.readSync(fd, buffer, 0, length, offset);
  return buffer;
};

// Run `read` on the file at `path` opened, or give none when it can't be.
function opened(path, read) {
  let fd;
  try {
    fd = fs.openSync(path, "r");
    return read(fd);
  } catch {
    return null;
  } finally {
    if (fd !== undefined) fs.closeSync(fd);
  }
}

// The member names of the archive at `path`: BSD names (`#1/<length>`, the
// name after the header) or GNU ones (`/<offset>` into the `//` table).
function archived(path) {
  return opened(path, (fd) => {
    if (bytes(fd, 0, 8).toString("latin1") !== "!<arch>\n") return null;
    const names = new Set();
    let table = null;
    for (let offset = 8, header; (header = bytes(fd, offset, 60)).toString("latin1", 58, 60) === "`\n"; ) {
      const id = header.toString("latin1", 0, 16).trimEnd();
      const length = Number(header.toString("latin1", 48, 58).trim());
      if (id.startsWith("#1/")) {
        names.add(bytes(fd, offset + 60, Number(id.slice(3))).toString("latin1").replace(/\0+$/, ""));
      } else if (id === "//") {
        table = bytes(fd, offset + 60, length).toString("latin1");
      } else if (/^\/\d+$/.test(id) && table) {
        const start = Number(id.slice(1));
        names.add(table.slice(start, table.indexOf("/\n", start)));
      } else {
        names.add(id.replace(/\/$/, ""));
      }
      offset += 60 + length + (length % 2);
    }
    return names;
  });
}

// The object files the debug map of the 64-bit Mach-O binary at `path`
// names (its `N_OSO` symbols), by file name: a member's name when the path
// is an archive's.
function linked(path) {
  return opened(path, (fd) => {
    const header = bytes(fd, 0, 32);
    if (header.readUInt32LE(0) !== 0xfeedfacf) return null;
    const commands = bytes(fd, 32, header.readUInt32LE(20));
    for (let at = 0, count = 0; count < header.readUInt32LE(16); count += 1) {
      if (commands.readUInt32LE(at) === 0x2) {
        const symbols = bytes(fd, commands.readUInt32LE(at + 8), commands.readUInt32LE(at + 12) * 16);
        const strings = bytes(fd, commands.readUInt32LE(at + 16), commands.readUInt32LE(at + 20));
        const names = new Set();
        for (let symbol = 0; symbol < symbols.length; symbol += 16) {
          if (symbols[symbol + 4] !== 0x66) continue;
          const start = symbols.readUInt32LE(symbol);
          const name = strings.toString("utf8", start, strings.indexOf(0, start));
          names.add(name.match(/\(([^)]+)\)$/)?.[1] ?? basename(name));
        }
        return names;
      }
      at += commands.readUInt32LE(at + 4);
    }
    return null;
  });
}

// Rewrite the Bash command of the PreToolUse event on stdin: each `cargo`
// that starts a command, after any variable assignments, and runs a recorded
// subcommand runs this script instead.
function hook() {
  const event = JSON.parse(fs.readFileSync(0, "utf8"));
  const command = event.tool_input?.command ?? "";
  const script = fileURLToPath(import.meta.url).replaceAll("'", "'\\''");
  const subcommands = [...RECORDED].join("|");
  const pattern = new RegExp(
    `(^|[;&|(\\n]\\s*|\\bthen\\s+|\\bdo\\s+)((?:[A-Za-z_][A-Za-z0-9_]*=\\S*\\s+)*)cargo((?:\\s+\\+\\S+)?\\s+(?:${subcommands})(?=\\s|$|[;&|)]))`,
    "g",
  );
  const rewritten = command.replace(pattern, `$1$2'${script}'$3`);
  if (rewritten === command) return;
  const updatedInput = { ...event.tool_input, command: rewritten };
  console.log(JSON.stringify({ hookSpecificOutput: { hookEventName: "PreToolUse", updatedInput } }));
}

if (process.argv[2] === "--hook") {
  hook();
} else if (process.argv[2] === "--prune") {
  prune(...process.argv.slice(3));
} else {
  wrap(process.argv.slice(2));
}
