#!/usr/bin/env node
// Checks that iced's examples behave the same under `cargo test` and `rawr test`.
//
// Usage: node scripts/iced-parity.mjs [example ...]
//
// The examples are the standalone crates scripts/iced-examples.sh makes under
// target/scratch/iced-examples (ICED_PARITY_SRC overrides). For each one this
// copies the crate to target/iced-parity/<name> (ICED_PARITY_WORK overrides),
// never touching the original, and appends `#[cfg(test)] mod parity;`, whose
// file src/parity.rs it writes from scripts/iced-parity:
//
// - `sim_<name>` replays <name>.ice with iced_test's Simulator: each run of
//   interactions goes to a simulator of the example's `view`, its messages go
//   to `update`, and each `expect` looks for its text in a fresh view. Tasks
//   and subscriptions don't run. It hashes the final frame through
//   `Snapshot::matches_hash`.
// - `ice_<name>`, behind the copy's `iced-parity-emulator` feature, replays the
//   same script through iced_test's Emulator, which runs the whole program
//   (tasks, subscriptions) from the `iced::application(..)` chain in `main`, as
//   `iced_test::run` does, and hashes its final screenshot.
// - <name>.rs, when present, is appended to the module: hand-written tests for
//   what the script can't express, or a `program()` the chain can't give.
//
// Both draw headless with tiny-skia (ICED_TEST_BACKEND), so no window opens and
// no GPU is used. cargo runs every test twice with the emulator feature; rawr
// runs them once, and once more without the feature when the emulator half
// doesn't build. A hash counts only when both cargo runs agree on it, so frames
// that show the clock or a random seed don't count as mismatches.
//
// Every copy shares one target directory, target/iced-parity/_target, for
// cargo and for rawr's project builds. After each example the script deletes
// that example's test binaries and rawr project build; when free disk falls
// under ICED_PARITY_MIN_FREE_GB (default 10) it deletes the shared target.
//
// The tests run with HOME and the XDG directories inside the copy, so an
// example that saves state writes there, not to the user's data directory.
//
// Prints one row per example, writes report.json beside the copies, and exits
// 1 when any example's runners disagree.

import { spawnSync, execFileSync } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";

const here = path.dirname(new URL(import.meta.url).pathname);
const scripts = path.join(here, "iced-parity");
const checkout = path.resolve(
  execFileSync("git", ["-C", here, "rev-parse", "--path-format=absolute", "--git-common-dir"], {
    encoding: "utf8",
  }).trim(),
  "..",
);
const source = process.env.ICED_PARITY_SRC ?? path.join(checkout, "target/scratch/iced-examples");
const work = process.env.ICED_PARITY_WORK ?? path.join(checkout, "target/iced-parity");
const targetDir = path.join(work, "_target");
const minFreeGb = Number(process.env.ICED_PARITY_MIN_FREE_GB ?? 10);
const icedRev = "be88873d38544455e26c7a104eb757da2cbbac6b";
const feature = "iced-parity-emulator";
const cargoHome = process.env.CARGO_HOME ?? path.join(os.homedir(), ".cargo");
const rustupHome = process.env.RUSTUP_HOME ?? path.join(os.homedir(), ".rustup");

const examples = process.argv.length > 2 ? process.argv.slice(2) : allExamples();
// Cargo builds for the host named as a target, as rawr builds a project that
// imports Rust, so the two runners share their dependency units.
fs.mkdirSync(path.join(work, ".cargo"), { recursive: true });
fs.writeFileSync(path.join(work, ".cargo/config.toml"), '[build]\ntarget = "host-tuple"\n');

const rows = [];
let disagreements = 0;
console.log(row(["example", "cargo", "rawr", "verdict"]));
console.log(row(["---", "---", "---", "---"]));
for (const name of examples) {
  const result = check(name);
  rows.push(result);
  if (!result.agrees) {
    disagreements += 1;
  }
  console.log(row([name, result.cargo, result.rawr, result.verdict]));
  clean(name);
  fs.writeFileSync(path.join(work, "report.json"), JSON.stringify(rows, null, 2) + "\n");
}
console.log(
  `\n${examples.length - disagreements} of ${examples.length} agree; logs in ${work}/<example>/logs`,
);
process.exit(disagreements > 0 ? 1 : 0);

