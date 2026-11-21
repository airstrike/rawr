// /web/…: rawr-web's sets, served from the R2 bucket bound as WEB
// (wrangler.jsonc). upload-web.mjs writes each set's files under
// web/<version>/ and then web/current.json, the pointer to the set the site
// runs (site/src/scripts/web.ts has its format). A request's path is its
// key with the leading slash:
//
// - /web/current.json, the pointer. It revalidates on every use
//   (`no-cache`, with the object's ETag), so a page sees a new set at once.
// - /web/<version>/<file>, a file of one set. A set never changes once
//   written, so these are immutable: a page that resolved the pointer loads
//   its modules from one set, cached for good.
// - /web/<file>, that file of the set the pointer names now, revalidated
//   like the pointer, for clients that ask by name (live-web.sh, a page
//   built before the pointer).
//
// A module (.wasm) has a brotli copy beside it (web/build.sh), which a
// browser that takes brotli gets as is, about a quarter smaller than the
// edge's own compression makes it. A module stored only as its brotli copy
// (the universe's) goes out as that copy to every client. A file asked for
// by its own name, such as universe_bg.wasm.br, goes out as stored.

import { POINTER, VERSION, parsePointer, type Pointer } from "../../../../site/src/scripts/web.ts";

/** What webAsset reads of the bucket: the binding's `get`. */
export interface WebBucket {
  get(key: string, options?: R2GetOptions): Promise<R2ObjectBody | R2Object | null>;
}

const TYPES: Record<string, string> = {
  ".wasm": "application/wasm",
  ".js": "text/javascript; charset=utf-8",
  ".json": "application/json",
  ".toml": "text/plain; charset=utf-8",
};

/** A set's file's content type, by its name: SHA256SUMS is text, and any other file plain bytes. */
export function webType(name: string): string {
  if (name === "SHA256SUMS") return "text/plain; charset=utf-8";
  const dot = name.lastIndexOf(".");
  return (dot >= 0 && TYPES[name.slice(dot)]) || "application/octet-stream";
}

/** A versioned file's caching: it never changes. */
export const IMMUTABLE = "public, max-age=31536000, immutable";
/** The pointer's and the current set's: used from the cache only after the ETag is checked. */
export const REVALIDATE = "no-cache";

const plain = (body: string, status: number) =>
  new Response(body + "\n", { status, headers: { "content-type": "text/plain; charset=utf-8", "cache-control": "no-store" } });

/** The set the pointer names, or the response that says why there is none. */
async function current(bucket: WebBucket): Promise<Pointer | Response> {
  const object = await bucket.get(POINTER.slice(1));
  if (!object || !("body" in object)) return plain(`no rawr-web set is published: the bucket has no ${POINTER.slice(1)}`, 404);
  const pointer = parsePointer(await object.json().catch(() => null));
  return pointer ?? plain(`rawr-web's pointer (${POINTER.slice(1)}) is not one`, 500);
}

/** `object`, the file `name`, as the answer to `request`; `packed` when it is the brotli copy of `name`. */
function answer(request: Request, object: R2ObjectBody | R2Object, name: string, cache: string, packed: boolean): Response {
  const headers = new Headers({ "content-type": webType(name), "cache-control": cache, etag: object.httpEtag });
  if (packed) {
    headers.set("content-encoding", "br");
    headers.set("vary", "accept-encoding");
  }
  // A conditional request whose ETag matched: R2 answers without the body.
  if (!("body" in object)) return new Response(null, { status: 304, headers });
  if (request.method === "HEAD") {
    object.body.cancel();
    return new Response(null, { headers });
  }
  // `manual`: the brotli copy goes out as stored, never encoded again.
  return new Response(object.body, { headers, encodeBody: packed ? "manual" : "automatic" } as ResponseInit);
}

/** Serve a request under /web from the bucket. */
export async function webAsset(request: Request, bucket: WebBucket): Promise<Response> {
  if (request.method !== "GET" && request.method !== "HEAD") return plain("method not allowed", 405);
  const path = new URL(request.url).pathname;
  const parts = path.slice("/web/".length).split("/");
  let version: string;
  let name: string;
  let cache: string;
  if (path === POINTER) {
    const object = await bucket.get(path.slice(1), { onlyIf: request.headers });
    if (!object) return plain(`no rawr-web set is published: the bucket has no ${path.slice(1)}`, 404);
    return answer(request, object, "current.json", REVALIDATE, false);
  } else if (parts.length === 2 && VERSION.test(parts[0]) && parts[1] !== "") {
    [version, name] = parts;
    cache = IMMUTABLE;
  } else if (parts.length === 1 && parts[0] !== "") {
    const pointer = await current(bucket);
    if (pointer instanceof Response) return pointer;
    [version, name, cache] = [pointer.version, parts[0], REVALIDATE];
  } else {
    return plain(`no rawr-web file ${path}`, 404);
  }
  const brotli = /\bbr\b/.test(request.headers.get("accept-encoding") ?? "");
  const tries = !name.endsWith(".wasm") ? [name] : brotli ? [name + ".br", name] : [name, name + ".br"];
  for (const file of tries) {
    const object = await bucket.get(`web/${version}/${file}`, { onlyIf: request.headers });
    if (object) return answer(request, object, name, cache, file !== name);
  }
  return plain(`no rawr-web file ${path}`, 404);
}
