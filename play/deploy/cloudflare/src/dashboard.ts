// The status page at /bot: server-rendered HTML in the site's look (look.ts),
// from the PlayBot Durable Object's aggregate stats and boot events. Its only
// scripts are the site's theme, font gate and switch.

import { BASE, HEAD, SWITCH, SWITCH_SCRIPT } from "./look.ts";
import type { Event, Server, Status, Summary } from "./stats.ts";

export interface Dashboard {
  summary: Summary;
  status: Status | null;
  events: Event[];
  /** Lifetime counts by server token. */
  servers: Record<string, Server>;
  /** Whether the Durable Object's container is running. */
  running: boolean;
  /** The image the Worker deploys (PLAY_VERSION). */
  deployed: string;
  /** The image the running bot reported since its container started, if it has reported. */
  started: string | null;
  /** The last daily purge of unvisited shares. */
  purge?: { day: number; stored: number; purged: number } | null;
  /** Unix seconds. */
  now: number;
}

/** Seconds without a report before the bot counts as silent. */
export const SILENT_AFTER = 180;

export type State = "ready" | "starting" | "silent" | "stopped";

export function state(d: Dashboard): State {
  if (!d.running) return "stopped";
  const s = d.status;
  if (!s || d.now - s.seen > SILENT_AFTER) return "silent";
  return s.ready ? "ready" : "starting";
}

const HEADLINE: Record<State, [string, string]> = {
  ready: ["Ready", ""],
  starting: ["Starting", "Connecting to Discord."],
  silent: ["Silent", "No report for over 3 minutes."],
  stopped: ["Stopped", "Restarts within 5 minutes."],
};

