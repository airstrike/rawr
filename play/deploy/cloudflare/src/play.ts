// The web playground's pure parts: share ids, share validation and the
// per-IP rate limiter the PlayBot Durable Object keeps in memory, and the
// signed passes /todo keeps in a cookie.

/** Longest snippet, in bytes (the bot checks it again). */
export const MAX_CODE = 16 * 1024;
/** Largest stored share, in bytes of canonical JSON. */
export const MAX_SHARE = 64 * 1024;
export const ID_LENGTH = 10;
export const ID_PATTERN = /^[0-9A-Za-z]{10}$/;

/** What a share keeps: the code, nothing about who. */
export interface Snippet {
  code: string;
}

/** The snippet in a request body, or an error. Other fields are ignored. */
export function parseSnippet(body: unknown): Snippet | string {
  if (typeof body !== "object" || body === null) return "bad request";
  const b = body as Record<string, unknown>;
  if (typeof b.code !== "string" || b.code.trim() === "") return "write some code first";
  if (new TextEncoder().encode(b.code).length > MAX_CODE) return `the code is over ${MAX_CODE / 1024} KiB`;
  return { code: b.code };
}

/** The stored form: equal snippets are equal bytes. */
export function canonical(snippet: Snippet): string {
  return JSON.stringify({ code: snippet.code });
}

const DIGITS = "0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";

/** A share's id: the first 64 bits of the SHA-256 of its canonical form, in base 62. */
export async function shareId(text: string): Promise<string> {
  const digest = new Uint8Array(await crypto.subtle.digest("SHA-256", new TextEncoder().encode(text)));
  let n = 0n;
  for (const byte of digest.slice(0, 8)) n = (n << 8n) | BigInt(byte);
  let id = "";
  for (let i = 0; i < ID_LENGTH; i++) {
    id = DIGITS[Number(n % 62n)] + id;
    n /= 62n;
  }
  return id;
}

/** Hex HMAC-SHA256 of `text` under `key`. */
export async function hmac(key: string, text: string): Promise<string> {
  const k = await crypto.subtle.importKey(
    "raw",
    new TextEncoder().encode(key),
    { name: "HMAC", hash: "SHA-256" },
    false,
    ["sign"],
  );
  const mac = new Uint8Array(await crypto.subtle.sign("HMAC", k, new TextEncoder().encode(text)));
  return [...mac].map((b) => b.toString(16).padStart(2, "0")).join("");
}

/** How long a pass lasts: 30 days, in seconds. */
export const PASS_SECONDS = 30 * 24 * 60 * 60;

/**
 * A pass: its expiry in Unix seconds, a dot, and the HMAC of the expiry.
 * It names no one. /todo (todo.ts) derives its key from its password.
 */
export async function makePass(key: string, nowMs: number): Promise<string> {
  const expires = Math.floor(nowMs / 1000) + PASS_SECONDS;
  return `${expires}.${await hmac(key, `pass ${expires}`)}`;
}

/** True when `value` is a pass made under `key` that has not expired. */
export async function checkPass(key: string, value: string | null, nowMs: number): Promise<boolean> {
  if (!value) return false;
  const match = /^(\d{1,12})\.([0-9a-f]{64})$/.exec(value);
  if (!match) return false;
  const expires = Number(match[1]);
  if (expires * 1000 <= nowMs || expires > Math.floor(nowMs / 1000) + PASS_SECONDS) return false;
  return same(match[2], await hmac(key, `pass ${expires}`));
}

/** The bot's render request for `body`. */
export function renderRequest(body: string): Request {
  return new Request("http://container/render", {
    method: "POST",
    headers: { "content-type": "application/json" },
    body,
  });
}

/** The named cookie's value in a Cookie header, or null. */
export function cookie(header: string | null, name: string): string | null {
  for (const part of (header ?? "").split(";")) {
    const at = part.indexOf("=");
    if (at > 0 && part.slice(0, at).trim() === name) return part.slice(at + 1).trim();
  }
  return null;
}

/** Equal strings, compared in time that depends only on the length. */
export function same(a: string, b: string): boolean {
  if (a.length !== b.length) return false;
  let diff = 0;
  for (let i = 0; i < a.length; i++) diff |= a.charCodeAt(i) ^ b.charCodeAt(i);
  return diff === 0;
}

export interface Window {
  /** Requests allowed per window. */
  max: number;
  ms: number;
}

/** Sliding-window limits per key (a hashed IP), in memory. */
export class Limiter {
  private seen = new Map<string, number[]>();
  private readonly windows: Window[];
  private readonly maxKeys: number;
  private readonly longest: number;

  constructor(windows: Window[], maxKeys = 10_000) {
    this.windows = windows;
    this.maxKeys = maxKeys;
    this.longest = Math.max(...windows.map((w) => w.ms));
  }

