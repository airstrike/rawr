// rawr-web's module in Node, as the page runs it:
//   web/build.sh && node web/tests/run.ts [WEB_DIR]
// Each snippet in web/tests/fixtures prints its `.stdout`, as rustc's build
// of it does (play/tests/cli.rs checks the native run against the same
// files). A program that panics leaves the module runnable, and a trap
// leaves a fresh copy of the module runnable, as the playground loads one.
import { readdirSync, readFileSync } from "node:fs";
import { join, resolve } from "node:path";
import { pathToFileURL } from "node:url";

const here = import.meta.dirname;
const web = resolve(process.argv[2] ?? join(here, "../../target/wasm/site"));
const fixtures = join(here, "fixtures");
const wasm = readFileSync(join(web, "rawr_web_bg.wasm"));

interface Web {
  initSync(options: { module: Buffer }): unknown;
  run_text(code: string): string;
}
interface Result {
  kind: string;
  status?: number;
  text?: string;
  output?: { stream: string; text: string }[];
}

let copies = 0;
/** A fresh copy of the module: its own instance, as a new import gives. */
const load = async (): Promise<Web> => {
  const url = pathToFileURL(join(web, "rawr_web.js")).href + "?" + ++copies;
  const m: Web = await import(url);
  m.initSync({ module: wasm });
  return m;
};
const run = (m: Web, code: string): Result => JSON.parse(m.run_text(code));
const stdout = (result: Result) =>
  (result.output ?? []).filter((chunk) => chunk.stream === "stdout").map((chunk) => chunk.text).join("");

let failed = false;
const check = (ok: boolean, what: string, detail = "") => {
  console.log(`${ok ? "ok" : "FAILED"}: ${what}${ok || !detail ? "" : "\n" + detail}`);
  failed ||= !ok;
};

let m = await load();
for (const name of readdirSync(fixtures).filter((file) => file.endsWith(".snippet")).sort()) {
  const code = readFileSync(join(fixtures, name), "utf8");
  const wanted = readFileSync(join(fixtures, name.replace(/\.snippet$/, ".stdout")), "utf8");
  let result: Result;
  try {
    result = run(m, code);
  } catch (error) {
    check(false, name, String(error));
    m = await load();
    continue;
  }
  check(result.kind === "text" && result.status === 0 && stdout(result) === wanted, name, JSON.stringify(result));
}

const panicked = run(m, 'let v: Vec<i32> = Vec::new();\nprintln!("{}", v[3]);');
check(panicked.status === 101, "a program that panics exits with 101", JSON.stringify(panicked));
check(stdout(run(m, 'println!("after");')) === "after\n", "and the module runs the next program");

// `std::thread::sleep` has no clock to wait on in the browser: it traps.
let trapped = false;
try {
  run(m, "std::thread::sleep(std::time::Duration::from_millis(1));");
} catch {
  trapped = true;
}
if (trapped) {
  const fresh = await load();
  check(stdout(run(fresh, 'println!("fresh");')) === "fresh\n", "a fresh copy runs after a trap");
}

process.exit(failed ? 1 : 0);
