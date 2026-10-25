// rawr-play on Cloudflare Containers: one always-on container runs
// rawr-play-bot, which holds Discord's gateway websocket (outbound only).
// A cron trigger and GET /bot/health start it if it is not running.
//
// It serves all of rawr.airstrike.fyi: the landing page (site/public) and
// the bot's files (public/), merged into dist/ by build.mjs, plus /play and
// /bot. The old host, rawr-bot.airstrike.fyi, redirects every request here.
//
// The channels each server's mods chose with /play-channel live in this
// Durable Object's storage. The bot loads and saves them at
// http://play-config.internal/channels, a host only the container can reach:
// outbound interception hands its requests to `config` below, so there is no
// public endpoint and no secret. The bot also posts its boot events
// (/events) and minute stats (/stats) there, which /bot shows as a dashboard.
//
// A Durable Object's container keeps the image it was started from: a deploy
// does not replace it. The Durable Object records the PLAY_VERSION (and the
// settings passed to the container) it started
// and `ensure` (cron, /bot/health) restarts a container started from another.
//
// /play is a web playground: the Worker serves the page, keeps shares in this
// Durable Object (content-addressed), limits each IP (by HMAC, never in
// clear), checks Turnstile, and forwards renders to the bot's HTTP port,
// which only this Durable Object can reach. A render is an iced snippet's
// PNG or a plain program's output (JSON). The Turnstile widget stays
// invisible unless Cloudflare needs an interaction. After one successful
// Siteverify the Worker sets a 30-day pass cookie (HttpOnly, Secure,
// SameSite=Lax), signed with a key derived from TURNSTILE_SECRET. While
// the pass verifies, /play skips Turnstile.
//
// POST /play/run runs the site's own snippets without Turnstile: only
// sources whose SHA-256 is one of the `<code data-run>` texts in the site's
// pages. build.mjs collects those hashes into src/runnable.gen.ts, so the
// list is the deployed pages'. Results are cached per image version. A
// snippet edited on the site is the visitor's own code: run.js sends it to
// POST /play/render with the playground's Turnstile, pass and limits, and
// learns the site key and whether it holds a pass from GET /play/gate.

import { Container, ContainerProxy, getContainer, type OutboundHandlerContext } from "@cloudflare/containers";
import { render, type Dashboard } from "./dashboard.ts";
import { COUNTER, EXAMPLES } from "./examples.ts";
import {
  ID_PATTERN,
  KEEP_DAYS,
  LANDING_LIMITS,
  Limiter,
  MAX_PREVIEW,
  MAX_SHARE,
  PREVIEW_LIMITS,
  RETRY_MS,
  day,
  firstLine,
  firstOutputLine,
  parseTextResult,
  pngSize,
  stillBody,
  previewHeaders,
  purgeSelection,
  RENDER_LIMITS,
  SHARE_LIMITS,
  canonical,
  sha256,
  hmac,
  checkPass,
  cookie,
  renderRequest,
  makePass,
  PASS_COOKIE,
  passCookie,
  parseSnippet,
  same,
  shareId,
  type Og,
  type Snippet,
} from "./play.ts";
import { render as renderPlay } from "./playpage.ts";
import { RUNNABLE } from "./runnable.gen.ts";
import {
  addWebRateLimited,
  emptyStats,
  merge,
  parseEvent,
  parseReport,
  pushEvent,
  summarize,
  type Event,
  type Stats,
} from "./stats.ts";

// Outbound interception runs through this entrypoint.
export { ContainerProxy };

const CONFIG_HOST = "play-config.internal";
const CONFIG_KEY = "play-channels";
const CONFIG_MAX = 64 * 1024;
const STATS_KEY = "play-stats";
const EVENTS_KEY = "play-events";
const STARTED_KEY = "play-started-version";
/** Unix seconds of the last start, and the deployed value a stale image was last restarted for. */
const STARTED_AT_KEY = "play-started-at";
const STALE_KEY = "play-stale-restart";
const REPORT_MAX = 256 * 1024;
const SHARE_PREFIX = "share:";
const SEEN_PREFIX = "seen:";
const PURGE_KEY = "play-purge";
const COUNT_KEY = "play-share-count";
const HOST = "rawr.airstrike.fyi";
const SITE = `https://${HOST}`;
const OLD_HOST = "rawr-bot.airstrike.fyi";
/** Old paths that moved under /bot; every other path is the same on the new host. */
const MOVED: Record<string, string> = { "/": "/bot", "/invite": "/bot/invite", "/health": "/bot/health" };

/**
 * Docs pages that moved. The FAQ's answers went to /docs/how-it-works and
 * elsewhere. rawr-iced's and ?play's pages became directories. The reference
 * pages went under /docs/reference, and /docs/language became its overview
 * and one page per topic. A redirect keeps the URL's #anchor, and docs.js
 * sends an anchor that moved on to its new page.
 */