  /** Count a request at `now` (ms); 0 if allowed, else seconds until it would be. */
  take(key: string, now: number): number {
    const times = (this.seen.get(key) ?? []).filter((t) => t > now - this.longest);
    let wait = 0;
    for (const w of this.windows) {
      const inside = times.filter((t) => t > now - w.ms);
      if (inside.length >= w.max) wait = Math.max(wait, inside[inside.length - w.max] + w.ms - now);
    }
    if (wait > 0) {
      this.seen.set(key, times);
      return Math.ceil(wait / 1000);
    }
    times.push(now);
    this.seen.delete(key);
    this.seen.set(key, times);
    if (this.seen.size > this.maxKeys) {
      const oldest = this.seen.keys().next().value;
      if (oldest !== undefined) this.seen.delete(oldest);
    }
    return 0;
  }
}

/** Shares: 10 a minute and 100 an hour per IP. */
export const SHARE_LIMITS: Window[] = [
  { max: 10, ms: 60_000 },
  { max: 100, ms: 3_600_000 },
];

/** Largest preview (a program's output) kept, in bytes. */
export const MAX_PREVIEW = 1024 * 1024;
/** How long a preview that failed for a passing reason (busy, unreachable) is not retried. */
export const RETRY_MS = 10 * 60_000;
/** Shares not visited for longer than this many days are purged. */
export const KEEP_DAYS = 30;

/** The first non-blank line of the code, trimmed, at most 150 characters. */
export function firstLine(code: string): string {
  const line = code.split("\n").map((l) => l.trim()).find((l) => l !== "") ?? "";
  return line.length > 150 ? line.slice(0, 149) + "…" : line;
}

/** The UTC day number of `now` (ms). */
export function day(now: number): number {
  return Math.floor(now / 86_400_000);
}

/**
 * One purge pass over shares and the day each was last visited (undefined:
 * never recorded): shares unvisited for more than KEEP_DAYS go, and shares
 * without a day are stamped with today rather than removed.
 */
export function purgeSelection(
  seen: Map<string, number | undefined>,
  today: number,
): { remove: string[]; stamp: string[] } {
  const remove: string[] = [];
  const stamp: string[] = [];
  for (const [id, last] of seen) {
    if (last === undefined) stamp.push(id);
    else if (today - last > KEEP_DAYS) remove.push(id);
  }
  return { remove, stamp };
}

export interface Og {
  title: string;
  description: string;
  url: string;
  image: string;
  alt: string;
  width?: number;
  height?: number;
}

/** Open Graph and Twitter card tags, escaped. */
export function ogTags(og: Og, escape: (text: string) => string): string {
  const meta = (attr: string, name: string, content: string) =>
    `<meta ${attr}="${name}" content="${escape(content)}">`;
  return [
    meta("property", "og:title", og.title),
    meta("property", "og:description", og.description),
    meta("property", "og:type", "website"),
    meta("property", "og:url", og.url),
    meta("property", "og:image", og.image),
    ...(og.width && og.height
      ? [meta("property", "og:image:width", String(og.width)), meta("property", "og:image:height", String(og.height))]
      : []),
    meta("property", "og:image:alt", og.alt),
    meta("name", "twitter:card", "summary_large_image"),
    meta("name", "twitter:title", og.title),
    meta("name", "twitter:description", og.description),
    meta("name", "twitter:image", og.image),
    meta("name", "description", og.description),
  ].join("\n");
}
/** One stream's run of a plain program's output. */
export interface Chunk {
  stream: "stdout" | "stderr";
  text: string;
}

/** A plain program's output, as rawr-play writes it. */
export interface TextResult {
  kind: "text";
  status: number;
  output: Chunk[];
  truncated: boolean;
  limit: boolean;
  /** A remark on the run, such as messages it ignored. */
  note?: string;
}

/** A text result read from JSON, or null. */
export function parseTextResult(json: string): TextResult | null {
  let value: unknown;
  try {
    value = JSON.parse(json);
  } catch {
    return null;
  }
  const v = value as Partial<TextResult> | null;
  if (!v || v.kind !== "text" || typeof v.status !== "number" || !Array.isArray(v.output)) return null;
  const output = v.output.filter(
    (c): c is Chunk => !!c && (c.stream === "stdout" || c.stream === "stderr") && typeof c.text === "string",
  );
  const note = typeof v.note === "string" ? { note: v.note } : {};
  return { kind: "text", status: v.status, output, truncated: v.truncated === true, limit: v.limit === true, ...note };
}

/** The first non-blank line a program wrote, at most 150 characters, for a link preview. */
export function firstOutputLine(result: TextResult): string {
  return firstLine(result.output.map((c) => c.text).join(""));
}
