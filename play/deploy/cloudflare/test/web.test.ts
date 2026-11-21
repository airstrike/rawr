// node --test test/web.test.ts
// /web as the Worker serves it (src/web.ts), in workerd with a local R2
// bucket: Miniflare, which comes with wrangler, runs src/web.ts bundled by
// esbuild, which comes with it too. The pointer, a versioned file, the
// current set by name, content types and caching, brotli copies, misses,
// and the page's protocol check (site/src/scripts/web.ts) against it.
import assert from "node:assert/strict";
import { after, before, test } from "node:test";
import { brotliCompressSync } from "node:zlib";
import { build } from "esbuild";
import * as miniflare from "miniflare";
import { Miniflare } from "miniflare";
import { WEB_PROTOCOL, webFiles } from "../../../../site/src/scripts/web.ts";

const SITE = "https://rawr.airstrike.fyi";
const OLD = "0123456789abcdef";
const NEW = "fedcba9876543210";

let mf: Miniflare;
let bucket: Awaited<ReturnType<Miniflare["getR2Bucket"]>>;
let worker: Awaited<ReturnType<Miniflare["getWorker"]>>;

before(async () => {
  const bundled = await build({
    stdin: {
      contents: `import { webAsset } from "./src/web.ts";
export default { fetch: (request, env) => webAsset(request, env.WEB) };`,
      resolveDir: new URL("..", import.meta.url).pathname,
      loader: "ts",
    },
    bundle: true,
    format: "esm",
    write: false,
  });
  const options = {
    modules: true,
    script: bundled.outputFiles[0].text,
    compatibilityDate: "2026-10-01",
    r2Buckets: ["WEB"],
  };
  // Miniflare 5 takes its own options and converts these, Miniflare 4's.
  const convert = (miniflare as { convertV4MiniflareOptions?: (options: object) => object }).convertV4MiniflareOptions;
  mf = new Miniflare((convert ? convert(options) : options) as ConstructorParameters<typeof Miniflare>[0]);
  bucket = await mf.getR2Bucket("WEB");
  worker = await mf.getWorker();
});

after(() => mf.dispose());

// The Worker itself: dispatchFetch would re-encode the answer to the
// request's first accepted encoding, as an edge might.
const get = (path: string, headers: Record<string, string> = {}, method = "GET") =>
  worker.fetch(SITE + path, { headers, method });

/** The encoding the Worker sent: Miniflare decodes the body and names it mf-content-encoding. */
const sent = (response: { headers: Headers }) =>
  response.headers.get("content-encoding") ?? response.headers.get("mf-content-encoding");

/** A set under `version`: rawr-web with its brotli copy, and the universe's module as its copy only. */
async function upload(version: string, tag: string) {
  const put = (name: string, body: string | Uint8Array) => bucket.put(`web/${version}/${name}`, body);
  await put("rawr_web.js", `glue ${tag}`);
  await put("rawr_web_bg.wasm", `wasm ${tag}`);
  await put("rawr_web_bg.wasm.br", brotliCompressSync(`wasm ${tag}`));
  await put("universe_bg.wasm.br", brotliCompressSync(`universe ${tag}`));
  await put("universe.toml", "[dependencies]\n");
  await put("SHA256SUMS", "hashes\n");
}

const point = (version: string, protocol = WEB_PROTOCOL) =>
  bucket.put("web/current.json", JSON.stringify({ version, commit: "abc", protocol, uploaded: "2026-11-20T00:00:00Z" }));

test("with no pointer, a file by name is a 404 that says so", async () => {
  const missing = await get("/web/current.json");
  assert.equal(missing.status, 404);
  assert.match(await missing.text(), /no rawr-web set is published/);
  const byName = await get("/web/rawr_web.js");
  assert.equal(byName.status, 404);
  assert.equal(byName.headers.get("content-type"), "text/plain; charset=utf-8");
  assert.match(await byName.text(), /no rawr-web set is published: the bucket has no web\/current\.json/);
});

test("the pointer is JSON that revalidates by its ETag", async () => {
  await upload(OLD, "old");
  await point(OLD);
  const pointer = await get("/web/current.json");
  assert.equal(pointer.status, 200);
  assert.equal(pointer.headers.get("content-type"), "application/json");
  assert.equal(pointer.headers.get("cache-control"), "no-cache");
  assert.equal((await pointer.json()).version, OLD);
  const etag = pointer.headers.get("etag")!;
  assert.ok(etag);
  const again = await get("/web/current.json", { "if-none-match": etag });
  assert.equal(again.status, 304);
  assert.equal(await again.text(), "");
});