export const DOCS_MOVED: Record<string, string> = {
  "/docs/faq": "/docs/how-it-works",
  "/docs/faq.html": "/docs/how-it-works",
  "/docs/iced.html": "/docs/iced",
  "/docs/play.html": "/docs/play",
  "/docs/language": "/docs/reference/overview",
  "/docs/language.html": "/docs/reference/overview",
  "/docs/cli": "/docs/reference/commands",
  "/docs/cli.html": "/docs/reference/commands",
  "/docs/requirements": "/docs/reference/requirements",
  "/docs/requirements.html": "/docs/reference/requirements",
  "/docs/iced/reference": "/docs/reference/iced",
  "/docs/iced/reference.html": "/docs/reference/iced",
  "/docs/play/reference": "/docs/reference/play",
  "/docs/play/reference.html": "/docs/reference/play",
  "/docs/reference": "/docs/reference/overview",
};

/**
 * A share's preview: its PNG, a plain program's output (JSON), or a failure
 * (kept forever for a code error or a limit).
 */
export type Preview =
  | { kind: "png"; png: Uint8Array; rendered?: boolean }
  | { kind: "text"; output: string }
  | { kind: "failed"; permanent: boolean }
  | { kind: "missing" }
  | { kind: "limited"; wait: number };

export interface Purge {
  day: number;
  stored: number;
  purged: number;
}
const HTTP_PORT = 8080;

interface Env {
  ASSETS: Fetcher;
  PLAY_BOT: DurableObjectNamespace<PlayBot>;
  DISCORD_TOKEN: string;
  PLAY_WORKERS: string;
  PLAY_MEMORY_MB: string;
  PLAY_TIMEOUT_MS: string;
  PLAY_TIME_MS: string;
  PLAY_ANIMATION_TIME_MS: string;
  PLAY_IDLE_ANIMATION_TIME_MS: string;
  PLAY_ANIMATION_ALLOWANCE_S: string;
  PLAY_ANIMATION_WINDOW_S: string;
  PLAY_VERSION: string;
  /**
   * Secret: the key the bot makes server tokens with. The Worker only
   * derives keys from it (HMAC) for hashing IPs and the admin header.
   */
  PLAY_LOG_KEY?: string;
  /** Turnstile: the public site key (a var) and the secret. */
  TURNSTILE_SITEKEY?: string;
  TURNSTILE_SECRET?: string;
}

export class PlayBot extends Container<Env> {
  sleepAfter = "1h";
  // The playground's render port (PLAY_HTTP_PORT); a start waits for it.
  defaultPort = HTTP_PORT;
  private renders = new Limiter(RENDER_LIMITS);
  private shares = new Limiter(SHARE_LIMITS);
  private previews = new Limiter(PREVIEW_LIMITS);
  private landing = new Limiter(LANDING_LIMITS);
  /** Renders in flight by share id, so concurrent first requests render once. */
  private rendering = new Map<string, Promise<Preview>>();
  /** The day each share was last seen visiting, as stored (saves reads). */
  private seenDays = new Map<string, number>();

  constructor(ctx: DurableObjectState<{}>, env: Env) {
    super(ctx, env);
    this.envVars = {
      DISCORD_TOKEN: env.DISCORD_TOKEN,
      PLAY_WORKERS: env.PLAY_WORKERS,
      PLAY_MEMORY_MB: env.PLAY_MEMORY_MB,
      PLAY_TIMEOUT_MS: env.PLAY_TIMEOUT_MS,
      PLAY_TIME_MS: env.PLAY_TIME_MS,
      PLAY_ANIMATION_TIME_MS: env.PLAY_ANIMATION_TIME_MS,
      PLAY_IDLE_ANIMATION_TIME_MS: env.PLAY_IDLE_ANIMATION_TIME_MS,
      PLAY_ANIMATION_ALLOWANCE_S: env.PLAY_ANIMATION_ALLOWANCE_S,
      PLAY_ANIMATION_WINDOW_S: env.PLAY_ANIMATION_WINDOW_S,
      PLAY_CONFIG_URL: `http://${CONFIG_HOST}/channels`,
      PLAY_REPORT_URL: `http://${CONFIG_HOST}`,
      PLAY_HTTP_PORT: String(HTTP_PORT),
      // Only the container uses it; without it the bot reports no servers.
      ...(env.PLAY_LOG_KEY ? { PLAY_LOG_KEY: env.PLAY_LOG_KEY } : {}),
    };
  }

  private ensuring: Promise<boolean> | undefined;

  /** The stored channel config: `{"<guild id>": ["<channel id>", ...]}`. */
  async channels(): Promise<string> {
    return (await this.ctx.storage.get<string>(CONFIG_KEY)) ?? "{}";
  }

  async setChannels(json: string): Promise<void> {
    await this.ctx.storage.put(CONFIG_KEY, json);
  }

