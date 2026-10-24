// The playground page at /play and /play/<id>: server-rendered HTML in
// RawrGallery's Ink sheet. A small inline script runs the snippet through
// POST /play/render and shares it through POST /play/share. A run shows an
// iced snippet's last frame or its GIF, or a plain program's output as a terminal.

import { escape } from "./dashboard.ts";
import { ogTags, type Og, type Snippet } from "./play.ts";

export interface Example {
  name: string;
  code: string;
  /** Messages sent before the preview. */
  messages?: string[];
  /** The canvas size, WxH, when the default would cut the view off. */
  size?: string;
}

export interface Page {
  snippet: Snippet;
  examples: Example[];
  /** Turnstile site key; no widget without one. */
  sitekey: string | null;
  /** The browser holds a valid pass: no widget unless the server refuses it. */
  pass?: boolean;
  /** The share this page was opened from. */
  shared: string | null;
  /** The link preview. */
  og: Og;
}

const TURNSTILE_JS = "https://challenges.cloudflare.com/turnstile/v0/api.js?render=explicit";

/** JSON safe inside a <script> element. */
function scriptJson(value: unknown): string {
  return JSON.stringify(value).replace(/</g, "\\u003c").replace(/>/g, "\\u003e").replace(/&/g, "\\u0026");
}