export function escape(text: string): string {
  return text.replace(/[&<>"']/g, (c) => `&#${c.charCodeAt(0)};`);
}

function when(seconds: number | null | undefined): string {
  if (!seconds) return "—";
  return new Date(seconds * 1000).toISOString().replace("T", " ").slice(0, 19) + " UTC";
}

function ago(now: number, seconds: number | null | undefined): string {
  if (!seconds) return "—";
  const d = Math.max(0, now - seconds);
  if (d < 90) return `${d} s ago`;
  if (d < 5400) return `${Math.round(d / 60)} min ago`;
  if (d < 172800) return `${Math.round(d / 3600)} h ago`;
  return `${Math.round(d / 86400)} d ago`;
}

function ms(bound: number | null): string {
  if (bound === null) return "—";
  if (bound === Infinity) return "&gt; 15 s";
  return bound >= 1000 ? `≤ ${bound / 1000} s` : `≤ ${bound} ms`;
}

const n = (x: number) => x.toLocaleString("en-US");

function cell(label: string, value: string, note = ""): string {
  return `<div class="cell"><p class="cap">${label}</p><p class="num">${value}</p>${
    note ? `<p class="note">${note}</p>` : ""
  }</div>`;
}

export function render(d: Dashboard): string {
  const st = state(d);
  const [title, line] = HEADLINE[st];
  const s = d.status;
  const { day, totals } = d.summary;
  const stale = s && s.version !== d.deployed;
  const version = s
    ? `${escape(s.version)}${stale ? ` <span class="red">≠ deployed ${escape(d.deployed)}</span>` : ""}`
    : "—";
  const servers = Object.entries(d.servers)
    .sort(([, a], [, b]) => (b.last ?? 0) - (a.last ?? 0) || b.channels - a.channels)
    .map(
      ([token, s]) =>
        `<tr><td class="t">${escape(token)}</td><td>${n(s.ok + s.code + s.limit)}</td><td>${n(
          s.code,
        )}</td><td>${n(s.limit)}</td><td>${n(s.rate_limited)}</td><td>${n(s.channels)}</td><td class="t">${ago(
          d.now,
          s.last,
        )}</td></tr>`,
    )
    .join("");
  const events = [...d.events]
    .reverse()
    .map(
      (e) =>
        `<tr><td class="t">${when(e.t).replace(/^(\S+) (.*)$/, "<span>$1</span> <span>$2</span>")}</td><td class="src">${e.source}</td><td class="phase${
          /error|unreachable|stopped|restart/.test(e.phase) ? " red" : ""
        }">${escape(e.phase)}</td><td class="detail">${escape(e.detail)}</td></tr>`,
    )
    .join("");
  return `<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="UTF-8">
<meta name="viewport" content="width=device-width, initial-scale=1.0">
<meta http-equiv="refresh" content="60">
${HEAD}
<title>rawr play bot</title>
<meta name="description" content="Status and aggregate render stats of the rawr ?play Discord bot.">
<style>
${BASE}
.kicker { margin-bottom: 10px; }
h1.silent, h1.stopped, h1.starting { color: var(--red); }
.lede { max-width: 46rem; padding: 20px 0; font-size: var(--fs-read); border-bottom: 1px solid var(--rule); }
.meta .links a + a { margin-left: 12px; }
section { margin-top: 32px; }
section > h2 {
  font-size: var(--fs-small); font-weight: var(--w-heavy); text-transform: uppercase; letter-spacing: 0.06em;
  border-top: var(--rule-mid) solid var(--rule); padding: 9px 0 7px;
}
/* The counters: /todo's figures, which wrap. The grid sits 17px left of its
   clip, so each row's first rule is hidden and its text lines up with the page. */
.figures { overflow: hidden; border-top: 1px solid var(--rule); border-bottom: 1px solid var(--rule); }
.grid { display: grid; grid-template-columns: repeat(auto-fill, minmax(160px, 1fr)); margin: 0 0 -1px -17px; }
.cell { padding: 16px 16px 18px; border-left: 1px solid var(--rule); border-bottom: 1px solid var(--grid); }
.num { font-size: 40px; font-weight: var(--w-heavy); letter-spacing: -0.045em; line-height: 1; margin-top: 8px; }
.note { color: var(--quiet); font-size: var(--fs-label); margin-top: 6px; }
.rows { margin-top: 16px; border-top: 1px solid var(--rule); }
.row { display: grid; grid-template-columns: 13rem minmax(0, 1fr); column-gap: 20px; align-items: baseline; padding: 7px 0; border-bottom: 1px solid var(--grid); }
.row span:last-child { overflow-wrap: anywhere; }
.scroll { overflow-x: auto; }
table { width: 100%; border-collapse: collapse; font-size: var(--fs-small); }
th, td { text-align: left; vertical-align: top; padding: 6px 12px 6px 0; border-bottom: 1px solid var(--grid); }
th { border-bottom: var(--rule-mid) solid var(--rule); font-weight: var(--w-strong); font-size: var(--fs-label); text-transform: uppercase; letter-spacing: var(--track-label); white-space: nowrap; }
td.detail { overflow-wrap: anywhere; }
td.t { white-space: nowrap; color: var(--quiet); font-family: var(--mono); font-size: var(--fs-label); }
td.src { color: var(--quiet); }
td.phase { white-space: nowrap; }
@media (max-width: 640px) {
  .num { font-size: 32px; }
  .grid { grid-template-columns: repeat(2, minmax(0, 1fr)); }
  .row { grid-template-columns: 1fr; row-gap: 2px; }
  td.t:has(span) { white-space: normal; }
  td.t span { white-space: nowrap; }
}
</style>
</head>
<body>
<header class="page">
<div>
<p class="mark"><a href="/" aria-label="rawr home, play bot">rawr</a><span>Play bot</span></p>
<p class="label kicker"><span class="red">//</span> Status</p>
<h1 class="${st}">${title}</h1>
</div>
<p class="meta">${SWITCH}<br><span class="links"><a href="/play">Playground</a><a href="/bot/invite">Add to a server</a></span></p>
</header>
${line ? `<p class="lede">${line}</p>` : ""}
<section>
<h2>Last 24 hours</h2>
<div class="figures"><div class="grid">
${cell("Renders", n(day.renders), `${n(totals.renders)} in all`)}
${cell("Images", n(day.ok))}
${cell("Code errors", n(day.code), `${n(totals.code)} in all`)}
${cell("Limits", n(day.limit))}
${cell("Median", ms(d.summary.p50))}
${cell("p95", ms(d.summary.p95))}
${cell("Rate-limited", n(day.rate_limited), `${n(totals.rate_limited)} in all`)}
</div></div>
</section>
<section>
<h2>Playground · last 24 hours</h2>
<div class="figures"><div class="grid">
${cell("Runs", n(day.web), `${n(totals.web)} in all`)}
${cell("Images", n(day.web_ok))}
${cell("Code errors", n(day.web_code))}
${cell("Limits", n(day.web_limit))}
${cell("Shares", d.purge ? n(d.purge.stored) : "—", d.purge ? `${n(d.purge.purged)} purged last pass` : "")}
${cell("Rate-limited", n(day.web_rate_limited))}
</div></div>
</section>
<section>
<h2>Bot</h2>
<div class="figures"><div class="grid">
${cell("Servers", s ? n(s.servers) : "—")}
${cell("Channels", s ? n(s.channels) : "—", s && !s.channels_loaded ? '<span class="red">not loaded</span>' : "")}
</div></div>
<div class="rows">
<div class="row"><span class="label">Image</span><span>${version}</span></div>
<div class="row"><span class="label">Container</span><span>${d.running ? "running" : "stopped"}${
    d.started ? ` · reports ${escape(d.started)}` : ""
  }</span></div>
<div class="row"><span class="label">Booted</span><span>${when(s?.boot)}</span></div>
<div class="row"><span class="label">Ready</span><span>${when(s?.ready)}</span></div>
<div class="row"><span class="label">Last report</span><span>${ago(d.now, s?.seen)}</span></div>
</div>
</section>
<section>
<h2><span class="red">//</span> Servers, by token</h2>
<div class="scroll"><table>
<tr><th>Token</th><th>Renders</th><th>Code errors</th><th>Limits</th><th>Rate-limited</th><th>Channels</th><th>Last</th></tr>
${servers || '<tr><td colspan="7" class="note">None yet.</td></tr>'}
</table></div>
</section>
<section>
<h2><span class="red">//</span> Events</h2>
<div class="scroll"><table>
<tr><th>Time</th><th>From</th><th>Phase</th><th>Detail</th></tr>
${events || '<tr><td colspan="4" class="note">No events yet.</td></tr>'}
</table></div>
</section>
<footer class="page"><span>Aggregate only · no messages, users or server names</span><span>${when(d.now)}</span><a href="https://airstrike.fyi">airstrike.fyi</a></footer>
${SWITCH_SCRIPT}
</body>
</html>
`;
}