  async event(source: Event["source"], phase: string, detail: string): Promise<void> {
    const events = (await this.ctx.storage.get<Event[]>(EVENTS_KEY)) ?? [];
    const t = Math.floor(Date.now() / 1000);
    await this.ctx.storage.put(EVENTS_KEY, pushEvent(events, { t, source, phase, detail }));
  }

  async botEvent(body: unknown): Promise<boolean> {
    const event = parseEvent(body, Math.floor(Date.now() / 1000));
    if (!event) return false;
    const events = (await this.ctx.storage.get<Event[]>(EVENTS_KEY)) ?? [];
    await this.ctx.storage.put(EVENTS_KEY, pushEvent(events, event));
    return true;
  }

  async report(body: unknown): Promise<boolean> {
    const report = parseReport(body);
    if (!report) return false;
    const stats = (await this.ctx.storage.get<Stats>(STATS_KEY)) ?? emptyStats();
    await this.ctx.storage.put(STATS_KEY, merge(stats, report, Date.now()));
    return true;
  }

  /** Count a playground request from `ip` (a hash); seconds to wait, or 0. */
  async allow(kind: "render" | "share" | "landing", ip: string): Promise<number> {
    const limiter = kind === "render" ? this.renders : kind === "share" ? this.shares : this.landing;
    const wait = limiter.take(ip, Date.now());
    if (wait > 0 && kind === "render") {
      const stats = (await this.ctx.storage.get<Stats>(STATS_KEY)) ?? emptyStats();
      await this.ctx.storage.put(STATS_KEY, addWebRateLimited(stats, Date.now()));
    }
    return wait;
  }

  /** Store a share under its id (the same snippet always lands on the same key). */
  async saveShare(id: string, text: string): Promise<void> {
    const key = SHARE_PREFIX + id;
    if ((await this.ctx.storage.get<string>(key)) === undefined) {
      await this.ctx.storage.put(key, text);
      const count = await this.ctx.storage.get<number>(COUNT_KEY);
      // Counted from the next purge pass on (it counts them all).
      if (count !== undefined) await this.ctx.storage.put(COUNT_KEY, count + 1);
    }
    await this.visit(id);
  }

  /** Record a visit: at most one write per share per day. */
  async visit(id: string): Promise<void> {
    const today = day(Date.now());
    if (this.seenDays.get(id) === today) return;
    const stored = await this.ctx.storage.get<number>(SEEN_PREFIX + id);
    if (stored !== today) await this.ctx.storage.put(SEEN_PREFIX + id, today);
    this.seenDays.set(id, today);
  }

  private tableReady = false;

  /** The previews: a PNG, or `output` (a plain program's JSON), or neither for a failure. */
  private previewTable(): SqlStorage {
    const sql = this.ctx.storage.sql;
    if (this.tableReady) return sql;
    sql.exec(
      "CREATE TABLE IF NOT EXISTS play_previews (id TEXT PRIMARY KEY, png BLOB, permanent INTEGER NOT NULL, retry_at INTEGER NOT NULL)",
    );
    const columns = sql.exec("PRAGMA table_info(play_previews)").toArray();
    if (!columns.some((column) => column.name === "output")) sql.exec("ALTER TABLE play_previews ADD COLUMN output TEXT");
    this.tableReady = true;
    return sql;
  }

  /** What the stored preview is, without rendering: an image's size, or a program's output. */
  async previewInfo(id: string): Promise<{ width: number; height: number } | { output: string } | null> {
    const rows = this.previewTable().exec("SELECT png, output FROM play_previews WHERE id = ?", id).toArray();
    const { png, output } = rows[0] ?? {};
    if (typeof output === "string") return { output };
    return png instanceof ArrayBuffer ? pngSize(new Uint8Array(png)) : null;
  }

  /**
   * A share's preview: stored, else rendered once (concurrent callers share
   * the render) and stored. `ip` (a hash) limits uncached renders; null for
   * the warm-up after a share.
   */
  async preview(id: string, ip: string | null): Promise<Preview> {
    const sql = this.previewTable();
    const row = sql.exec("SELECT png, output, permanent, retry_at FROM play_previews WHERE id = ?", id).toArray()[0];
    if (row) {
      if (row.png instanceof ArrayBuffer) return { kind: "png", png: new Uint8Array(row.png) };
      if (typeof row.output === "string") return { kind: "text", output: row.output };
      if (row.permanent || Date.now() < Number(row.retry_at)) return { kind: "failed", permanent: Boolean(row.permanent) };
    }
    const pending = this.rendering.get(id);
    if (pending) return pending;
    if (ip !== null) {
      const wait = this.previews.take(ip, Date.now());
      if (wait > 0) return { kind: "limited", wait };
    }
    const render = this.renderPreview(id).finally(() => this.rendering.delete(id));
    this.rendering.set(id, render);
    return render;
  }

