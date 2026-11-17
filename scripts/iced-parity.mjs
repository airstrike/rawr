#!/usr/bin/env node
// Checks that iced's examples behave the same under `cargo test` and `rawr test`.
//
// Usage: node scripts/iced-parity.mjs [--jobs N] [--rawr-emulator] [--drop-rawr-builds] [example ...]
//        node scripts/iced-parity.mjs --strip [example ...]
//
// The examples are the standalone crates scripts/iced-examples.sh makes under
// target/scratch/iced-examples (ICED_PARITY_SRC overrides). The check runs in
// them in place, so cargo's shared target (`_target`, which their
// `.cargo/config.toml` names) and rawr's project builds under
// `_target/rawr/projects/` stay warm between runs. It adds to each example:
//
// - src/parity.rs, generated from scripts/iced-parity, and one line at the end
//   of src/main.rs that declares it as `#[cfg(test)] mod parity`;
// - lines in Cargo.toml ending in `# iced-parity`: iced_test as a
//   dev-dependency where the example lacks it, and the `iced-parity-emulator`
//   feature.
//
// `--strip` removes all three, and the example's target/iced-parity directory.
// `--drop-rawr-builds` deletes each rawr project build this run made, once it has
// run (rawr 0.3.214 keeps a few hundred MB per build in its store, about
// 0.5 to 2 GB per example), for a full run on a disk that can't keep all 56
// warm; the next run of that example is then cold for rawr.
//
// The parity module holds these tests:
//
// - `sim_<name>` replays <name>.ice with iced_test's Simulator: each run of
//   interactions goes to a simulator of the example's `view`, its messages go
//   to `update`, and each `expect` looks for its text in a fresh view. Tasks
//   and subscriptions don't run. It hashes the final frame through
//   `Snapshot::matches_hash` into target/iced-parity/snapshots.
// - `ice_<name>`, behind the `iced-parity-emulator` feature, replays the same
//   script through iced_test's Emulator, which runs the whole program (tasks,
//   subscriptions) from the `iced::application(..)` chain in `main`, as
//   `iced_test::run` does, and hashes its final screenshot.
// - <name>.rs, when present, is appended: hand-written tests (a `sim_<name>`
//   there replaces the generated one) for what the
//   script can't express, or a `program()` the chain can't give.
//
// Both draw headless with tiny-skia (ICED_TEST_BACKEND), so no window opens and
// no GPU is used. cargo runs every test once with the emulator feature, for the
// emulator's own results. rawr runs the tests without it, and cargo runs that
// same set twice to compare with: tests in one process share iced's font
// system, so the emulator's loading the example's fonts changes what a
// simulator test beside it draws. With `--rawr-emulator`, rawr first tries the
// set with the feature, compared with cargo's, and falls back when that half
// doesn't build. (rawr 0.3.214 can't build the emulator half: its generated
// adapters name the program type as `iced::Application<_>`. Trying costs about
// 5 s per warm example, so it is opt-in until that's fixed.) A hash counts only
// when both cargo runs agree on it, so frames that show the clock or a random
// seed don't count as mismatches.
//
// Examples run `--jobs` at a time (default 4); cargo's lock on the shared
// target serializes their builds, not their tests. The tests run with HOME and
// the XDG directories under the example's target/iced-parity, so an example
// that saves state writes there, not to the user's data directory. The run
// stops before an example when free disk is under ICED_PARITY_MIN_FREE_GB
// (default 8): it never deletes the shared target's caches.
//
// Prints one row per example with each step's wall time, writes
// target/iced-parity-report.json, and exits 1 when any example's runners
// disagree.

import { execFileSync, spawn } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";

