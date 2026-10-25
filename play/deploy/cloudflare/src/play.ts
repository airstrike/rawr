// The web playground's pure parts: share ids, share validation and the
// per-IP rate limiter the PlayBot Durable Object keeps in memory.

/** Longest snippet, in bytes (the bot checks it again). */
export const MAX_CODE = 16 * 1024;
/** Largest stored share, in bytes of canonical JSON. */
export const MAX_SHARE = 64 * 1024;
export const MAX_MESSAGES = 32;
export const ID_LENGTH = 10;
export const ID_PATTERN = /^[0-9A-Za-z]{10}$/;

/** What a share keeps: the code and the run options, nothing about who. */
export interface Snippet {
  code: string;
  theme: "dark" | "light";
  size: string;
  messages: string[];
}

/** The options a `?play` line sets. Each is absent when the line doesn't name it. */
export interface PlayLine {
  theme?: "dark" | "light";
  size?: string;
  messages: string[];
}

/**
 * The options after `?play` on a line, read as the bot reads them. A token is
 * `size=WxH` or a bare `WxH`, `theme=dark|light` or a bare `dark` or `light`,
 * or `msg=EXPR`, which may repeat. An error names the token.
 */
export function playLine(line: string): PlayLine | string {
  const options: PlayLine = { messages: [] };
  for (const token of line.split(/\s+/).filter((t) => t !== "")) {
    const at = token.indexOf("=");
    const key = at < 0 ? "" : token.slice(0, at);
    const value = at < 0 ? token : token.slice(at + 1);
    if (key === "msg" || key === "message") options.messages.push(value);
    else if (key === "size" || (key === "" && /^\d/.test(value) && /x/i.test(value))) options.size = value;
    else if ((key === "theme" || key === "") && (value === "dark" || value === "light")) options.theme = value;
    else return `unknown ?play option \`${token}\`; expected size=WxH, theme=dark|light or msg=EXPR`;
  }
  return options;
}

/**
 * The snippet in a request body, normalized, or an error. The body names its
 * options as `theme`, `size` and `messages`, or as `play`, the text of a
 * `?play` line after the marker. The line's options apply after the fields.
 */
export function parseSnippet(body: unknown): Snippet | string {
  if (typeof body !== "object" || body === null) return "bad request";
  const b = body as Record<string, unknown>;
  if (typeof b.code !== "string" || b.code.trim() === "") return "write some code first";
  if (new TextEncoder().encode(b.code).length > MAX_CODE) return `the code is over ${MAX_CODE / 1024} KiB`;
  let theme: Snippet["theme"] = b.theme === "light" ? "light" : "dark";
  let size = typeof b.size === "string" ? b.size.trim() : "";
  const raw = b.messages ?? [];
  if (!Array.isArray(raw) || !raw.every((m) => typeof m === "string")) return "messages are text";
  let messages = (raw as string[]).map((m) => m.trim()).filter((m) => m !== "");
  if (b.play !== undefined) {
    if (typeof b.play !== "string") return "play options are text";
    const line = playLine(b.play);
    if (typeof line === "string") return line;
    theme = line.theme ?? theme;
    size = line.size ?? size;
    messages = [...messages, ...line.messages];
  }
  if (size !== "" && !/^\d{1,4}x\d{1,4}$/i.test(size)) return "size is WxH, like 400x300";
  if (messages.length > MAX_MESSAGES) return `at most ${MAX_MESSAGES} messages`;
  if (messages.some((m) => m.length > 256)) return "a message is over 256 characters";
  return { code: b.code, theme, size: size.toLowerCase(), messages };
}