  private async renderPreview(id: string): Promise<Preview> {
    const text = await this.share(id);
    if (text === null) return { kind: "missing" };
    let response: Response;
    try {
      response = await this.containerFetch(
        // A preview is the last frame as a PNG, never an animation.
        new Request("http://container/render", { method: "POST", headers: { "content-type": "application/json" }, body: stillBody(text) }),
        HTTP_PORT,
      );
    } catch {
      return this.failed(id, false);
    }
    if (response.ok && (response.headers.get("content-type") ?? "").startsWith("application/json")) {
      const output = await response.text();
      if (output.length > MAX_PREVIEW || !parseTextResult(output)) return this.failed(id, true);
      this.previewTable().exec(
        "INSERT OR REPLACE INTO play_previews (id, png, output, permanent, retry_at) VALUES (?, NULL, ?, 1, 0)",
        id,
        output,
      );
      return { kind: "text", output };
    }
    if (response.ok) {
      const png = new Uint8Array(await response.arrayBuffer());
      if (png.length > MAX_PREVIEW || !pngSize(png)) return this.failed(id, true);
      this.previewTable().exec(
        "INSERT OR REPLACE INTO play_previews (id, png, permanent, retry_at) VALUES (?, ?, 1, 0)",
        id,
        png.buffer.slice(png.byteOffset, png.byteOffset + png.byteLength),
      );
      return { kind: "png", png, rendered: true };
    }
    await response.body?.cancel();
    // A code error or a bad request never changes for the same snippet; busy or
    // unreachable is retried after RETRY_MS.
    return this.failed(id, response.status === 422 || response.status === 400);
  }

  private failed(id: string, permanent: boolean): Preview {
    this.previewTable().exec(
      "INSERT OR REPLACE INTO play_previews (id, png, permanent, retry_at) VALUES (?, NULL, ?, ?)",
      id,
      permanent ? 1 : 0,
      Date.now() + RETRY_MS,
    );
    return { kind: "failed", permanent };
  }

  /**
   * Once a day (from the cron): delete shares unvisited for KEEP_DAYS, with
   * their previews; stamp shares that have no visit day yet. Returns the ids
   * removed (for the cache) or null when today's pass already ran.
   */
  async purge(): Promise<string[] | null> {
    const today = day(Date.now());
    const last = await this.ctx.storage.get<Purge>(PURGE_KEY);
    if (last?.day === today) return null;
    const seen = new Map<string, number | undefined>((await this.shareIds()).map((id) => [id, undefined]));
    const days = await this.ctx.storage.list<number>({ prefix: SEEN_PREFIX });
    for (const [key, value] of days) {
      const id = key.slice(SEEN_PREFIX.length);
      if (seen.has(id)) seen.set(id, value);
      else await this.ctx.storage.delete(key);
    }
    const { remove, stamp } = purgeSelection(seen, today);
    for (const id of stamp) await this.ctx.storage.put(SEEN_PREFIX + id, today);
    for (const id of remove) {
      await this.ctx.storage.delete([SHARE_PREFIX + id, SEEN_PREFIX + id]);
      this.previewTable().exec("DELETE FROM play_previews WHERE id = ?", id);
      this.seenDays.delete(id);
    }
    await this.ctx.storage.put(PURGE_KEY, { day: today, stored: seen.size - remove.length, purged: remove.length });
    await this.ctx.storage.put(COUNT_KEY, seen.size - remove.length);
    if (remove.length > 0) await this.event("worker", "purge", `${remove.length} share(s) unvisited for ${KEEP_DAYS} days`);
    return remove;
  }

  async share(id: string): Promise<string | null> {
    return (await this.ctx.storage.get<string>(SHARE_PREFIX + id)) ?? null;
  }

  private async purgeSummary(): Promise<Purge | null> {
    const last = await this.ctx.storage.get<Purge>(PURGE_KEY);
    if (!last) return null;
    let count = await this.ctx.storage.get<number>(COUNT_KEY);
    if (count === undefined) {
      count = (await this.shareIds()).length;
      await this.ctx.storage.put(COUNT_KEY, count);
    }
    return { ...last, stored: count };
  }

  /** Every stored share's id, read in pages. */
  private async shareIds(): Promise<string[]> {
    const ids: string[] = [];
    let start: string | undefined;
    for (;;) {
      const page = await this.ctx.storage.list<string>({ prefix: SHARE_PREFIX, start, limit: 128 });
      for (const key of page.keys()) ids.push(key.slice(SHARE_PREFIX.length));
      if (page.size < 128) return ids;
      start = [...page.keys()].pop() + "\u0000";
    }
  }

  async dashboard(): Promise<Dashboard> {
    const now = Date.now();
    const stats = (await this.ctx.storage.get<Stats>(STATS_KEY)) ?? emptyStats();
    return {
      summary: summarize(stats, now),
      status: stats.status,
      events: (await this.ctx.storage.get<Event[]>(EVENTS_KEY)) ?? [],
      servers: stats.servers ?? {},
      running: this.ctx.container?.running ?? false,
      deployed: this.env.PLAY_VERSION,
      started: (await this.ctx.storage.get<string>(STARTED_KEY))?.split(" ")[0] ?? null,
      purge: await this.purgeSummary(),
      now: Math.floor(now / 1000),
    };
  }

