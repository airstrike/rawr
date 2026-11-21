// How the build runs the site's snippets (docs.ts's runOutputs): in the
// rawr-web the page loads. Node only.
//
// rawr-web's files are the set in target/web at the repository's root:
// play/deploy/cloudflare/live-web.sh fetches the one the site runs now, and
// web/build.sh target/web builds one. The build only reads them; the site
// serves its modules from R2 (scripts/web.ts). Without them the build fails.
import { existsSync, readFileSync } from "node:fs";
import { join, resolve } from "node:path";
import { pathToFileURL } from "node:url";
import type { Runs, Web } from "./docs.ts";

/** The site's build runs in site/. */
const WEB = resolve(process.cwd(), "../target/web");

let wasm: WebAssembly.Module | null = null;
let copies = 0;

/** A fresh rawr-web: its own import of the glue, so its own instance. */
async function web(): Promise<Web> {
  const glue = join(WEB, "rawr_web.js");
  const binary = join(WEB, "rawr_web_bg.wasm");
  if (!existsSync(glue) || !existsSync(binary)) {
    throw new Error(`rawr-web is not in ${WEB}; run play/deploy/cloudflare/live-web.sh, which fetches the set the site runs`);
  }
  wasm ??= new WebAssembly.Module(readFileSync(binary));
  const m = await import(/* @vite-ignore */ `${pathToFileURL(glue).href}?copy=${++copies}`);
  m.initSync({ module: wasm });
  return m;
}

export const runs: Runs = { web };
