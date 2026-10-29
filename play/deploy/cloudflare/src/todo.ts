// The work plan's private tabs (/todo#decisions, /todo#code). The documents
// are local and never in the repository, the site's build or this bundle:
// plan/private.mjs renders them to HTML and uploads them to the TODO_PRIVATE
// KV namespace. The Worker serves them only to a browser holding the unlock
// cookie.
//
//   POST /todo/unlock          {"password": "..."}: 204 and the cookie, 403, or 429
//   POST /todo/lock            clears the cookie
//   GET  /todo/private/<doc>   the document's HTML (decisions, code), or 401
//
// The password (the TODO_PASSWORD secret) is short, so it is checked here,
// never in the browser: in constant time, and limited per IP and in total by
// the Durable Object's limiter. The cookie holds no password: it is a pass
// (play.ts) signed with a key derived from the password and PLAY_LOG_KEY, so
// changing either ends every session. It is HttpOnly, Secure and
// SameSite=Strict, for /todo only.
import { PASS_SECONDS, checkPass, cookie, hmac, makePass, same, type Window } from "./play.ts";

export const TODO_COOKIE = "rawr_todo";
/** The KV keys of the private documents, by the name in /todo/private/<doc>. */
export const DOCS = ["decisions", "code"] as const;
export type Doc = (typeof DOCS)[number];

/** Password tries per IP: 5 a minute, 20 an hour, 50 a day. */
export const UNLOCK_LIMITS: Window[] = [
  { max: 5, ms: 60_000 },
  { max: 20, ms: 3_600_000 },
  { max: 50, ms: 86_400_000 },
];
/** Password tries from everyone together: 60 an hour, 300 a day. */
export const UNLOCK_TOTAL_LIMITS: Window[] = [
  { max: 60, ms: 3_600_000 },
  { max: 300, ms: 86_400_000 },
];

export interface TodoEnv {
  TODO_PASSWORD?: string;
  PLAY_LOG_KEY?: string;
  TODO_PRIVATE?: { get(key: string, type: "text"): Promise<string | null> };
}

export interface TodoDeps {
  /** Count a try from this request's IP; seconds to wait, or 0. */
  allow(request: Request): Promise<number>;
  now?: () => number;
}

/** The secret as set, without the trailing newline a file gives it. */
export const password = (value: string | undefined) => (value ?? "").replace(/\r?\n$/, "");

const noStore = { "cache-control": "no-store", "x-robots-tag": "noindex", "x-content-type-options": "nosniff" };
const text = (body: string, status: number, headers: Record<string, string> = {}) =>
  new Response(body, { status, headers: { "content-type": "text/plain; charset=utf-8", ...noStore, ...headers } });

/** The key unlock cookies are signed with. */
const cookieKey = (env: TodoEnv) => hmac(env.PLAY_LOG_KEY ?? "rawr-play", `rawr todo ${password(env.TODO_PASSWORD)}`);

export function todoCookie(value: string): string {
  return `${TODO_COOKIE}=${value}; Max-Age=${PASS_SECONDS}; Path=/todo; HttpOnly; Secure; SameSite=Strict`;
}
export const CLEAR_COOKIE = `${TODO_COOKIE}=; Max-Age=0; Path=/todo; HttpOnly; Secure; SameSite=Strict`;

/** True when the request holds an unexpired unlock cookie under the current password. */
export async function unlocked(request: Request, env: TodoEnv, now = Date.now()): Promise<boolean> {
  if (!password(env.TODO_PASSWORD)) return false;
  return checkPass(await cookieKey(env), cookie(request.headers.get("cookie"), TODO_COOKIE), now);
}

/** The response to a /todo/ path, or null for a path that isn't one of these. */
export async function todoRoute(request: Request, env: TodoEnv, path: string, deps: TodoDeps): Promise<Response | null> {
  const now = deps.now ?? Date.now;
  if (path === "/todo/unlock") {
    if (request.method !== "POST") return text("method not allowed", 405, { allow: "POST" });
    const secret = password(env.TODO_PASSWORD);
    if (!secret) return text("the private tabs are not set up", 503);
    const wait = await deps.allow(request);
    if (wait > 0) return text(`too many tries; try again in ${wait} s`, 429, { "retry-after": String(wait) });
    const body = await request.text();
    let given: unknown = null;
    try {
      given = body.length <= 1024 ? (JSON.parse(body) as { password?: unknown }).password : null;
    } catch {
      given = null;
    }
    if (typeof given !== "string") return text("send {\"password\": \"...\"}", 400);
    // Equal-length HMACs, so the comparison's time says nothing of the password.
    const key = await hmac(env.PLAY_LOG_KEY ?? "rawr-play", "rawr todo compare");
    if (!same(await hmac(key, password(given)), await hmac(key, secret))) return text("wrong password", 403);
    return new Response(null, { status: 204, headers: { ...noStore, "set-cookie": todoCookie(await makePass(await cookieKey(env), now())) } });
  }
  if (path === "/todo/lock") {
    if (request.method !== "POST") return text("method not allowed", 405, { allow: "POST" });
    return new Response(null, { status: 204, headers: { ...noStore, "set-cookie": CLEAR_COOKIE } });
  }
  const doc = /^\/todo\/private\/([a-z]+)$/.exec(path)?.[1];
  if (doc === undefined) return null;
  if (!(DOCS as readonly string[]).includes(doc)) return text("no such document", 404);
  if (request.method !== "GET" && request.method !== "HEAD") return text("method not allowed", 405, { allow: "GET" });
  if (!(await unlocked(request, env, now()))) return text("locked", 401);
  if (!env.TODO_PRIVATE) return text("the private tabs are not set up", 503);
  const html = await env.TODO_PRIVATE.get(doc, "text");
  if (html === null) return text("not uploaded yet; run plan/private.mjs", 404);
  return new Response(request.method === "HEAD" ? null : html, {
    headers: { "content-type": "text/html; charset=utf-8", ...noStore },
  });
}
