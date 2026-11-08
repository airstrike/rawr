// How the build runs the site's snippets (docs.ts's runOutputs): in the
// rawr-web the page loads, and on rawr's server for a block marked
// `data-where="server"`. Node only.
//
// rawr-web's files are play/deploy/cloudflare/public/web, the ones the deploy
// ships. live-web.sh fetches the live ones and the Playground workflow builds
// new ones. Without them the build fails.
//
// A server block goes to POST /play/render, the playground's endpoint, which
// runs any source. /play/run would not do: it runs only the sources on the
// pages already deployed, and a new snippet is not among them. The request
// carries `x-play-admin`, as the deploy's smoke test does: PLAY_SMOKE_TOKEN,
// or else the admin HMAC of ~/.config/rawr-play/log-key. Without either the
// build fails. The answer comes from the server that is live while the site
// builds.
import { createHmac } from "node:crypto";
import { existsSync, readFileSync } from "node:fs";
import { homedir } from "node:os";
import { join, resolve } from "node:path";
import { pathToFileURL } from "node:url";
import type { Runs, Web } from "./docs.ts";
import type { Result } from "./output.ts";

const SITE = process.env.PLAY_SITE || "https://rawr.airstrike.fyi";
/** The site's build runs in site/. */
const WEB = resolve(process.cwd(), "../play/deploy/cloudflare/public/web");

let wasm: WebAssembly.Module | null = null;
let copies = 0;

/** A fresh rawr-web: its own import of the glue, so its own instance. */
async function web(): Promise<Web> {
  const glue = join(WEB, "rawr_web.js");
  const binary = join(WEB, "rawr_web_bg.wasm");
  if (!existsSync(glue) || !existsSync(binary)) {
    throw new Error(`rawr-web is not in ${WEB}; run play/deploy/cloudflare/live-web.sh, which fetches the live one`);
  }
  wasm ??= new WebAssembly.Module(readFileSync(binary));
  const m = await import(/* @vite-ignore */ `${pathToFileURL(glue).href}?copy=${++copies}`);
  m.initSync({ module: wasm });
  return m;
}

function admin(): string {
  if (process.env.PLAY_SMOKE_TOKEN) return process.env.PLAY_SMOKE_TOKEN;
  try {
    const key = readFileSync(join(homedir(), ".config/rawr-play/log-key"), "utf8").trim();
    return createHmac("sha256", key).update("rawr-play admin").digest("hex");
  } catch {
    throw new Error("a data-where=\"server\" snippet runs on rawr's server at build, which needs PLAY_SMOKE_TOKEN or ~/.config/rawr-play/log-key");
  }
}

async function server(code: string): Promise<Result> {
  const headers = { "content-type": "application/json", "x-play-admin": admin() };
  for (;;) {
    const response = await fetch(`${SITE}/play/render`, { method: "POST", headers, body: JSON.stringify({ code }) });
    const type = response.headers.get("content-type") || "";
    // A Run button reads the answer the same way (scripts/run.ts).
    if (response.ok && type.startsWith("application/json")) return response.json();
    if (response.status === 422) return { kind: "error", text: await response.text() };
    const text = (await response.text()).trim();
    if (response.status === 429) {
      const wait = Number(response.headers.get("retry-after") ?? /(\d+) s/.exec(text)?.[1] ?? 30);
      await new Promise((done) => setTimeout(done, (wait + 1) * 1000));
      continue;
    }
    const first = code.split("\n").find((line) => line.trim()) ?? "";
    throw new Error(`rawr's server answered ${response.status} to the snippet starting ${JSON.stringify(first)}: ${text.slice(0, 500)}`);
  }
}

export const runs: Runs = { web, server };
