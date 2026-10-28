// The dashboard at /: server-rendered HTML in RawrGallery's Ink sheet, from
// the PlayBot Durable Object's aggregate stats and boot events. No script.

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
    ? `${escape(s.version)}${stale ? ` <span class="pink">≠ deployed ${escape(d.deployed)}</span>` : ""}`
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
        `<tr><td class="t">${when(e.t)}</td><td class="src">${e.source}</td><td class="phase${
          /error|unreachable|stopped|restart/.test(e.phase) ? " pink" : ""
        }">${escape(e.phase)}</td><td>${escape(e.detail)}</td></tr>`,
    )
    .join("");
  return `<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="UTF-8">
<meta name="viewport" content="width=device-width, initial-scale=1.0">
<meta http-equiv="refresh" content="60">
<title>rawr play bot</title>
<meta name="description" content="Status and aggregate render stats of the rawr ?play Discord bot.">
<meta name="theme-color" content="#ff2e93">
<style>
:root { --bg: #141416; --raised: #1b1b1e; --text: #e4e4e8; --dim: #9a9aa3; --line: #2e2e33; --pink: #ff2e93; }
* { margin: 0; padding: 0; box-sizing: border-box; }
body { background: var(--bg); color: var(--text); font: 15px/1.5 "Fira Sans", system-ui, sans-serif; }
a { color: var(--pink); text-decoration: none; }
.cap, th, .bar, footer { font: 11px/1.4 "Fira Mono", ui-monospace, monospace; letter-spacing: 0.06em; text-transform: uppercase; color: var(--dim); }
.pink { color: var(--pink); }
.bar { display: flex; align-items: center; justify-content: space-between; gap: 16px; padding: 18px 24px; border-bottom: 1px solid var(--line); }
.brand { display: flex; align-items: center; gap: 12px; color: inherit; text-decoration: none; }
.brand:hover b { color: var(--pink); }
.brand:focus-visible { outline: 2px solid var(--text); outline-offset: 4px; }
.mark { width: 10px; height: 10px; background: var(--pink); }
.brand b { font-weight: 400; color: var(--text); font-size: 13px; letter-spacing: 0.02em; }
.bar a { border: 1px solid var(--pink); padding: 6px 12px; }
.links { display: flex; gap: 8px; }
main { max-width: 960px; margin: 0 auto; padding: 40px 24px; }
.kicker .pink { margin-right: 8px; }
h1 { font-weight: 400; font-size: 30px; margin: 10px 0 8px; }
h1.ready { color: var(--pink); }
.lede { color: var(--dim); padding-bottom: 20px; border-bottom: 1px solid var(--line); }
section { margin-top: 32px; }
section > .cap { margin-bottom: 10px; }
.brand b { text-transform: none; }
.grid { display: grid; grid-template-columns: repeat(auto-fill, minmax(150px, 1fr)); border-top: 1px solid var(--line); border-left: 1px solid var(--line); }
.cell { background: var(--raised); padding: 14px 16px; border-right: 1px solid var(--line); border-bottom: 1px solid var(--line); }
.num { font-size: 22px; margin-top: 6px; }
.note { color: var(--dim); font-size: 12px; }
.rows { border: 1px solid var(--line); background: var(--raised); }
.row { display: flex; justify-content: space-between; gap: 16px; padding: 10px 16px; border-top: 1px solid var(--line); }
.row:first-child { border-top: 0; }
.row span:last-child { text-align: right; overflow-wrap: anywhere; }
.scroll { overflow-x: auto; border: 1px solid var(--line); }
table { width: 100%; border-collapse: collapse; background: var(--raised); font-size: 13px; }
th, td { text-align: left; padding: 8px 12px; border-top: 1px solid var(--line); vertical-align: top; }
th { border-top: 0; font-weight: 400; }
td.t { white-space: nowrap; color: var(--dim); font-family: "Fira Mono", ui-monospace, monospace; font-size: 12px; }
td.src { color: var(--dim); }
td.phase { white-space: nowrap; }
footer { display: flex; justify-content: space-between; gap: 16px; flex-wrap: wrap; padding: 18px 24px; border-top: 1px solid var(--line); }
@media (max-width: 600px) { .bar, main, footer { padding-left: 16px; padding-right: 16px; } main { padding-top: 28px; } }
</style>
</head>
<body>
<header class="bar"><a class="brand" href="/" aria-label="rawr home, play bot"><span class="mark" aria-hidden="true"></span><b>rawr</b><span>Play bot</span></a><span class="links"><a href="/play">Playground</a><a href="/bot/invite">Add to a server</a></span></header>
<main>
<p class="cap kicker"><span class="pink">//</span>Status</p>
<h1 class="${st}">${title}</h1>
${line ? `<p class="lede">${line}</p>` : ""}
<section>
<p class="cap">Last 24 hours</p>
<div class="grid">
${cell("Renders", n(day.renders), `${n(totals.renders)} in all`)}
${cell("Images", n(day.ok))}
${cell("Code errors", n(day.code), `${n(totals.code)} in all`)}
${cell("Limits", n(day.limit))}
${cell("Median", ms(d.summary.p50))}
${cell("p95", ms(d.summary.p95))}
${cell("Rate-limited", n(day.rate_limited), `${n(totals.rate_limited)} in all`)}
</div>
</section>
<section>
<p class="cap">Playground · last 24 hours</p>
<div class="grid">
${cell("Runs", n(day.web), `${n(totals.web)} in all`)}
${cell("Images", n(day.web_ok))}
${cell("Code errors", n(day.web_code))}
${cell("Limits", n(day.web_limit))}
${cell("Shares", d.purge ? n(d.purge.stored) : "—", d.purge ? `${n(d.purge.purged)} purged last pass` : "")}
${cell("Rate-limited", n(day.web_rate_limited))}
</div>
</section>
<section>
<p class="cap">Bot</p>
<div class="grid">
${cell("Servers", s ? n(s.servers) : "—")}
${cell("Channels", s ? n(s.channels) : "—", s && !s.channels_loaded ? '<span class="pink">not loaded</span>' : "")}
</div>
<div class="rows" style="margin-top:16px">
<div class="row"><span class="cap">Image</span><span>${version}</span></div>
<div class="row"><span class="cap">Container</span><span>${d.running ? "running" : "stopped"}${
    d.started ? ` · reports ${escape(d.started)}` : ""
  }</span></div>
<div class="row"><span class="cap">Booted</span><span>${when(s?.boot)}</span></div>
<div class="row"><span class="cap">Ready</span><span>${when(s?.ready)}</span></div>
<div class="row"><span class="cap">Last report</span><span>${ago(d.now, s?.seen)}</span></div>
</div>
</section>
<section>
<p class="cap"><span class="pink">//</span> Servers, by token</p>
<div class="scroll"><table>
<tr><th>Token</th><th>Renders</th><th>Code errors</th><th>Limits</th><th>Rate-limited</th><th>Channels</th><th>Last</th></tr>
${servers || '<tr><td colspan="7" class="note">None yet.</td></tr>'}
</table></div>
</section>
<section>
<p class="cap"><span class="pink">//</span> Events</p>
<div class="scroll"><table>
<tr><th>Time</th><th>From</th><th>Phase</th><th>Detail</th></tr>
${events || '<tr><td colspan="4" class="note">No events yet.</td></tr>'}
</table></div>
</section>
</main>
<footer><span>Aggregate only · no messages, users or server names</span><span>${when(d.now)}</span><a href="https://airstrike.fyi">airstrike.fyi</a></footer>
</body>
</html>
`;
}
