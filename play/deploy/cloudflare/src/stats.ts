// What the dashboard shows, kept in the PlayBot Durable Object: the bot's
// last status, hourly render counts with a duration histogram for the last
// day, lifetime totals, per-server totals, and the last boot events. All
// aggregate: no message content, user IDs or server names ever reach here,
// and a server appears only as its token (one AES block of its ID under a
// key the Worker never has; `rawr-play decrypt-server` reads it locally).

/** Upper bounds (ms) of the render-time histogram's buckets. */
export const BOUNDS = [
  10, 15, 20, 30, 50, 75, 100, 150, 200, 300, 500, 750, 1000, 1500, 2000, 3000, 5000, 7500, 10000,
  15000, Infinity,
];
export const HOURS = 24;
export const MAX_EVENTS = 50;
export const MAX_DURATIONS = 10_000;
export const MAX_SERVERS = 1000;

export interface Counts {
  ok: number;
  code: number;
  limit: number;
  rate_limited: number;
  /** Playground renders and refusals, apart from Discord's. */
  web_ok?: number;
  web_code?: number;
  web_limit?: number;
  web_rate_limited?: number;
}

const KEYS = ["ok", "code", "limit", "rate_limited", "web_ok", "web_code", "web_limit", "web_rate_limited"] as const;

export interface Hour extends Counts {
  /** Unix hour (ms / 3_600_000, floored). */
  hour: number;
  hist: number[];
}

export interface Status {
  version: string;
  boot: number;
  ready: number | null;
  servers: number;
  channels: number;
  channels_loaded: boolean;
  /** Unix seconds when the Durable Object received it. */
  seen: number;
}

/** One server's lifetime counts, by token. */
export interface Server extends Counts {
  /** Channels chosen with /play-channel, as last reported. */
  channels: number;
  /** Unix seconds of its last render or rate limit. */
  last: number | null;
}

export interface Stats {
  hours: Hour[];
  totals: Counts;
  status: Status | null;
  servers?: Record<string, Server>;
}

/** A per-server row in a report: counts since the last report. */
export interface ServerRow extends Server {
  server: string;
}

export interface Event {
  /** Unix seconds. */
  t: number;
  source: "bot" | "worker";
  phase: string;
  detail: string;
}

/** A report body from the bot: counts since its last report, and its status. */
export interface Report extends Counts {
  version: string;
  boot: number;
  ready: number | null;
  servers: number;
  channels: number;
  channels_loaded: boolean;
  durations_ms: number[];
  per_server: ServerRow[];
}

export function emptyStats(): Stats {
  return { hours: [], totals: zero(), status: null };
}

function zero(): Required<Counts> {
  return { ok: 0, code: 0, limit: 0, rate_limited: 0, web_ok: 0, web_code: 0, web_limit: 0, web_rate_limited: 0 };
}

/** The hour bucket for `now`, made if missing. */
function hourOf(stats: Stats, now: number): Hour {
  const hour = Math.floor(now / 3_600_000);
  let current = stats.hours.find((h) => h.hour === hour);
  if (!current) {
    current = { hour, ...zero(), hist: BOUNDS.map(() => 0) };
    stats.hours.push(current);
  }
  stats.hours = stats.hours.filter((h) => h.hour > hour - HOURS).sort((a, b) => a.hour - b.hour);
  return current;
}

/** A playground render refused by the per-IP limit. */
export function addWebRateLimited(stats: Stats, now: number): Stats {
  const current = hourOf(stats, now);
  current.web_rate_limited = (current.web_rate_limited ?? 0) + 1;
  stats.totals.web_rate_limited = (stats.totals.web_rate_limited ?? 0) + 1;
  return stats;
}

const count = (x: unknown): x is number => Number.isSafeInteger(x) && (x as number) >= 0;

/** The report in `body`, or null when it is not one. */
export function parseReport(body: unknown): Report | null {
  if (typeof body !== "object" || body === null) return null;
  const r = body as Record<string, unknown>;
  const ok =
    typeof r.version === "string" &&
    r.version.length <= 64 &&
    count(r.boot) &&
    (r.ready === null || count(r.ready)) &&
    count(r.servers) &&
    count(r.channels) &&
    typeof r.channels_loaded === "boolean" &&
    count(r.ok) &&
    count(r.code) &&
    count(r.limit) &&
    count(r.rate_limited) &&
    [r.web_ok, r.web_code, r.web_limit].every((x) => x === undefined || count(x)) &&
    Array.isArray(r.durations_ms) &&
    r.durations_ms.length <= MAX_DURATIONS &&
    r.durations_ms.every(count) &&
    (r.per_server === undefined ||
      (Array.isArray(r.per_server) &&
        r.per_server.length <= MAX_SERVERS &&
        r.per_server.every(serverRow)));
  if (!ok) return null;
  return {
    ...(r as unknown as Report),
    web_ok: (r.web_ok as number | undefined) ?? 0,
    web_code: (r.web_code as number | undefined) ?? 0,
    web_limit: (r.web_limit as number | undefined) ?? 0,
    web_rate_limited: 0,
    per_server: (r.per_server as ServerRow[] | undefined) ?? [],
  };
}