const here = path.dirname(new URL(import.meta.url).pathname);
const scripts = path.join(here, "iced-parity");
// Cargo goes through scripts/cargo.mjs, which records the units each run
// uses and removes the test builds a newer run of the same example
// superseded, so the shared target keeps what the examples use and nothing
// stale. It is set before the workers start, as they run from the
// top-level await below.
const cargoWrapper = path.join(path.dirname(fileURLToPath(import.meta.url)), "cargo.mjs");
const checkout = path.resolve(
  execFileSync("git", ["-C", here, "rev-parse", "--path-format=absolute", "--git-common-dir"], {
    encoding: "utf8",
  }).trim(),
  "..",
);
const source = process.env.ICED_PARITY_SRC ?? path.join(checkout, "target/scratch/iced-examples");
const minFreeGb = Number(process.env.ICED_PARITY_MIN_FREE_GB ?? 8);
const icedRev = "be88873d38544455e26c7a104eb757da2cbbac6b";
const feature = "iced-parity-emulator";
const mark = "# iced-parity";
const include = '#[cfg(test)] #[path = "parity.rs"] mod parity;';
const cargoHome = process.env.CARGO_HOME ?? path.join(os.homedir(), ".cargo");
const rustupHome = process.env.RUSTUP_HOME ?? path.join(os.homedir(), ".rustup");

const options = { jobs: 4, strip: false, rawrEmulator: false, dropRawrBuilds: false, names: [] };
const argv = process.argv.slice(2);
for (let i = 0; i < argv.length; i += 1) {
  if (argv[i] === "--jobs") options.jobs = Number(argv[++i]);
  else if (argv[i] === "--strip") options.strip = true;
  else if (argv[i] === "--rawr-emulator") options.rawrEmulator = true;
  else if (argv[i] === "--drop-rawr-builds") options.dropRawrBuilds = true;
  else options.names.push(argv[i]);
}
const examples = options.names.length > 0 ? options.names : allExamples();
const rawrProjectsDir = path.join(source, "_target", "rawr", "projects");
const projectsBefore = new Set(rawrProjects());

if (options.strip) {
  for (const name of examples) strip(name);
  process.exit(0);
}

console.log(row(["example", "cargo", "rawr", "verdict", "s: cargo, cargo warm, rawr, rawr warm"]));
console.log(row(["---", "---", "---", "---", "---"]));
const rows = [];
let next = 0;
let stopped = false;
await Promise.all(Array.from({ length: options.jobs }, worker));
rows.sort((a, b) => a.name.localeCompare(b.name));
fs.writeFileSync(
  path.join(checkout, "target/iced-parity-report.json"),
  JSON.stringify(rows, null, 2) + "\n",
);
const agreeing = rows.filter((result) => result.agrees).length;
console.log(`\n${agreeing} of ${rows.length} agree; logs in <example>/target/iced-parity/logs`);
process.exit(agreeing === examples.length ? 0 : 1);

async function worker() {
  while (next < examples.length && !stopped) {
    const name = examples[next];
    next += 1;
    if (freeGb() < minFreeGb) {
      console.error(`free disk under ${minFreeGb} GB: stopping before ${name}`);
      stopped = true;
      return;
    }
    const result = await check(name);
    rows.push(result);
    const times = result.seconds.map((s) => (s === null ? "-" : s.toFixed(1))).join(", ");
    console.log(row([name, result.cargo, result.rawr, result.verdict, times]));
  }
}

function allExamples() {
  return fs
    .readdirSync(source)
    .filter((name) => fs.existsSync(path.join(source, name, "Cargo.toml")))
    .sort();
}

function row(cells) {
  return `| ${cells.join(" | ")} |`;
}

// The example's own output directory, where cargo and rawr also build nothing.
function scratchDir(dir) {
  return path.join(dir, "target", "iced-parity");
}

