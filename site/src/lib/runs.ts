// How the build runs the site's snippets (docs.ts's runOutputs): in the
// rawr-web the page loads. Node only.
//
// rawr-web's files are play/deploy/cloudflare/public/web, the ones the deploy
// ships. live-web.sh fetches the live ones and the Playground workflow builds
// new ones. Without them the build fails.
import { existsSync, readFileSync } from "node:fs";
import { join, resolve } from "node:path";
import { pathToFileURL } from "node:url";
import type { Runs, Web } from "./docs.ts";

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

export const runs: Runs = { web };