test("a versioned file is immutable, with its type", async () => {
  const glue = await get(`/web/${OLD}/rawr_web.js`);
  assert.equal(glue.status, 200);
  assert.equal(await glue.text(), "glue old");
  assert.equal(glue.headers.get("content-type"), "text/javascript; charset=utf-8");
  assert.equal(glue.headers.get("cache-control"), "public, max-age=31536000, immutable");
  const manifest = await get(`/web/${OLD}/universe.toml`);
  assert.equal(manifest.headers.get("content-type"), "text/plain; charset=utf-8");
  const sums = await get(`/web/${OLD}/SHA256SUMS`);
  assert.equal(sums.headers.get("content-type"), "text/plain; charset=utf-8");
  const etag = glue.headers.get("etag")!;
  assert.equal((await get(`/web/${OLD}/rawr_web.js`, { "if-none-match": etag })).status, 304);
  const head = await get(`/web/${OLD}/rawr_web.js`, {}, "HEAD");
  assert.equal(head.status, 200);
  assert.equal(await head.text(), "");
});

test("a browser that takes brotli gets a module's brotli copy as is", async () => {
  // fetch decodes the body; the header says what came over the wire.
  const packed = await get(`/web/${OLD}/rawr_web_bg.wasm`, { "accept-encoding": "gzip, deflate, br, zstd" });
  assert.equal(sent(packed), "br");
  assert.equal(packed.headers.get("content-type"), "application/wasm");
  assert.equal(await packed.text(), "wasm old");
  const raw = await get(`/web/${OLD}/rawr_web_bg.wasm`, { "accept-encoding": "gzip" });
  assert.equal(sent(raw), null);
  assert.equal(raw.headers.get("content-type"), "application/wasm");
  assert.equal(await raw.text(), "wasm old");
});

test("a module stored only as its brotli copy goes out as that copy to every client", async () => {
  for (const encoding of ["gzip, deflate, br, zstd", "gzip", ""]) {
    const served = await get(`/web/${OLD}/universe_bg.wasm`, { "accept-encoding": encoding });
    assert.equal(served.status, 200);
    assert.equal(sent(served), "br");
    assert.equal(served.headers.get("content-type"), "application/wasm");
    assert.equal(await served.text(), "universe old");
  }
  // live-web.sh fetches the copy itself by its name, as stored.
  const copy = await get(`/web/${OLD}/universe_bg.wasm.br`);
  assert.equal(sent(copy), null);
  assert.equal(copy.headers.get("content-type"), "application/octet-stream");
  assert.deepEqual(new Uint8Array(await copy.arrayBuffer()), new Uint8Array(brotliCompressSync("universe old")));
});

test("a file by name is the current set's, and follows the pointer", async () => {
  const before = await get("/web/rawr_web.js");
  assert.equal(await before.text(), "glue old");
  assert.equal(before.headers.get("cache-control"), "no-cache");
  await upload(NEW, "new");
  await point(NEW);
  assert.equal(await (await get("/web/rawr_web.js")).text(), "glue new");
  assert.equal(await (await get("/web/universe_bg.wasm", { "accept-encoding": "br" })).text(), "universe new");
  // The old set stays, for a page that resolved the pointer before it moved.
  assert.equal(await (await get(`/web/${OLD}/rawr_web.js`)).text(), "glue old");
});

test("a missing file is a 404 that names it", async () => {
  for (const path of [`/web/${NEW}/missing.wasm`, `/web/${NEW}/missing.js`, "/web/missing.wasm", "/web/a/b/c", "/web/nothex/rawr_web.js", "/web/"]) {
    const missing = await get(path, { "accept-encoding": "br" });
    assert.equal(missing.status, 404, path);
    assert.equal(missing.headers.get("cache-control"), "no-store");
    assert.equal(await missing.text(), `no rawr-web file ${path}\n`);
  }
  assert.equal((await get(`/web/${NEW}/rawr_web.js`, {}, "POST")).status, 405);
});

test("a pointer that isn't one is a 500 that says so", async () => {
  await bucket.put("web/current.json", "{}");
  const broken = await get("/web/rawr_web.js");
  assert.equal(broken.status, 500);
  assert.match(await broken.text(), /pointer \(web\/current\.json\) is not one/);
  await point(NEW);
});

test("the page loads the set its pointer names, and refuses one of another protocol", async () => {
  const fetched: string[] = [];
  const fetch = (async (url: string) => (fetched.push(url), get(url))) as unknown as typeof globalThis.fetch;
  const files = webFiles(fetch);
  const glue = await files("rawr_web.js");
  assert.equal(glue, `/web/${NEW}/rawr_web.js`);
  assert.equal(await (await get(glue)).text(), "glue new");
  assert.equal(await files("universe.toml"), `/web/${NEW}/universe.toml`);
  assert.deepEqual(fetched, ["/web/current.json"], "the pointer is read once");

  await point(NEW, WEB_PROTOCOL + 1);
  fetched.length = 0;
  await assert.rejects(webFiles(fetch)("rawr_web.js"), {
    message: `the site's modules are rawr-web protocol ${WEB_PROTOCOL + 1}, newer than this page's protocol ${WEB_PROTOCOL}. Reload the page.`,
  });
  assert.deepEqual(fetched, ["/web/current.json"], "no module is fetched");
  await assert.rejects(webFiles(fetch, WEB_PROTOCOL + 2)("rawr_web.js"), /this page needs rawr-web protocol \d+, and the site's modules are still protocol \d+/);
  await point(NEW);
});