function serverRow(x: unknown): boolean {
  if (typeof x !== "object" || x === null) return false;
  const r = x as Record<string, unknown>;
  return (
    typeof r.server === "string" &&
    /^[0-9a-f]{32}$/.test(r.server) &&
    count(r.ok) &&
    count(r.code) &&
    count(r.limit) &&
    count(r.rate_limited) &&
    count(r.channels) &&
    (r.last === null || count(r.last))
  );
}

export function bucket(ms: number): number {
  return BOUNDS.findIndex((bound) => ms <= bound);
}

/** Add a report received at `now` (ms) into `stats`; returns it. */
export function merge(stats: Stats, report: Report, now: number): Stats {
  const current = hourOf(stats, now);
  for (const key of KEYS) {
    current[key] = (current[key] ?? 0) + (report[key] ?? 0);
    stats.totals[key] = (stats.totals[key] ?? 0) + (report[key] ?? 0);
  }
  for (const ms of report.durations_ms) {
    current.hist[bucket(ms)]++;
  }
  const servers = (stats.servers ??= {});
  for (const row of report.per_server) {
    const s = (servers[row.server] ??= { ok: 0, code: 0, limit: 0, rate_limited: 0, channels: 0, last: null });
    for (const key of ["ok", "code", "limit", "rate_limited"] as const) s[key] += row[key];
    s.channels = row.channels;
    if (row.last !== null) s.last = Math.max(s.last ?? 0, row.last);
  }
  const tokens = Object.keys(servers);
  if (tokens.length > MAX_SERVERS) {
    tokens
      .sort((a, b) => (servers[a].last ?? 0) - (servers[b].last ?? 0))
      .slice(0, tokens.length - MAX_SERVERS)
      .forEach((token) => delete servers[token]);
  }
  stats.status = {
    version: report.version,
    boot: report.boot,
    ready: report.ready,
    servers: report.servers,
    channels: report.channels,
    channels_loaded: report.channels_loaded,
    seen: Math.floor(now / 1000),
  };
  return stats;
}

export interface Summary {
  totals: Required<Counts> & { renders: number; web: number };
  day: Required<Counts> & { renders: number; web: number };
  /** Upper bounds of the median and 95th-percentile buckets; null with no renders. */
  p50: number | null;
  p95: number | null;
}

/** The last 24 hours as of `now` (ms), and lifetime totals. */
export function summarize(stats: Stats, now: number): Summary {
  const hour = Math.floor(now / 3_600_000);
  const day = zero();
  const hist = BOUNDS.map(() => 0);
  for (const h of stats.hours) {
    if (h.hour <= hour - HOURS) continue;
    for (const key of KEYS) day[key] += h[key] ?? 0;
    h.hist.forEach((n, i) => (hist[i] += n));
  }
  const renders = (c: Required<Counts>) => c.ok + c.code + c.limit;
  const web = (c: Required<Counts>) => c.web_ok + c.web_code + c.web_limit;
  const totals = { ...zero(), ...stats.totals };
  return {
    totals: { ...totals, renders: renders(totals), web: web(totals) },
    day: { ...day, renders: renders(day), web: web(day) },
    p50: percentile(hist, 0.5),
    p95: percentile(hist, 0.95),
  };
}

/** The upper bound of the bucket holding the `q` quantile. */
export function percentile(hist: number[], q: number): number | null {
  const total = hist.reduce((a, b) => a + b, 0);
  if (total === 0) return null;
  const rank = Math.ceil(q * total);
  let seen = 0;
  for (let i = 0; i < hist.length; i++) {
    seen += hist[i];
    if (seen >= rank) return BOUNDS[i];
  }
  return BOUNDS[BOUNDS.length - 1];
}

/** `events` with `event` appended, keeping the last MAX_EVENTS. */
export function pushEvent(events: Event[], event: Event): Event[] {
  return [...events, event].slice(-MAX_EVENTS);
}

/** An event from the bot's body, or null. */
export function parseEvent(body: unknown, t: number): Event | null {
  if (typeof body !== "object" || body === null) return null;
  const { phase, detail } = body as Record<string, unknown>;
  if (typeof phase !== "string" || phase.length === 0 || typeof detail !== "string") return null;
  return { t, source: "bot", phase: phase.slice(0, 64), detail: detail.slice(0, 500) };
}