function allExamples() {
  return fs
    .readdirSync(source)
    .filter((name) => fs.existsSync(path.join(source, name, "Cargo.toml")))
    .sort();
}

function row(cells) {
  return `| ${cells.join(" | ")} |`;
}

// Copies the example and writes its parity modules; returns the copy's path.
function prepare(name) {
  const dir = path.join(work, name);
  fs.rmSync(dir, { recursive: true, force: true });
  fs.cpSync(path.join(source, name), dir, {
    recursive: true,
    filter: (from) => path.basename(from) !== "target",
  });

  const manifest = path.join(dir, "Cargo.toml");
  fs.writeFileSync(manifest, withParityManifest(fs.readFileSync(manifest, "utf8")));

  const iceFile = path.join(scripts, `${name}.ice`);
  const ice = fs.existsSync(iceFile) ? parseIce(fs.readFileSync(iceFile, "utf8")) : null;
  if (ice !== null) {
    fs.mkdirSync(path.join(dir, "parity"));
    fs.copyFileSync(iceFile, path.join(dir, "parity", `${name}.ice`));
  }
  const extraFile = path.join(scripts, `${name}.rs`);
  const extra = fs.existsSync(extraFile) ? fs.readFileSync(extraFile, "utf8") : "";
  const ownProgram = /\bfn program\(\)/.test(extra);

  const main = path.join(dir, "src/main.rs");
  const mainText = fs.readFileSync(main, "utf8");
  const expression = programExpression(mainText);

  let parity = "// rawr\n// Generated by scripts/iced-parity.mjs.\nuse crate::*;\n\n";
  parity += fs.readFileSync(path.join(scripts, "simulator.rs"), "utf8");
  const parts = expression === null ? null : applicationParts(expression, mainText);
  if (ice !== null && parts !== null) {
    parity += simulatorTest(`sim_${name}`, ice, parts);
  }
  if (extra !== "") {
    parity += `\n${extra.replace(/^\/\/ rawr\n/, "")}`;
  }
  fs.writeFileSync(path.join(dir, "src/parity.rs"), parity);

  let emulated = "// rawr\n// Generated by scripts/iced-parity.mjs.\nuse crate::*;\n\n";
  emulated += fs.readFileSync(path.join(scripts, "emulator.rs"), "utf8");
  if (ownProgram) {
    emulated += "\nuse crate::parity::program;\n";
  } else if (expression !== null) {
    emulated +=
      `\nfn program() -> impl Program<Theme = iced_test::core::Theme> + 'static {\n` +
      `    ${expression}\n}\n`;
  }
  if (ice !== null && (ownProgram || expression !== null)) {
    const test = `ice_${name}`;
    const preset = ice.preset === null ? "" : `preset ${JSON.stringify(ice.preset)}, `;
    emulated +=
      `\n#[test]\nfn ${test}() {\n` +
      `    let app = program();\n` +
      `    let outcome = replay!(app, ${preset}include_str!("../parity/${name}.ice"));\n` +
      `    outcome.record("${test}");\n` +
      `    outcome.assert_passed();\n}\n`;
  }
  fs.writeFileSync(path.join(dir, "src/parity_emulator.rs"), emulated);

  // Both modules are declared in the crate root. `rawr test` 0.3.214 ignores
  // `#[path]` on a module declared in src/parity.rs and opens
  // src/parity/<name>.rs instead.
  fs.writeFileSync(
    main,
    `${mainText.replace(/\n*$/, "\n")}\n#[cfg(test)]\n#[path = "parity.rs"]\nmod parity;\n` +
      `\n#[cfg(all(test, feature = "${feature}"))]\n#[path = "parity_emulator.rs"]\nmod parity_emulator;\n`,
  );
  return dir;
}