// Writes the example's parity module, declares it and edits its manifest.
// Running it again rewrites the module and leaves the rest as it is.
function install(name) {
  const dir = path.join(source, name);
  const manifest = path.join(dir, "Cargo.toml");
  fs.writeFileSync(manifest, withParityManifest(fs.readFileSync(manifest, "utf8")));

  const iceFile = path.join(scripts, `${name}.ice`);
  const iceText = fs.existsSync(iceFile) ? fs.readFileSync(iceFile, "utf8") : null;
  const ice = iceText === null ? null : parseIce(iceText);
  const extraFile = path.join(scripts, `${name}.rs`);
  const extra = fs.existsSync(extraFile) ? fs.readFileSync(extraFile, "utf8") : "";
  const ownProgram = /\bfn program\(\)/.test(extra);

  const main = path.join(dir, "src/main.rs");
  const mainText = fs.readFileSync(main, "utf8");
  const expression = programExpression(mainText);

  let parity =
    "// rawr\n" +
    "// Generated by scripts/iced-parity.mjs; `node scripts/iced-parity.mjs --strip`\n" +
    "// removes it.\n" +
    "use super::*;\n\n";
  parity += fs.readFileSync(path.join(scripts, "simulator.rs"), "utf8");
  const parts = expression === null ? null : applicationParts(expression, mainText);
  // <name>.rs may write the simulator test by hand, as `fn sim_<name>`.
  const ownSimulator = new RegExp(`\\bfn sim_${name}\\(\\)`).test(extra);
  if (ice !== null && parts !== null && !ownSimulator) {
    parity += simulatorTest(`sim_${name}`, ice, parts);
  }
  if (extra !== "") {
    parity += `\n${extra.replace(/^\/\/ rawr\n/, "")}`;
  }

  // The emulator half is an inline module: `rawr test` 0.3.214 ignores
  // `#[path]` on a module declared outside the crate root.
  let emulated = `\n#[cfg(feature = "${feature}")]\nmod emulated {\nuse super::*;\n\n`;
  emulated += fs.readFileSync(path.join(scripts, "emulator.rs"), "utf8");
  if (!ownProgram && expression !== null) {
    emulated +=
      `\nfn program() -> impl Program<Theme = iced_test::core::Theme> + 'static {\n` +
      `    ${expression}\n}\n`;
  }
  if (ice !== null && (ownProgram || expression !== null)) {
    const test = `ice_${name}`;
    const preset = ice.preset === null ? "" : `preset ${JSON.stringify(ice.preset)}, `;
    emulated +=
      `\nconst SCRIPT: &str = r#"${iceText}"#;\n` +
      `\n#[test]\nfn ${test}() {\n` +
      `    let app = program();\n` +
      `    let outcome = replay!(app, ${preset}SCRIPT);\n` +
      `    outcome.record("${test}");\n` +
      `    outcome.assert_passed();\n}\n`;
  }
  parity += `${emulated}}\n`;
  writeIfChanged(path.join(dir, "src/parity.rs"), parity);

  if (!mainText.split("\n").includes(include)) {
    fs.writeFileSync(main, `${mainText.replace(/\n*$/, "\n")}${include}\n`);
  }
  return dir;
}

// Leaves an unchanged module's mtime alone, so cargo and rawr don't rebuild.
function writeIfChanged(file, text) {
  if (!fs.existsSync(file) || fs.readFileSync(file, "utf8") !== text) {
    fs.writeFileSync(file, text);
  }
}

function strip(name) {
  const dir = path.join(source, name);
  const manifest = path.join(dir, "Cargo.toml");
  const lines = fs.readFileSync(manifest, "utf8").split("\n");
  fs.writeFileSync(manifest, lines.filter((line) => !line.endsWith(mark)).join("\n"));
  const main = path.join(dir, "src/main.rs");
  const mainLines = fs.readFileSync(main, "utf8").split("\n");
  fs.writeFileSync(main, mainLines.filter((line) => line !== include).join("\n"));
  fs.rmSync(path.join(dir, "src/parity.rs"), { force: true });
  fs.rmSync(scratchDir(dir), { recursive: true, force: true });
  console.log(`stripped ${name}`);
}

// Adds iced_test as a dev-dependency at the examples' iced revision, and the
// feature that turns on the emulator tests. Every added line ends in the mark.
function withParityManifest(manifest) {
  let text = manifest.replace(/\n*$/, "\n");
  if (!/^iced_test\s*=/m.test(text)) {
    const line = `iced_test = { git = "https://github.com/iced-rs/iced", rev = "${icedRev}" } ${mark}`;
    text = /^\[dev-dependencies\]$/m.test(text)
      ? text.replace(/^\[dev-dependencies\]$/m, `[dev-dependencies]\n${line}`)
      : `${text}[dev-dependencies] ${mark}\n${line}\n`;
  }
  if (!new RegExp(`^${feature} =`, "m").test(text)) {
    const line = `${feature} = [] ${mark}`;
    text = /^\[features\]$/m.test(text)
      ? text.replace(/^\[features\]$/m, `[features]\n${line}`)
      : `${text}[features] ${mark}\n${line}\n`;
  }
  return text;
}