  // The bot gets no inbound requests, so idleness means nothing: never sleep.
  override async onActivityExpired() {}

  override async onStop({ exitCode, reason }: { exitCode?: number; reason?: string }) {
    console.log(`rawr-play-bot stopped (${reason}, exit ${exitCode}); the next cron restarts it`);
    await this.event("worker", "stopped", `${reason ?? "unknown"}, exit code ${exitCode ?? "?"}`);
  }

  override async onError(error: unknown): Promise<never> {
    await this.event("worker", "error", String(error).slice(0, 500));
    throw error;
  }

  /** Start the bot unless it is running from the deployed image; true once it runs. */
  async ensure(): Promise<boolean> {
    // Cron and /bot/health may overlap: one at a time.
    this.ensuring ??= this.ensureOnce().finally(() => (this.ensuring = undefined));
    return this.ensuring;
  }

  /** The image and the settings the container reads at start: a change to either restarts it. */
  private wanted(): string {
    const e = this.env;
    return (
      `${e.PLAY_VERSION} workers=${e.PLAY_WORKERS},memory=${e.PLAY_MEMORY_MB},timeout=${e.PLAY_TIMEOUT_MS},time=${e.PLAY_TIME_MS},` +
      `animation=${e.PLAY_ANIMATION_TIME_MS},idle=${e.PLAY_IDLE_ANIMATION_TIME_MS},allowance=${e.PLAY_ANIMATION_ALLOWANCE_S}/${e.PLAY_ANIMATION_WINDOW_S}`
    );
  }

  private async ensureOnce(): Promise<boolean> {
    const deployed = this.wanted();
    const started = await this.ctx.storage.get<string>(STARTED_KEY);
    const container = this.ctx.container;
    // A start can land before Cloudflare has the deploy's image: the bot it
    // started then reports the old image. Restart it once for this deploy.
    const status = (await this.ctx.storage.get<Stats>(STATS_KEY))?.status;
    const startedAt = (await this.ctx.storage.get<number>(STARTED_AT_KEY)) ?? 0;
    const stale =
      started === deployed &&
      status !== null &&
      status !== undefined &&
      status.boot >= startedAt &&
      status.version !== this.env.PLAY_VERSION &&
      (await this.ctx.storage.get<string>(STALE_KEY)) !== deployed;
    if (container?.running && (started !== deployed || stale)) {
      if (stale) await this.ctx.storage.put(STALE_KEY, deployed);
      const running = stale ? `the bot reports ${status?.version}` : `running ${started ?? "an unrecorded image"}`;
      await this.event("worker", "restart", `${running}; deployed ${deployed}`);
      await container.destroy();
      for (let i = 0; i < 40 && container.running; i++) {
        await new Promise((resolve) => setTimeout(resolve, 250));
      }
    }
    if (!container?.running) {
      await this.event("worker", "start", deployed);
      try {
        await this.start();
      } catch (error) {
        await this.event("worker", "error", `start failed: ${String(error).slice(0, 400)}`);
        return false;
      }
      await this.ctx.storage.put(STARTED_KEY, deployed);
      await this.ctx.storage.put(STARTED_AT_KEY, Math.floor(Date.now() / 1000));
    }
    return this.ctx.container?.running ?? false;
  }
}

// Plain HTTP from the container to CONFIG_HOST; every other host (Discord)
// goes out as before. Assigned, not a `static` field: the base class's setter
// registers the handlers by class name, and a class field would shadow it.
PlayBot.outboundByHost = { [CONFIG_HOST]: config };

/**
 * From the container only, into its Durable Object: GET and PUT /channels,
 * POST /events, PUT /stats.
 */
async function config(request: Request, env: Env, ctx: OutboundHandlerContext): Promise<Response> {
  const path = new URL(request.url).pathname;
  const bot = env.PLAY_BOT.get(env.PLAY_BOT.idFromString(ctx.containerId));
  if (path === "/events" || path === "/stats") {
    const want = path === "/events" ? "POST" : "PUT";
    if (request.method !== want) {
      return new Response("method not allowed\n", { status: 405 });
    }
    const text = await request.text();
    let body: unknown;
    try {
      body = text.length <= REPORT_MAX ? JSON.parse(text) : null;
    } catch {
      body = null;
    }
    const ok = path === "/events" ? await bot.botEvent(body) : await bot.report(body);
    return new Response(null, { status: ok ? 204 : 400 });
  }
  if (path !== "/channels") {
    return new Response("not found\n", { status: 404 });
  }
  if (request.method === "GET") {
    // Seen once per boot: proof that interception reaches the Worker.
    await bot.event("worker", "channels read", "the bot reached the config host");
    return new Response(await bot.channels(), { headers: { "content-type": "application/json" } });
  }
  if (request.method !== "PUT") {
    return new Response("method not allowed\n", { status: 405 });
  }
  const json = await request.text();
  if (json.length > CONFIG_MAX || !validConfig(json)) {
    return new Response("bad channel config\n", { status: 400 });
  }
  await bot.setChannels(json);
  return new Response(null, { status: 204 });
}