export function render(page: Page): string {
  const { snippet, examples, sitekey, shared } = page;
  const pass = Boolean(sitekey && page.pass);
  const options = examples
    .map((e, i) => `<option value="${i}">${escape(e.name)}</option>`)
    .join("");
  const theme = (t: string) =>
    `<label><input type="radio" name="theme" value="${t}"${snippet.theme === t ? " checked" : ""}><span>${t}</span></label>`;
  return `<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="UTF-8">
<meta name="viewport" content="width=device-width, initial-scale=1.0">
<title>rawr playground</title>
<link rel="icon" href="/play.svg" type="image/svg+xml">
${ogTags(page.og, escape)}
<meta name="theme-color" content="#ff2e93">
${sitekey && !pass ? `<script src="${TURNSTILE_JS}" async defer></script>` : ""}
<style>
:root { --bg: #141416; --raised: #1b1b1e; --inset: #101012; --text: #e4e4e8; --dim: #9a9aa3; --line: #2e2e33; --pink: #ff2e93; }
* { margin: 0; padding: 0; box-sizing: border-box; }
html, body { height: 100%; }
body { background: var(--bg); color: var(--text); font: 15px/1.5 "Fira Sans", system-ui, sans-serif; display: flex; flex-direction: column; }
a { color: var(--pink); text-decoration: none; }
.cap, .bar, label span, button, select, footer { font: 11px/1.4 "Fira Mono", ui-monospace, monospace; letter-spacing: 0.06em; text-transform: uppercase; }
.cap { color: var(--dim); }
.brand b { text-transform: none; }
.pink { color: var(--pink); }
.bar { display: flex; align-items: center; justify-content: space-between; gap: 16px; padding: 14px 24px; border-bottom: 1px solid var(--line); color: var(--dim); }
.brand { display: flex; align-items: center; gap: 12px; color: inherit; text-decoration: none; }
.brand:hover b { color: var(--pink); }
.brand:focus-visible { outline: 2px solid var(--text); outline-offset: 4px; }
/* The play triangle of /play.svg (M11 8 L25 16 L11 24 Z in 32x32), in the square's 10px box. */
.mark { width: 10px; height: 10px; background: var(--pink); clip-path: polygon(6.25% 0, 93.75% 50%, 6.25% 100%); }
.brand b { font-weight: 400; color: var(--text); font-size: 13px; letter-spacing: 0.02em; }
.links { display: flex; gap: 16px; }
.tools { display: flex; flex-wrap: wrap; align-items: center; gap: 8px; padding: 12px 24px; border-bottom: 1px solid var(--line); }
button, select, input[type=text] { height: 30px; border: 1px solid var(--line); background: var(--raised); color: var(--text); padding: 0 12px; border-radius: 0; }
input[type=text] { font: 13px "Fira Mono", ui-monospace, monospace; width: 9em; text-transform: none; }
button { cursor: pointer; }
button:hover, select:hover { border-color: var(--dim); }
button.run { background: var(--pink); border-color: var(--pink); color: var(--bg); }
button:disabled { opacity: 0.6; cursor: default; }
.seg { display: inline-flex; }
.seg input { display: none; }
.seg span { display: inline-block; height: 30px; line-height: 28px; padding: 0 12px; border: 1px solid var(--line); background: var(--raised); color: var(--text); cursor: pointer; }
.seg label + label span { border-left: 0; }
.seg input:checked + span { background: var(--pink); border-color: var(--pink); color: var(--bg); }
.grow { flex: 1; }
main { flex: 1; display: grid; grid-template-columns: minmax(0, 1fr) minmax(0, 1fr); min-height: 0; }
.pane { display: flex; flex-direction: column; min-height: 0; padding: 16px 24px 24px; }
.pane + .pane { border-left: 1px solid var(--line); }
.pane > .cap { margin-bottom: 8px; display: flex; justify-content: space-between; gap: 8px; }
textarea { width: 100%; resize: vertical; background: var(--inset); color: var(--text); border: 1px solid var(--line); border-radius: 0; padding: 12px; font: 13px/1.55 "Fira Mono", ui-monospace, monospace; tab-size: 4; outline: none; }
textarea:focus { border-color: var(--dim); }
#code { flex: 1; min-height: 360px; white-space: pre; overflow: auto; }
#messages { height: 4.6em; white-space: pre; }
.msgcap { margin: 12px 0 6px; }
.out { flex: 1; min-height: 240px; border: 1px solid var(--line); background: var(--inset); display: grid; place-items: center; overflow: auto; padding: 16px; }
.out img { max-width: 100%; height: auto; image-rendering: auto; }
.out pre { place-self: start stretch; white-space: pre-wrap; overflow-wrap: anywhere; font: 13px/1.55 "Fira Mono", ui-monospace, monospace; }
.out pre.error { color: var(--text); border-left: 2px solid var(--pink); padding-left: 12px; }
.out pre.term { color: var(--text); }
.term .err { color: #ff8fc4; }
.term .exit, .term .none, .term .note { color: var(--dim); }
#messages.unused { opacity: 0.45; }
.note { color: var(--dim); }
.plain { text-transform: none; letter-spacing: 0; }
#ts { margin-top: 12px; }
footer { display: flex; justify-content: space-between; gap: 16px; flex-wrap: wrap; padding: 12px 24px; border-top: 1px solid var(--line); color: var(--dim); }
@media (max-width: 760px) {
  html, body { height: auto; }
  main { display: block; }
  .pane { min-height: 0; }
  .out { min-height: 200px; }
  .pane + .pane { border-left: 0; border-top: 1px solid var(--line); }
  .bar, .tools, .pane, footer { padding-left: 16px; padding-right: 16px; }
  #code { min-height: 300px; }
  .links a.status { display: none; }
}
</style>
</head>
<body>
<header class="bar"><a class="brand" href="/" aria-label="rawr home, playground"><span class="mark" aria-hidden="true"></span><b>rawr</b><span>Playground</span></a><span class="links"><a href="/docs">Docs</a><a href="/docs/play">Help</a><a class="status" href="/bot">Status</a><a href="/bot/invite">Add to Discord</a></span></header>
<div class="tools">
<select id="examples" aria-label="Examples"><option value="">Examples</option>${options}<option value="reset">Reset</option></select>
<span class="seg">${theme("dark")}${theme("light")}</span>
<input type="text" id="size" placeholder="size: auto" value="${escape(snippet.size)}" aria-label="Size, WxH" spellcheck="false">
<span class="grow"></span>
<button id="share" type="button">Share</button>
<button id="run" class="run" type="button" title="Ctrl/Cmd + Enter">Run</button>
</div>
<main>
<section class="pane">
<p class="cap"><span><span class="pink">//</span> Code</span></p>
<textarea id="code" spellcheck="false" autocapitalize="off" autocomplete="off" aria-label="Code">${escape(snippet.code)}</textarea>
<p class="cap msgcap">Messages <span class="plain" id="msghint">(sent before preview, separated by newline)</span></p>
<textarea id="messages" spellcheck="false" placeholder="Message::Foo" aria-label="Messages">${escape(snippet.messages.join("\n"))}</textarea>
${sitekey ? '<div id="ts"></div>' : ""}
</section>
<section class="pane">
<p class="cap"><span><span class="pink">//</span> Output</span><span id="status">${shared ? `shared · ${escape(shared)}` : "ready"}</span></p>
<div class="out" id="out" aria-live="polite"><p class="note">Run (Ctrl/Cmd + Enter). iced code shows its last frame. An app that moves shows a GIF. Other programs show their output.</p></div>
</section>
</main>
<footer><span>MIT or Apache-2.0</span><a href="https://airstrike.fyi">airstrike.fyi</a></footer>
<script type="application/json" id="example-data">${scriptJson(examples)}</script>
<script>
(() => {
  const $ = (id) => document.getElementById(id);
  const code = $("code"), messages = $("messages"), out = $("out"), status = $("status");
  const examples = JSON.parse($("example-data").textContent);
  const sitekey = ${scriptJson(sitekey)};
  const shared = ${scriptJson(shared)};
  // A valid pass cookie skips Turnstile; the widget mounts only without one.
  let pass = ${scriptJson(pass)};
  // The latest snippet stays in this browser, as on the Rust Playground:
  // bare /play restores it; a share shows the share until it is edited.
  const DRAFT = "rawr-play-draft";
  const radios = () => document.querySelectorAll("input[name=theme]");
  const fill = (s) => {
    code.value = s.code; messages.value = (s.messages || []).join("\\n"); $("size").value = s.size || "";
    radios().forEach((r) => { r.checked = r.value === (s.theme === "light" ? "light" : "dark"); });
  };
  const save = () => { try { localStorage.setItem(DRAFT, JSON.stringify(snippet())); } catch {} };
  let saving = null;
  const edited = () => { clearTimeout(saving); saving = setTimeout(save, 400); };
  let widget = null, token = null, busy = false, url = null;
  const mount = () => {
    if (widget !== null) return;
    if (!window.turnstile) {
      if (!document.querySelector("script[data-turnstile]")) {
        const js = document.createElement("script"); js.src = ${scriptJson(sitekey ? TURNSTILE_JS : "")}; js.async = true; js.dataset.turnstile = "";
        document.head.append(js);
      }
      return setTimeout(mount, 200);
    }
    widget = turnstile.render("#ts", { sitekey, appearance: "interaction-only", theme: "dark",
      callback: (t) => { token = t; }, "expired-callback": () => { token = null; }, "refresh-expired": "auto" });
  };
  if (sitekey && !pass) mount();
  const snippet = () => ({
    code: code.value,
    theme: document.querySelector('input[name=theme]:checked').value,
    size: $("size").value.trim(),
    messages: messages.value.split("\\n"),
  });
  if (!shared) {
    try {
      const draft = JSON.parse(localStorage.getItem(DRAFT) || "null");
      if (draft && typeof draft.code === "string" && draft.code.trim()) fill(draft);
    } catch {}
  }
  for (const node of [code, messages, $("size")]) node.addEventListener("input", edited);
  radios().forEach((r) => r.addEventListener("change", edited));
  const show = (node) => { out.replaceChildren(node); };
  const text = (message, cls) => { const pre = document.createElement("pre"); pre.textContent = message; if (cls) pre.className = cls; return pre; };
  // A plain program's output: stdout, stderr in its own colour, then the exit status if not 0.
  const terminal = (result) => {
    const pre = text("", "term");
    const add = (value, cls) => { const span = document.createElement("span"); span.textContent = value; if (cls) span.className = cls; pre.append(span); };
    for (const chunk of result.output || []) add(chunk.text, chunk.stream === "stderr" ? "err" : "");
    if (!(result.output || []).some((chunk) => chunk.text !== "")) add("(no output)\\n", "none");
    if (result.status !== 0) add("exit status " + result.status + "\\n", "exit");
    if (result.note) add("\\n" + result.note + "\\n", "note");
    return pre;
  };
  // Messages drive an iced app: after a plain program's output they are dimmed, not sent.
  const HINT = $("msghint").textContent;
  const markMessages = (plain) => {
    messages.classList.toggle("unused", plain);
    $("msghint").textContent = plain ? "(ignored for programs without iced)" : HINT;
  };
  const human = (token) => new Promise((resolve) => {
    if (!sitekey || pass) return resolve(null);
    const start = Date.now();
    const wait = () => token() ? resolve(token()) : Date.now() - start > 15000 ? resolve(null) : setTimeout(wait, 150);
    wait();
  });
  async function run(retry = true) {
    if (busy) return;
    save();
    busy = true; $("run").disabled = true; status.textContent = "running…";
    const started = performance.now();
    try {
      const check = await human(() => token);
      if (sitekey && !pass && !check) { show(text("Waiting for the browser check; try again in a moment.", "error")); return; }
      const sentPass = pass;
      const response = await fetch("/play/render", { method: "POST", headers: { "content-type": "application/json" },
        body: JSON.stringify({ ...snippet(), turnstile: check }) });
      if (response.status === 403 && sentPass && retry) {
        // The pass expired or was refused: fall back to Turnstile and run again.
        pass = false; mount(); busy = false;
        return await run(false);
      }
      // The server set a pass cookie: later runs skip the browser check.
      if (response.headers.get("x-play-pass")) pass = true;
      const ms = Math.round(performance.now() - started);
      const type = response.headers.get("content-type") || "";
      if (response.ok && type.startsWith("application/json")) {
        const result = await response.json();
        show(terminal(result)); markMessages(true); status.textContent = ms + " ms" + (result.status !== 0 ? " · exit " + result.status : "");
      } else if (response.ok && type.startsWith("image/")) {
        markMessages(false);
        if (url) URL.revokeObjectURL(url);
        url = URL.createObjectURL(await response.blob());
        const animated = type.startsWith("image/gif");
        const img = new Image(); img.src = url; img.alt = animated ? "The animation" : "The rendered frame";
        const note = response.headers.get("x-play-note");
        show(img); status.textContent = ms + " ms" + (note ? " · " + note : "");
      } else {
        show(text(await response.text(), "error"));
        status.textContent = response.status === 429 ? "slow down" : response.status === 422 ? "error" : "failed";
      }
    } catch (error) {
      show(text("Could not reach the playground: " + error.message, "error")); status.textContent = "failed";
    } finally {
      busy = false; $("run").disabled = false;
      if (widget !== null) {
        token = null;
        if (pass) { turnstile.remove(widget); widget = null; } else turnstile.reset(widget);
      }
    }
  }
  async function share() {
    status.textContent = "sharing…";
    const response = await fetch("/play/share", { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify(snippet()) });
    if (!response.ok) { status.textContent = "share failed"; show(text(await response.text(), "error")); return; }
    const { id } = await response.json();
    const link = location.origin + "/play/" + id;
    history.replaceState(null, "", "/play/" + id);
    try { await navigator.clipboard.writeText(link); status.textContent = "link copied · " + id; }
    catch { status.textContent = "shared · " + id; }
  }
  $("run").onclick = run;
  $("share").onclick = share;
  $("examples").onchange = (event) => {
    const example = event.target.value === "reset" ? examples[0] : examples[event.target.value];
    if (example) {
      fill({ code: example.code, messages: example.messages || [], theme: "dark", size: example.size || "" });
      save(); history.replaceState(null, "", "/play"); run();
    }
    event.target.value = "";
  };
  document.addEventListener("keydown", (event) => {
    if ((event.ctrlKey || event.metaKey) && event.key === "Enter") { event.preventDefault(); run(); }
  });
  code.addEventListener("keydown", (event) => {
    if (event.key !== "Tab" || event.ctrlKey || event.metaKey || event.altKey) return;
    event.preventDefault();
    const { selectionStart: start, selectionEnd: end, value } = code;
    const lineStart = value.lastIndexOf("\\n", start - 1) + 1;
    if (event.shiftKey) {
      const lines = value.slice(lineStart, end).split("\\n");
      let removed = 0, first = 0;
      const next = lines.map((line, i) => { const cut = line.match(/^ {1,4}/)?.[0].length ?? 0; if (i === 0) first = cut; removed += cut; return line.slice(cut); }).join("\\n");
      code.setRangeText(next, lineStart, end, "preserve");
      code.setSelectionRange(Math.max(lineStart, start - first), end - removed);
    } else if (start === end) {
      code.setRangeText("    ", start, end, "end");
    } else {
      const lines = value.slice(lineStart, end).split("\\n");
      code.setRangeText(lines.map((line) => "    " + line).join("\\n"), lineStart, end, "preserve");
      code.setSelectionRange(start + 4, end + 4 * lines.length);
    }
  });
})();
</script>
</body>
</html>
`;
}