// The header and instructions of an .ice script, as iced_test reads them:
// the header ends at the first '-', the instructions start on the next line.
function parseIce(text) {
  const dash = text.indexOf("-");
  const header = text.slice(0, dash);
  const viewport = /^viewport:\s*(\d+)x(\d+)\s*$/m.exec(header);
  const preset = /^preset:\s*(.+?)\s*$/m.exec(header);
  const lines = text.slice(text.indexOf("\n", dash) + 1).split("\n").filter((line) => line !== "");
  return {
    width: Number(viewport[1]),
    height: Number(viewport[2]),
    preset: preset === null ? null : preset[1],
    lines,
  };
}

// The iced program `main` runs: the builder chain before its `.run()`.
// `iced::run(update, view)` becomes the `iced::application` it stands for.
function programExpression(text) {
  const main = text.search(/fn main\(\)/);
  if (main < 0) {
    return null;
  }
  const run = /iced::run\(\s*([^,]+?)\s*,\s*([^,)]+?)\s*,?\s*\)/.exec(text.slice(main));
  if (run !== null) {
    const [, update, view] = run;
    const boot = update === "()" ? "|| ()" : update.replace(/::update$/, "::default");
    return `iced::application(${boot}, ${update}, ${view})`;
  }
  const end = text.indexOf(".run()", main);
  if (end < 0) {
    return null;
  }
  const starts = ["iced::application", "iced::daemon", "application()"]
    .map((start) => text.lastIndexOf(start, end))
    .filter((at) => at > main);
  if (starts.length === 0) {
    return null;
  }
  return text.slice(Math.max(...starts), end).trim();
}

// The boot, update and view functions of an `iced::application` chain, with
// how to call boot; null for a daemon or a chain behind a function.
function applicationParts(expression, mainText) {
  const call = /^iced::application(::timed)?\(/.exec(expression);
  if (call === null) {
    return null;
  }
  const args = topLevelArgs(expression.slice(call[0].length));
  const [boot, update, view] = call[1] ? [args[0], args[1], args[3]] : args;
  let start;
  if (boot === "|| ()") {
    start = "let mut state = ();";
  } else {
    const fn = boot.split("::").pop();
    const tuple = new RegExp(`fn ${fn}\\s*\\(\\s*\\)\\s*->\\s*\\(`).test(mainText);
    start = tuple ? `let (mut state, _boot) = ${boot}();` : `let mut state = ${boot}();`;
  }
  return { start, update: update === "()" ? null : update, view };
}

// Splits a call's arguments at its top-level commas, up to its closing paren.
function topLevelArgs(text) {
  const args = [];
  let depth = 0;
  let current = "";
  for (const char of text) {
    if (depth === 0 && (char === "," || char === ")")) {
      if (current.trim() !== "") args.push(current.trim());
      current = "";
      if (char === ")") break;
      continue;
    }
    if ("([{".includes(char)) depth += 1;
    if (")]}".includes(char)) depth -= 1;
    current += char;
  }
  return args;
}

// A test that replays the script's instructions on simulators of the view.
function simulatorTest(test, ice, parts) {
  const simulator = (indent) =>
    `${indent}let mut ui = iced_test::Simulator::with_size(\n` +
    `${indent}    iced_test::core::Settings::default(),\n` +
    `${indent}    (${ice.width}.0, ${ice.height}.0),\n` +
    `${indent}    ${parts.view}(&state),\n` +
    `${indent});\n`;
  const failing = (indent, call, line) =>
    `${indent}if ${call}.is_err() {\n${indent}    stop("${test}", ${JSON.stringify(line)});\n${indent}}\n`;

  let body = `    ${parts.start}\n`;
  let pending = [];
  const flush = () => {
    if (pending.length === 0) return;
    body += "    let messages: Vec<_> = {\n" + simulator("        ");
    body += pending.join("");
    body += "        ui.into_messages().collect()\n    };\n";
    if (parts.update !== null) {
      body += `    for message in messages {\n        let _ = ${parts.update}(&mut state, message);\n    }\n`;
    }
    pending = [];
  };
  for (const line of ice.lines) {
    const text = /^click "((?:[^"\\]|\\.)*)"$/.exec(line);
    const id = /^click #([\w-]+)$/.exec(line);
    const typed = /^type "((?:[^"\\]|\\.)*)"$/.exec(line);
    const key = /^type (enter|escape|tab|backspace)$/.exec(line);
    const expect = /^expect "((?:[^"\\]|\\.)*)"$/.exec(line);
    if (text !== null) {
      pending.push(failing("        ", `ui.click("${text[1]}")`, line));
    } else if (id !== null) {
      pending.push(failing("        ", `ui.click(iced_test::selector::id("${id[1]}"))`, line));
    } else if (typed !== null) {
      pending.push(`        let _ = ui.typewrite("${typed[1]}");\n`);
    } else if (key !== null) {
      const named = key[1][0].toUpperCase() + key[1].slice(1);
      pending.push(`        let _ = ui.tap_key(iced_test::core::keyboard::key::Named::${named});\n`);
    } else if (expect !== null) {
      flush();
      body += "    {\n" + simulator("        ");
      body += failing("        ", `ui.find("${expect[1]}")`, line);
      body += "    }\n";
    } else {
      throw new Error(`${test}: the simulator replay has no step for \`${line}\``);
    }
  }
  flush();
  body +=
    simulator("    ") +
    `    let frame = match ui.snapshot(&iced_test::core::Theme::Light) {\n` +
    `        Ok(frame) => frame,\n` +
    `        Err(error) => panic!("the headless renderer draws the final frame: {error}"),\n` +
    `    };\n` +
    `    if let Err(error) = frame.matches_hash("target/iced-parity/snapshots/${test}") {\n` +
    `        panic!("the example's target directory is writable: {error}");\n` +
    `    }\n` +
    `    println!("iced-parity ${test} passed");\n`;
  return `\n#[test]\nfn ${test}() {\n${body}}\n`;
}