/** An object of ID strings to lists of ID strings. */
function validConfig(json: string): boolean {
  let value: unknown;
  try {
    value = JSON.parse(json);
  } catch {
    return false;
  }
  const id = (x: unknown) => typeof x === "string" && /^\d{1,20}$/.test(x);
  return (
    typeof value === "object" &&
    value !== null &&
    !Array.isArray(value) &&
    Object.entries(value).every(
      ([guild, channels]) => id(guild) && Array.isArray(channels) && channels.every(id),
    )
  );
}

const EMPTY: Snippet = {
  code: COUNTER.code,
  theme: "dark",
  size: "",
  messages: COUNTER.messages ?? [],
};

const html = (body: string, status = 200) =>
  new Response(body, {
    status,
    headers: { "content-type": "text/html; charset=utf-8", "cache-control": "no-store" },
  });

const plain = (body: string, status: number, headers: Record<string, string> = {}) =>
  new Response(body, { status, headers: { "content-type": "text/plain; charset=utf-8", ...headers } });

/** The caller's IP as an HMAC under a key derived from the log key: never stored in clear. */
async function ipHash(request: Request, env: Env): Promise<string> {
  const ip = request.headers.get("cf-connecting-ip") ?? "unknown";
  const key = await hmac(env.PLAY_LOG_KEY ?? "rawr-play", "ip");
  return (await hmac(key, ip)).slice(0, 32);
}

/** A caller holding the log key (header `x-play-admin`) skips Turnstile, not the limits. */
async function admin(request: Request, env: Env): Promise<boolean> {
  const header = request.headers.get("x-play-admin");
  if (!header || !env.PLAY_LOG_KEY) return false;
  return same(header, await hmac(env.PLAY_LOG_KEY, "rawr-play admin"));
}

async function human(token: unknown, request: Request, env: Env): Promise<boolean> {
  if (!env.TURNSTILE_SECRET) return true;
  if (typeof token !== "string" || token === "" || token.length > 2048) return false;
  const form = new FormData();
  form.append("secret", env.TURNSTILE_SECRET);
  form.append("response", token);
  const ip = request.headers.get("cf-connecting-ip");
  if (ip) form.append("remoteip", ip);
  const result = await fetch("https://challenges.cloudflare.com/turnstile/v0/siteverify", { method: "POST", body: form });
  const outcome = (await result.json()) as { success?: boolean };
  return outcome.success === true;
}

/** The key passes are signed with, derived from the Turnstile secret. */
const passKey = (secret: string) => hmac(secret, "rawr-play pass");

/** True when the request carries an unexpired pass signed by this Worker. */
async function hasPass(request: Request, env: Env): Promise<boolean> {
  if (!env.TURNSTILE_SECRET) return false;
  const value = cookie(request.headers.get("cookie"), PASS_COOKIE);
  return checkPass(await passKey(env.TURNSTILE_SECRET), value, Date.now());
}

/** The response with a new pass cookie, when there is one to set. */
function withPass(response: Response, pass: string | null): Response {
  if (pass) {
    response.headers.append("set-cookie", passCookie(pass));
    // The cookie is HttpOnly, so the page learns of it from this header.
    response.headers.set("x-play-pass", "set");
  }
  return response;
}

async function readJson(request: Request, max: number): Promise<unknown> {
  const text = await request.text();
  if (text.length > max) return null;
  try {
    return JSON.parse(text);
  } catch {
    return null;
  }
}

/** GET /play/<id>.png: the share's last frame, rendered once and kept. */
async function previewImage(request: Request, env: Env, ctx: ExecutionContext, id: string): Promise<Response> {
  const bot = getContainer(env.PLAY_BOT);
  ctx.waitUntil(bot.visit(id));
  const cache = caches.default;
  const key = new Request(`${SITE}/play/${id}.png`);
  const hit = await cache.match(key);
  if (hit) {
    const response = new Response(hit.body, hit);
    response.headers.set("x-preview", "cache");
    return response;
  }
  const preview = await bot.preview(id, await ipHash(request, env));
  switch (preview.kind) {
    case "missing":
      return plain("no such share", 404);
    case "limited":
      return plain(`too many previews; try again in ${preview.wait} s`, 429, { "retry-after": String(preview.wait) });
    case "png": {
      const response = new Response(preview.png.slice().buffer, { headers: previewHeaders(true) });
      ctx.waitUntil(cache.put(key, response.clone()));
      // Where it came from: "rendered" now, or "stored" in the Durable Object.
      response.headers.set("x-preview", preview.rendered ? "rendered" : "stored");
      return response;
    }
    case "text":
    case "failed": {
      // A plain program's share shows the site's card; its output is in
      // the description. A failure shows the error card.
      const text = preview.kind === "text";
      const card = await env.ASSETS.fetch(new Request(`${SITE}/og/${text ? "card" : "play-error"}.png`));
      const response = new Response(card.body, { headers: previewHeaders(text || preview.permanent) });
      ctx.waitUntil(cache.put(key, response.clone()));
      return response;
    }
  }
}