// Adds iced_test as a dev-dependency at the examples' iced revision, and the
// feature that turns on the emulator tests.
function withParityManifest(manifest) {
  let text = manifest.replace(/\n*$/, "\n");
  if (!/^iced_test\s*=/m.test(text)) {
    const line = `iced_test = { git = "https://github.com/iced-rs/iced", rev = "${icedRev}" }`;
    text = /^\[dev-dependencies\]$/m.test(text)
      ? text.replace(/^\[dev-dependencies\]$/m, `[dev-dependencies]\n${line}`)
      : `${text}\n[dev-dependencies]\n${line}\n`;
  }
  return /^\[features\]$/m.test(text)
    ? text.replace(/^\[features\]$/m, `[features]\n${feature} = []`)
    : `${text}\n[features]\n${feature} = []\n`;
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
    `    if let Err(error) = frame.matches_hash("parity-snapshots/${test}") {\n` +
    `        panic!("the copy's parity-snapshots directory is writable: {error}");\n` +
    `    }\n` +
    `    println!("iced-parity ${test} passed");\n`;
  return `\n#[test]\nfn ${test}() {\n${body}}\n`;
}

// The headless renderer the copy's tests draw with: tiny-skia, unless its
// extra Rust names another with an `// iced-parity: backend <name>` line, for
// an example whose widgets draw only with wgpu.
function backend(dir) {
  const extra = path.join(dir, "src/parity.rs");
  const named = /^\/\/ iced-parity: backend (\S+)$/m.exec(fs.readFileSync(extra, "utf8"));
  return named === null ? "tiny-skia" : named[1];
}

function testEnv(dir) {
  const home = path.join(dir, "home");
  fs.mkdirSync(home, { recursive: true });
  // ICED_THEME would pick the default theme; the parity module draws a
  // program without one in `Theme::Light`, the default while it is unset.
  const { ICED_THEME, ...inherited } = process.env;
  return {
    ...inherited,
    CARGO_HOME: cargoHome,
    RUSTUP_HOME: rustupHome,
    CARGO_TARGET_DIR: targetDir,
    HOME: home,
    XDG_DATA_HOME: path.join(home, ".local/share"),
    XDG_CONFIG_HOME: path.join(home, ".config"),
    XDG_CACHE_HOME: path.join(home, ".cache"),
    XDG_STATE_HOME: path.join(home, ".local/state"),
    ICED_TEST_BACKEND: backend(dir),
  };
}