// The headless renderer the example's tests draw with: tiny-skia, unless its
// extra Rust names another with an `// iced-parity: backend <name>` line, for
// an example whose widgets draw only with wgpu.
function backend(dir) {
  const parity = fs.readFileSync(path.join(dir, "src/parity.rs"), "utf8");
  const named = /^\/\/ iced-parity: backend (\S+)$/m.exec(parity);
  return named === null ? "tiny-skia" : named[1];
}

function testEnv(dir) {
  const home = path.join(scratchDir(dir), "home");
  fs.mkdirSync(home, { recursive: true });
  // ICED_THEME would pick the default theme; the parity module draws a
  // program without one in `Theme::Light`, the default while it is unset.
  const { ICED_THEME, ...inherited } = process.env;
  return {
    ...inherited,
    CARGO_HOME: cargoHome,
    RUSTUP_HOME: rustupHome,
    HOME: home,
    XDG_DATA_HOME: path.join(home, ".local/share"),
    XDG_CONFIG_HOME: path.join(home, ".config"),
    XDG_CACHE_HOME: path.join(home, ".cache"),
    XDG_STATE_HOME: path.join(home, ".local/state"),
    ICED_TEST_BACKEND: backend(dir),
  };
}

function capture(command, args, dir) {
  const [program, programArgs] =
    command === "cargo" ? [process.execPath, [cargoWrapper, ...args]] : [command, args];
  return new Promise((resolve) => {
    const child = spawn(program, programArgs, { cwd: dir, env: testEnv(dir) });
    let output = "";
    child.stdout.on("data", (chunk) => (output += chunk));
    child.stderr.on("data", (chunk) => (output += chunk));
    child.on("close", (status) => resolve({ status, output }));
  });
}