/** The SHA-256 of each source the site's Run buttons send. */
const SITE_SOURCES: ReadonlySet<string> = new Set(RUNNABLE);

/**
 * POST /play/run: one of the site's own snippets, by its exact
 * source, without Turnstile. Any other source is refused. Each result is
 * cached for the deployed image; uncached runs are limited per IP.
 */
async function landingRun(request: Request, env: Env, ctx: ExecutionContext): Promise<Response> {
  const body = (await readJson(request, 48 * 1024)) as Record<string, unknown> | null;
  const code = body?.code;
  if (typeof code !== "string") return plain("bad request", 400);
  const hash = await sha256(code);
  if (!SITE_SOURCES.has(hash)) {
    return plain("only the site's own snippets run here; use /play for your own code", 403);
  }
  const cache = caches.default;
  const key = new Request(`${SITE}/play/run/${hash}/${env.PLAY_VERSION}`);
  const hit = await cache.match(key);
  if (hit) {
    const response = new Response(hit.body, hit);
    response.headers.set("x-run", "cache");
    return response;
  }
  const bot = getContainer(env.PLAY_BOT);
  const wait = await bot.allow("landing", await ipHash(request, env));
  if (wait > 0) return plain(`too many runs; try again in ${wait} s`, 429, { "retry-after": String(wait) });
  const snippet: Snippet = { code, theme: "dark", size: "", messages: [] };
  let response: Response;
  try {
    response = await bot.fetch(renderRequest(canonical(snippet), await ipHash(request, env)));
  } catch (error) {
    return plain(`the runner is not reachable right now (${String(error).slice(0, 200)})`, 503);
  }
  const bytes = await response.arrayBuffer();
  const type = response.headers.get("content-type") ?? "text/plain";
  if (!response.ok) return new Response(bytes, { status: response.status, headers: { "content-type": type, "cache-control": "no-store" } });
  // A render the clock cut short (the bot was busy, or the IP's animation
  // time was used up) is not kept: the next run may get the full time.
  const cut = (response.headers.get("x-play-note") ?? "").includes("Rendering stopped");
  const result = new Response(bytes, { headers: { "content-type": type, "cache-control": cut ? "no-store" : "public, max-age=86400" } });
  if (!cut) ctx.waitUntil(cache.put(key, result.clone()));
  result.headers.set("x-run", "ran");
  return result;
}

