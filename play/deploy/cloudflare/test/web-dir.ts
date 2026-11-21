// A directory of web/build.sh's output as the bucket src/web.ts reads, for
// the checks that serve /web from local files (browser.ts, debugger-page.ts):
// the directory is the one set, and the pointer names it with this
// checkout's protocol. Conditions are ignored: every get has its body.
import { createHash } from "node:crypto";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { WEB_PROTOCOL } from "../../../../site/src/scripts/web.ts";
import type { WebBucket } from "../src/web.ts";

export function dirBucket(dir: string): WebBucket {
  let sums = "";
  try {
    sums = readFileSync(join(dir, "SHA256SUMS"), "utf8");
  } catch {}
  const version = createHash("sha256").update(sums).digest("hex").slice(0, 16);
  const pointer = JSON.stringify({ version, commit: "", protocol: WEB_PROTOCOL });
  const object = (bytes: Uint8Array) => {
    const text = () => new TextDecoder().decode(bytes);
    return {
      httpEtag: `"${createHash("md5").update(bytes).digest("hex")}"`,
      body: new Response(bytes).body!,
      json: async () => JSON.parse(text()),
    };
  };
  return {
    get: async (key: string) => {
      if (key === "web/current.json") return object(new TextEncoder().encode(pointer)) as unknown as R2ObjectBody;
      const prefix = `web/${version}/`;
      if (!key.startsWith(prefix)) return null;
      try {
        return object(readFileSync(join(dir, key.slice(prefix.length)))) as unknown as R2ObjectBody;
      } catch {
        return null;
      }
    },
  };
}