// Runs one runner in the example and reads each test's outcome from its output.
async function runTests(dir, label, command, args) {
  const snapshots = path.join(scratchDir(dir), "snapshots");
  fs.rmSync(snapshots, { recursive: true, force: true });
  // libtest prints a line per test with `--format pretty`, which cargo's `-q`
  // would otherwise make terse; `rawr test` prints those lines and takes no
  // `--format`.
  const trailing = command === "cargo" ? ["--format", "pretty", "--nocapture"] : ["--nocapture"];
  const full = [...args, "--", ...trailing];
  const started = performance.now();
  const { status, output } = await capture(command, full, dir);
  const seconds = (performance.now() - started) / 1000;
  const logs = path.join(scratchDir(dir), "logs");
  fs.mkdirSync(logs, { recursive: true });
  fs.writeFileSync(
    path.join(logs, `${label}.txt`),
    `$ ${command} ${full.join(" ")}\nexit ${status} after ${seconds.toFixed(1)} s\n${output}`,
  );

  // With `--nocapture` a test's own output can land between libtest's
  // `test name ... ` and its status, so a test's status comes from the
  // summary's `failures:` list and from `... ignored`, not from that line.
  const statuses = output.replace(/iced-parity [^\n]*\n/g, "");
  const failures = new Set();
  for (const block of statuses.matchAll(/^failures:\n((?: {4}\S+\n)+)/gm)) {
    for (const name of block[1].trim().split(/\s+/)) failures.add(name.replace(/^.*::/, ""));
  }
  const tests = {};
  for (const match of statuses.matchAll(/^test (\S+) \.\.\. (ignored)?/gm)) {
    const test = match[1].replace(/^.*::/, "");
    tests[test] = match[2] ?? (failures.has(test) ? "FAILED" : "ok");
  }
  const details = {};
  for (const match of output.matchAll(/iced-parity (\S+) ((?:passed|failed)\b.*)$/gm)) {
    details[match[1]] = match[2].trim();
  }
  if (fs.existsSync(snapshots)) {
    for (const file of fs.readdirSync(snapshots)) {
      const test = file.replace(/-[\w-]+\.sha256$/, "");
      const hash = fs.readFileSync(path.join(snapshots, file), "utf8").trim().slice(0, 16);
      details[test] = `${details[test] ?? "passed"} ${hash}`;
    }
  }
  // A runner that dies mid-run prints no summary; the tests it had started
  // without a status crashed with it, and its panic message says why.
  const began = /^running \d+ tests?$/m.test(output);
  const ran = began || /test result: /.test(output);
  if (began && !/test result: /.test(output)) {
    for (const match of statuses.matchAll(/^test (\S+) \.\.\. ?$/gm)) {
      const test = match[1].replace(/^.*::/, "");
      tests[test] = "crashed";
      details[test] = `crashed: ${/^thread .* panicked at [^\n]*\n([^\n]*)/m.exec(output)?.[1] ?? `exit ${status}`}`;
    }
  }
  return { ran, status, tests, details, error: ran ? null : firstError(output), seconds };
}

// The first error a failed build reports, with the file it names.
function firstError(output) {
  const lines = output.split("\n");
  const at = lines.findIndex(
    (line) => /^\s*error(\[E\d+\])?:/.test(line) && !/rawr could not build/.test(line),
  );
  // A build that stops without a compiler error (a panic, a signal, a rawr
  // message) is named by the last line it printed that isn't progress.
  if (at < 0) {
    const last = lines.findLast((line) => line.trim() !== "" && !/^rawr > /.test(line));
    return last === undefined ? "no output" : `no error line; last: ${last.trim()}`;
  }
  const where = lines.slice(at + 1, at + 3).find((line) => line.includes("-->"));
  const file = where ? ` (${path.basename(where.trim().replace(/^--> /, ""))})` : "";
  return `${lines[at].trim()}${file}`;
}

function describe(run, filter = () => true) {
  if (!run.ran) {
    return "build fails";
  }
  const counts = { ok: 0, FAILED: 0, ignored: 0, crashed: 0 };
  for (const [test, status] of Object.entries(run.tests)) {
    if (filter(test)) counts[status] += 1;
  }
  let text = `${counts.ok} ok`;
  if (counts.FAILED > 0) text += `, ${counts.FAILED} failed`;
  if (counts.crashed > 0) text += `, ${counts.crashed} crashed`;
  if (counts.ignored > 0) text += `, ${counts.ignored} ignored`;
  return text;
}