async function play(request: Request, env: Env, ctx: ExecutionContext, path: string): Promise<Response> {
  const bot = getContainer(env.PLAY_BOT);
  const image = /^\/play\/([0-9A-Za-z]{10})\.png$/.exec(path);
  if (image && (request.method === "GET" || request.method === "HEAD")) {
    return previewImage(request, env, ctx, image[1]);
  }
  if (path === "/play/render" && request.method === "POST") {
    const body = (await readJson(request, 48 * 1024)) as Record<string, unknown> | null;
    const snippet = parseSnippet(body);
    if (typeof snippet === "string") return plain(snippet, 400);
    // A pass from an earlier Siteverify skips Turnstile for 30 days. A
    // missing, forged or expired one falls back to Turnstile, and a
    // successful Siteverify sets a new one.
    let pass: string | null = null;
    if (!(await hasPass(request, env)) && !(await admin(request, env))) {
      if (!(await human(body?.turnstile, request, env))) {
        return plain("the browser check did not pass; reload the page and try again", 403);
      }
      if (env.TURNSTILE_SECRET) pass = await makePass(await passKey(env.TURNSTILE_SECRET), Date.now());
    }
    const wait = await bot.allow("render", await ipHash(request, env));
    if (wait > 0) return withPass(plain(`too many runs; try again in ${wait} s`, 429, { "retry-after": String(wait) }), pass);
    // The bot counts an animation's longer time against the IP's hash.
    const forward = renderRequest(canonical(snippet), await ipHash(request, env));
    try {
      const response = await bot.fetch(forward);
      const headers: Record<string, string> = { "content-type": response.headers.get("content-type") ?? "text/plain", "cache-control": "no-store" };
      // A note on an animation, such as why it was cut.
      const note = response.headers.get("x-play-note");
      if (note) headers["x-play-note"] = note;
      return withPass(
        new Response(response.body, { status: response.status, headers }),
        pass,
      );
    } catch (error) {
      return withPass(plain(`the renderer is not reachable right now (${String(error).slice(0, 200)})`, 503), pass);
    }
  }
  if (path === "/play/run" && request.method === "POST") {
    return landingRun(request, env, ctx);
  }
  if (path === "/play/gate" && request.method === "GET") {
    // What the site's run.js needs to send an edited snippet to
    // /play/render: the Turnstile site key, and whether this browser holds a
    // pass (the cookie is HttpOnly, so the page can't read it).
    return Response.json(
      { sitekey: env.TURNSTILE_SITEKEY ?? null, pass: await hasPass(request, env) },
      { headers: { "cache-control": "no-store" } },
    );
  }
  if (path === "/play/share" && request.method === "POST") {
    const snippet = parseSnippet(await readJson(request, 2 * MAX_SHARE));
    if (typeof snippet === "string") return plain(snippet, 400);
    const text = canonical(snippet);
    if (text.length > MAX_SHARE) return plain("too large to share", 413);
    const wait = await bot.allow("share", await ipHash(request, env));
    if (wait > 0) return plain(`too many shares; try again in ${wait} s`, 429, { "retry-after": String(wait) });
    const id = await shareId(text);
    await bot.saveShare(id, text);
    // Warm the link preview, so an unfurl finds it ready.
    ctx.waitUntil(bot.preview(id, null));
    return Response.json({ id });
  }
  if (request.method !== "GET" && request.method !== "HEAD") return plain("method not allowed", 405);
  let snippet = EMPTY;
  let shared: string | null = null;
  let og = {
    title: "rawr playground",
    description: "Write Rust and run it. iced code shows its last frame. An app that moves shows a GIF. Other programs show their output.",
    url: `${SITE}/play`,
    image: `${SITE}/og/invite.png`,
    alt: "rawr: iced code and the frame it renders",
    width: 1200,
    height: 630,
  } as Og;
  if (path !== "/play") {
    const id = path.slice("/play/".length);
    const text = ID_PATTERN.test(id) ? await bot.share(id) : null;
    if (text === null) return plain("no such share", 404);
    const parsed = parseSnippet(JSON.parse(text));
    if (typeof parsed === "string") return plain("this share cannot be read", 500);
    snippet = parsed;
    shared = id;
    ctx.waitUntil(bot.visit(id));
    const info = await bot.previewInfo(id);
    const output = info && "output" in info ? parseTextResult(info.output) : null;
    og = output
      ? {
          title: "rawr playground",
          description: firstOutputLine(output) || firstLine(snippet.code) || "A Rust program and its output",
          url: `${SITE}/play/${id}`,
          image: `${SITE}/play/${id}.png`,
          alt: "rawr is rust augmented with rust: a Rust interpreter for your dev builds",
          width: 1200,
          height: 630,
        }
      : {
          title: "rawr playground",
          description: firstLine(snippet.code) || "An iced snippet rendered by rawr",
          url: `${SITE}/play/${id}`,
          image: `${SITE}/play/${id}.png`,
          alt: "The last frame of this snippet, rendered by rawr",
          ...(info && "width" in info ? info : {}),
        };
  }
  const page = renderPlay({ snippet, examples: EXAMPLES, sitekey: env.TURNSTILE_SITEKEY ?? null, pass: await hasPass(request, env), shared, og });
  return html(request.method === "HEAD" ? "" : page);
}

export default {
  async fetch(request: Request, env: Env, ctx: ExecutionContext): Promise<Response> {
    const url = new URL(request.url);
    const path = url.pathname;
    if (url.hostname === OLD_HOST) {
      // 308 keeps a POST a POST (a playground page still open on the old host).
      const status = request.method === "GET" || request.method === "HEAD" ? 301 : 308;
      return Response.redirect(`${SITE}${MOVED[path] ?? path}${url.search}`, status);
    }
    if (path === "/play" || path.startsWith("/play/")) {
      return play(request, env, ctx, path);
    }
    if (path === "/bot" && (request.method === "GET" || request.method === "HEAD")) {
      const html = render(await getContainer(env.PLAY_BOT).dashboard());
      return new Response(request.method === "HEAD" ? null : html, {
        headers: { "content-type": "text/html; charset=utf-8", "cache-control": "no-store" },
      });
    }
    if (DOCS_MOVED[path]) {
      return Response.redirect(`${SITE}${DOCS_MOVED[path]}${url.search}`, 301);
    }
    if (path !== "/bot/health") {
      return env.ASSETS.fetch(request);
    }
    const running = await getContainer(env.PLAY_BOT).ensure();
    return Response.json({ running }, { status: running ? 200 : 503 });
  },

  async scheduled(_controller: ScheduledController, env: Env, ctx: ExecutionContext) {
    const bot = getContainer(env.PLAY_BOT);
    ctx.waitUntil(bot.ensure());
    ctx.waitUntil(
      bot.purge().then(async (removed) => {
        for (const id of removed ?? []) {
          await caches.default.delete(new Request(`${SITE}/play/${id}.png`)).catch(() => false);
        }
      }),
    );
  },
} satisfies ExportedHandler<Env>;