// Runs one runner in the copy and reads each test's outcome from its output.
function runTests(dir, label, command, args) {
  const snapshots = path.join(dir, "parity-snapshots");
  fs.rmSync(snapshots, { recursive: true, force: true });
  // libtest prints a line per test with `--format pretty`, which cargo's `-q`
  // would otherwise make terse; `rawr test` prints those lines and takes no
  // `--format`.
  const options = command === "cargo" ? ["--format", "pretty", "--nocapture"] : ["--nocapture"];
  const full = [...args, "--", ...options];
  const result = spawnSync(command, full, {
    cwd: dir,
    env: testEnv(dir),
    encoding: "utf8",
    maxBuffer: 256 * 1024 * 1024,
  });
  const output = `${result.stdout}\n${result.stderr}`;
  fs.mkdirSync(path.join(dir, "logs"), { recursive: true });
  fs.writeFileSync(
    path.join(dir, "logs", `${label}.txt`),
    `$ ${command} ${full.join(" ")}\nexit ${result.status}\n${output}`,
  );

  // With `--nocapture` a test's own line can land between libtest's
  // `test name ... ` and its status, so the status is read with it removed.
  const statuses = output.replace(/iced-parity [^\n]*\n/g, "");
  const tests = {};
  for (const match of statuses.matchAll(/^test (\S+) \.\.\. (ok|FAILED|ignored)\b/gm)) {
    tests[match[1].replace(/^.*::/, "")] = match[2];
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
  const ran = /test result: /.test(output);
  const error = ran ? null : firstError(output);
  return { ran, status: result.status, tests, details, error };
}

// The first error a failed build reports, with its location.
function firstError(output) {
  const lines = output.split("\n");
  const at = lines.findIndex((line) => /^\s*error(\[E\d+\])?:/.test(line) && !/rawr could not build/.test(line));
  if (at < 0) {
    return `exit with no error line`;
  }
  const where = lines.slice(at + 1, at + 3).find((line) => line.includes("-->"));
  return `${lines[at].trim()}${where ? ` (${where.trim().replace(/^--> /, "")})` : ""}`;
}

function describe(run, filter = () => true) {
  if (!run.ran) {
    return "build fails";
  }
  const counts = { ok: 0, FAILED: 0, ignored: 0 };
  for (const [test, status] of Object.entries(run.tests)) {
    if (filter(test)) counts[status] += 1;
  }
  let text = `${counts.ok} ok`;
  if (counts.FAILED > 0) text += `, ${counts.FAILED} failed`;
  if (counts.ignored > 0) text += `, ${counts.ignored} ignored`;
  return text;
}

function check(name) {
  const dir = prepare(name);
  const cargo = runTests(dir, "cargo", "cargo", ["test", "-q", "--features", feature]);
  const again = runTests(dir, "cargo-again", "cargo", ["test", "-q", "--features", feature]);
  const emulated = runTests(dir, "rawr", "rawr", ["test", "--features", feature]);
  const rawr = emulated.ran ? emulated : runTests(dir, "rawr-simulator", "rawr", ["test"]);
  const compared = emulated.ran ? () => true : (test) => !test.startsWith("ice_");

  const differences = [];
  if (!cargo.ran) {
    // The parity module must build under cargo; a failure there is the
    // harness's fault, not a difference between the runners.
    return { name, cargo: "build fails", rawr: "-", verdict: `HARNESS: ${cargo.error}`, differences };
  }
  if (!emulated.ran) {
    differences.push(`emulator half doesn't build: ${emulated.error}`);
  }
  if (!rawr.ran) {
    differences.push(`simulator half doesn't build: ${rawr.error}`);
  }
  const unstable = [];
  const names = new Set([...Object.keys(cargo.tests), ...Object.keys(rawr.tests)]);
  for (const test of [...names].filter(compared).sort()) {
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

  let verdict = differences.length === 0 ? "same" : `MISMATCH: ${differences.join("; ")}`;
  const notes = [];
  if (unstable.length > 0) notes.push(`frame varies between cargo runs: ${unstable.join(", ")}`);
  const cargoFailed = Object.entries(cargo.tests).filter(([, status]) => status === "FAILED");
  if (cargoFailed.length > 0) notes.push(`fails under cargo too: ${cargoFailed.map(([t]) => t).join(", ")}`);
  const rawrText = emulated.ran
    ? describe(rawr)
    : `${describe(rawr, compared)} (emulator half: build fails)`;
  return {
    name,
    cargo: describe(cargo),
    rawr: rawrText,
    verdict: notes.length > 0 ? `${verdict} (${notes.join("; ")})` : verdict,
    agrees: differences.length === 0,
    differences,
    cargoDetails: cargo.details,
    rawrDetails: rawr.details,
  };
}

// Deletes the build output the example no longer needs.
function clean(name) {
  const dir = path.join(work, name);
  fs.rmSync(path.join(dir, "target"), { recursive: true, force: true });
  const projects = path.join(targetDir, "rawr", "projects");
  if (fs.existsSync(projects)) {
    for (const entry of fs.readdirSync(projects)) {
      if (entry.startsWith(`${name}-`)) {
        fs.rmSync(path.join(projects, entry), { recursive: true, force: true });
      }
    }
  }
  const crate = name.replace(/-/g, "_");
  for (const profile of profileDirs()) {
    for (const sub of ["deps", "incremental", ".fingerprint", "build"]) {
      const at = path.join(profile, sub);
      if (!fs.existsSync(at)) continue;
      for (const entry of fs.readdirSync(at)) {
        if (entry.startsWith(`${crate}-`) || entry.startsWith(`lib${crate}-`)) {
          fs.rmSync(path.join(at, entry), { recursive: true, force: true });
        }
      }
    }
  }
  if (freeGb() < minFreeGb) {
    console.error(`free disk under ${minFreeGb} GB: deleting ${targetDir}`);
    fs.rmSync(targetDir, { recursive: true, force: true });
  }
}

// Every `debug` profile directory in the shared target, host-named or not.
function profileDirs() {
  if (!fs.existsSync(targetDir)) {
    return [];
  }
  const dirs = [path.join(targetDir, "debug")];
  for (const entry of fs.readdirSync(targetDir)) {
    dirs.push(path.join(targetDir, entry, "debug"));
  }
  return dirs.filter((dir) => fs.existsSync(dir));
}

function freeGb() {
  const stats = fs.statfsSync(work);
  return (stats.bavail * stats.bsize) / 1024 ** 3;
}