async function check(name) {
  const dir = install(name);
  // Each rawr run is compared with cargo runs of the same tests: tests in one
  // process share iced's font system, so the emulator's loading the example's
  // fonts changes what a simulator test in the same run draws.
  const emulatorArgs = ["test", "-q", "--features", feature];
  const emulator = await runTests(dir, "cargo-emulator", "cargo", emulatorArgs);
  const emulated = options.rawrEmulator
    ? await runTests(dir, "rawr", "rawr", ["test", "--features", feature])
    : null;
  const withEmulator = emulated?.ran === true;
  const cargoArgs = withEmulator ? emulatorArgs : ["test", "-q"];
  const cargo = withEmulator ? emulator : await runTests(dir, "cargo", "cargo", cargoArgs);
  const again = await runTests(dir, "cargo-again", "cargo", cargoArgs);
  const rawrArgs = withEmulator ? ["test", "--features", feature] : ["test"];
  const rawr = withEmulator ? emulated : await runTests(dir, "rawr-simulator", "rawr", rawrArgs);
  // The second rawr run is the warm one; it's timed, not compared.
  const warm = await runTests(dir, "rawr-again", "rawr", rawrArgs);
  const seconds = [cargo.seconds, again.seconds, rawr.seconds, warm.seconds];
  if (options.dropRawrBuilds) {
    dropRawrBuild(name);
  }

  if (!cargo.ran || !emulator.ran) {
    // The parity module must build under cargo; a failure there is the
    // harness's fault, not a difference between the runners.
    const verdict = `HARNESS: ${(cargo.ran ? emulator : cargo).error}`;
    return { name, cargo: "build fails", rawr: "-", verdict, agrees: false, seconds };
  }
  const differences = [];
  if (emulated !== null && !emulated.ran) {
    differences.push(`emulator half doesn't build: ${emulated.error}`);
  }
  if (!rawr.ran) {
    differences.push(`simulator half doesn't build: ${rawr.error}`);
  }
  const unstable = [];
  const names = new Set([...Object.keys(cargo.tests), ...Object.keys(rawr.tests)]);
  for (const test of [...names].sort()) {
    if (!rawr.ran) break;
    const ours = cargo.tests[test];
    const theirs = rawr.tests[test];
    if (ours !== theirs) {
      differences.push(`${test}: cargo ${ours ?? "absent"}, rawr ${theirs ?? "absent"}`);
      continue;
    }
    const detail = cargo.details[test];
    if (detail === rawr.details[test]) continue;
    if (detail !== again.details[test]) {
      unstable.push(test);
      continue;
    }
    differences.push(`${test}: cargo \`${detail}\`, rawr \`${rawr.details[test] ?? "nothing"}\``);
  }

  const notes = [];
  if (unstable.length > 0) notes.push(`frame varies between cargo runs: ${unstable.join(", ")}`);
  const failing = Object.entries(emulator.tests).filter(([, status]) => status === "FAILED");
  if (failing.length > 0) notes.push(`fails under cargo too: ${failing.map(([t]) => t).join(", ")}`);
  const verdict = differences.length === 0 ? "same" : `MISMATCH: ${differences.join("; ")}`;
  const rawrText =
    emulated === null || emulated.ran
      ? describe(rawr)
      : `${describe(rawr)} (emulator half: build fails)`;
  return {
    name,
    cargo: describe(emulator),
    rawr: rawrText,
    verdict: notes.length > 0 ? `${verdict} (${notes.join("; ")})` : verdict,
    agrees: differences.length === 0,
    differences,
    cargoDetails: cargo.details,
    rawrDetails: rawr.details,
    seconds,
  };
}

// Deletes the example's rawr project build under the shared target, whose
// store keeps every build's execution (a few hundred MB each), when this run
// made it; a build that was there before the run stays.
function dropRawrBuild(name) {
  for (const entry of rawrProjects()) {
    if (!projectsBefore.has(entry) && /^(.+)-[0-9a-f]{16}$/.exec(entry)?.[1] === name) {
      fs.rmSync(path.join(rawrProjectsDir, entry), { recursive: true, force: true });
    }
  }
}

function rawrProjects() {
  return fs.existsSync(rawrProjectsDir) ? fs.readdirSync(rawrProjectsDir) : [];
}

function freeGb() {
  const stats = fs.statfsSync(source);
  return (stats.bavail * stats.bsize) / 1024 ** 3;
}