/** The stored form: fixed key order, so equal snippets are equal bytes. */
export function canonical(snippet: Snippet): string {
  const { code, theme, size, messages } = snippet;
  return JSON.stringify({ code, theme, size, messages });
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

/** The cookie that lets a browser past Turnstile after one Siteverify. */
export const PASS_COOKIE = "rawr_play_pass";
/** How long a pass lasts: 30 days, in seconds. */
export const PASS_SECONDS = 30 * 24 * 60 * 60;

/**
 * A pass: its expiry in Unix seconds, a dot, and the HMAC of the expiry.
 * It names no one; the key is derived from the Turnstile secret.
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

/** The header that names a render's caller to the bot, for its animation allowance. */
export const USER_HEADER = "x-play-user";

/**
 * The bot's render request for `body`, naming its caller `user` (a hash of
 * the IP). The bot counts an animation's longer time against it.
 */
export function renderRequest(body: string, user: string): Request {
  return new Request("http://container/render", {
    method: "POST",
    headers: { "content-type": "application/json", [USER_HEADER]: user },
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

/** The Set-Cookie header that stores a pass for /play. */
export function passCookie(value: string): string {
  return `${PASS_COOKIE}=${value}; Max-Age=${PASS_SECONDS}; Path=/play; HttpOnly; Secure; SameSite=Lax`;
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

/** Playground renders: 6 a minute and 60 an hour per IP. */
export const RENDER_LIMITS: Window[] = [
  { max: 6, ms: 60_000 },
  { max: 60, ms: 3_600_000 },
];
/** Shares: 10 a minute and 100 an hour per IP. */
export const SHARE_LIMITS: Window[] = [
  { max: 10, ms: 60_000 },
  { max: 100, ms: 3_600_000 },
];

/** Previews: 10 uncached renders a minute and 60 an hour per IP. */
export const PREVIEW_LIMITS: Window[] = [
  { max: 10, ms: 60_000 },
  { max: 60, ms: 3_600_000 },
];
/** Largest preview PNG kept, in bytes. */
export const MAX_PREVIEW = 1024 * 1024;
/** How long a preview that failed for a passing reason (busy, unreachable) is not retried. */
export const RETRY_MS = 10 * 60_000;
/** Shares not visited for longer than this many days are purged. */
export const KEEP_DAYS = 30;

/** The width and height in a PNG's header, or null if it is not a PNG. */
export function pngSize(bytes: Uint8Array): { width: number; height: number } | null {
  const signature = [0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a];
  if (bytes.length < 24 || signature.some((b, i) => bytes[i] !== b)) return null;
  const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
  return { width: view.getUint32(16), height: view.getUint32(20) };
}

/** A share's render request asking for a still image: the last frame as a PNG, never a GIF. */
export function stillBody(share: string): string {
  return JSON.stringify({ ...JSON.parse(share), still: true });
}

/** The first non-blank line of the code, trimmed, at most 150 characters. */
export function firstLine(code: string): string {
  const line = code.split("\n").map((l) => l.trim()).find((l) => l !== "") ?? "";
  return line.length > 150 ? line.slice(0, 149) + "…" : line;
}

/** Headers of a preview image: forever when it can never change, else briefly. */
export function previewHeaders(permanent: boolean): Record<string, string> {
  return {
    "content-type": "image/png",
    "cache-control": permanent ? "public, max-age=31536000, immutable" : "public, max-age=300",
  };
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

/** Landing-page runs: 20 a minute and 120 an hour per IP (cached results are free). */
export const LANDING_LIMITS: Window[] = [
  { max: 20, ms: 60_000 },
  { max: 120, ms: 3_600_000 },
];

/** Hex SHA-256 of `text` as UTF-8. */
export async function sha256(text: string): Promise<string> {
  const digest = new Uint8Array(await crypto.subtle.digest("SHA-256", new TextEncoder().encode(text)));
  return [...digest].map((b) => b.toString(16).padStart(2, "0")).join("");
}

const ENTITIES: Record<string, string> = { amp: "&", lt: "<", gt: ">", quot: '"', apos: "'", nbsp: " ", rsquo: "’", lsquo: "‘", rdquo: "”", ldquo: "“" };

/** Text with HTML character references decoded, as a browser's `textContent` reads it. */
export function decodeHtml(html: string): string {
  return html.replace(/&(#x[0-9a-fA-F]+|#\d+|[A-Za-z]+);/g, (whole, name: string) => {
    if (name.startsWith("#x")) return String.fromCodePoint(parseInt(name.slice(2), 16));
    if (name.startsWith("#")) return String.fromCodePoint(parseInt(name.slice(1), 10));
    return ENTITIES[name] ?? whole;
  });
}

/** One `<code data-run>` element: its `data-run` name (may be empty) and its text. */
export interface RunBlock {
  name: string;
  code: string;
  /** The element's inner HTML, to catch markup inside it. */
  html: string;
  /** Its `data-play` attribute, the `?play` options its Run button sends, or null. */
  play: string | null;
  /**
   * The options of the `?play` file header right above its `<pre>`, as the
   * page shows them, or null when no such header is there.
   */
  header: string | null;
}

/**
 * The `<code data-run>` elements in a page's HTML. Their text is what the
 * page's Run buttons send (`textContent`), with the `data-play` options, so
 * these are the only snippets /play/run accepts without Turnstile.
 */
export function runBlocks(html: string): RunBlock[] {
  const blocks: RunBlock[] = [];
  for (const match of html.matchAll(/<code\b([^>]*)\bdata-run\b(?:="([^"]*)")?([^>]*)>([\s\S]*?)<\/code>/g)) {
    const play = /\bdata-play="([^"]*)"/.exec(match[1] + match[3]);
    const above = /<div class="file"><b>([^<]*)<\/b>(?:<span>[^<]*<\/span>)?<\/div>\s*<pre>\s*$/.exec(html.slice(0, match.index));
    const title = above ? decodeHtml(above[1]).trim() : "";
    blocks.push({
      name: match[2] ?? "",
      code: decodeHtml(match[4]),
      html: match[4],
      play: play ? decodeHtml(play[1]) : null,
      header: title === "?play" || title.startsWith("?play ") ? title.slice("?play".length).trim() : null,
    });
  }
  return blocks;
}

/** The snippet a run block's Run button sends to /play/run, or an error. */
export function runSnippet(block: { code: string; play: string | null }): Snippet | string {
  return parseSnippet(block.play === null ? { code: block.code } : { code: block.code, play: block.play });
}

/** The sources a page runs: the text of each `<code data-run>` element. */
export function runnable(html: string): string[] {
  return runBlocks(html).map((block) => block.code);
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

/**
 * rawr-web's files under /web (web/build.sh): each kind's content type, and
 * its caching. The module and its glue revalidate by ETag, so a release shows
 * at once; the fonts are pinned, so a browser keeps them a week. A CJK
 * collection (.ttc) is served as font/ttf, a type the edge compresses.
 */
export function webHeaders(path: string, from: Headers): Headers | null {
  const types: Record<string, string> = {
    ".wasm": "application/wasm",
    ".js": "text/javascript; charset=utf-8",
    ".ttf": "font/ttf",
    ".ttc": "font/ttf",
  };
  const type = types[path.slice(path.lastIndexOf("."))];
  if (!type) return null;
  const headers = new Headers(from);
  headers.set("content-type", type);
  if (path.startsWith("/web/fonts/")) headers.set("cache-control", "public, max-age=604800");
  return headers;
}

/**
 * Serve a file under /web. web/build.sh writes a brotli copy (.br, at
 * quality 11) of the module and the fonts: a browser that takes brotli gets
 * it as is, about a quarter smaller than the edge's own compression makes
 * it. Anything else goes out as stored, which the edge compresses by its
 * content type.
 */
export async function webAsset(request: Request, assets: { fetch(request: Request): Promise<Response> }): Promise<Response> {
  const path = new URL(request.url).pathname;
  const brotli = /\bbr\b/.test(request.headers.get("accept-encoding") ?? "");
  if (brotli && !path.endsWith(".js") && webHeaders(path, new Headers())) {
    const packed = await assets.fetch(new Request(request.url + ".br", request));
    if (packed.ok || packed.status === 304) {
      const headers = webHeaders(path, packed.headers)!;
      headers.set("content-encoding", "br");
      headers.set("vary", "accept-encoding");
      return new Response(packed.body, { status: packed.status, headers, encodeBody: "manual" } as ResponseInit);
    }
  }
  const response = await assets.fetch(request);
  const headers = response.ok ? webHeaders(path, response.headers) : null;
  return headers ? new Response(response.body, { status: response.status, headers }) : response;
}
