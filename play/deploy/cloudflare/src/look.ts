// The site's look for the bot's pages, which the site's build doesn't draw:
// the status page (/bot, dashboard.ts) and /bot/invite (public/, which
// build.mjs fills in). The colours are site/src/styles/tokens.css's, in both
// themes (build.mjs copies the file into tokens.gen.ts); the theme and the
// font gate are site/src/lib/theme-init.ts's, so the theme is the one the
// visitor picked on the site, under the same key. The type, rules and
// spacing are /todo's.

import { CHROME, FONT_GATE, KEY, THEME_INIT } from "../../../../site/src/lib/theme-init.ts";
import { TOKENS } from "./tokens.gen.ts";

const FONT = "https://abitaro-sans.pages.dev/abitaro-sans.css?v=0.300";

/**
 * The <head>'s theme, icon, font and tokens, in the order the site's pages
 * have them. The bot's pages take the playground's icon (site/public/play.svg).
 */
export const HEAD = `<meta name="theme-color" content="${CHROME.light}" data-follow>
<script>${THEME_INIT}</script>
<link rel="icon" href="/play.svg" type="image/svg+xml">
<link rel="preconnect" href="https://abitaro-sans.pages.dev" crossorigin>
<link rel="stylesheet" href="${FONT}">
<script>${FONT_GATE}</script>
<style>${TOKENS}</style>`;

/** /todo's page: the body, the header with its 4px rule, the footer, and the theme switch. */
export const BASE = `* { box-sizing: border-box; margin: 0; padding: 0; }
html[data-font="wait"] body { visibility: hidden; }
html { background: var(--paper); color: var(--ink); }
body {
  font-family: var(--sans); font-kerning: normal; font-size: var(--fs-base); font-weight: var(--w-base);
  line-height: 1.35; -webkit-font-smoothing: antialiased; font-variant-numeric: tabular-nums;
  padding: var(--pad) var(--pad) 96px;
}
a { color: inherit; text-decoration: none; border-bottom: 1px solid var(--grid); }
a:hover { border-bottom-color: currentColor; }
a:focus-visible, button:focus-visible { outline: 2px solid var(--red); outline-offset: 2px; }
.red { color: var(--red); }
.label { font-size: var(--fs-label); font-weight: 600; text-transform: uppercase; letter-spacing: var(--track-label); }
header.page { display: grid; grid-template-columns: 1fr auto; align-items: end; column-gap: 24px; border-bottom: var(--rule-thick) solid var(--rule); padding-bottom: 16px; }
.mark { font-weight: var(--w-heavy); font-size: var(--fs-base); letter-spacing: 0.01em; margin-bottom: 40px; }
.mark a { border: 0; }
.mark span { font-weight: var(--w-base); color: var(--quiet); margin-left: 8px; }
h1 { font-size: clamp(48px, 9vw, 128px); font-weight: var(--w-heavy); letter-spacing: var(--track-display); line-height: 0.88; }
.meta { text-align: right; font-size: var(--fs-small); line-height: 1.6; }
.meta b { font-weight: var(--w-strong); }
.theme {
  display: inline-grid; place-items: center; vertical-align: middle;
  width: 26px; height: 26px; padding: 0; margin: 0;
  border: 1px solid var(--grid); border-radius: 0;
  background: transparent; color: var(--quiet); cursor: pointer;
}
.theme:hover { color: var(--ink); border-color: var(--ink); }
.theme svg { width: 14px; height: 14px; display: block; }
[data-theme="dark"] .theme .moon, [data-theme="light"] .theme .sun { display: none; }
footer.page {
  margin-top: 56px; padding-top: 12px; border-top: var(--rule-thick) solid var(--rule);
  display: flex; flex-wrap: wrap; justify-content: space-between; gap: 4px 24px; font-size: var(--fs-label); color: var(--quiet);
}
@media (max-width: 640px) {
  body { padding: 24px var(--pad) 64px; }
  header.page { grid-template-columns: 1fr; }
  .meta { text-align: left; margin-top: 16px; }
}`;

/** The site bar's switch (site/src/components/ThemeSwitch.astro): a sun in the dark theme, a moon in the light one. */
export const SWITCH = `<button type="button" class="theme" aria-label="Switch theme" data-theme-switch><svg class="sun" viewBox="0 0 16 16" aria-hidden="true" focusable="false"><circle cx="8" cy="8" r="3" fill="currentColor"/><path d="M8 0.5v2.2M8 13.3v2.2M0.5 8h2.2M13.3 8h2.2M2.7 2.7l1.6 1.6M11.7 11.7l1.6 1.6M2.7 13.3l1.6-1.6M11.7 4.3l1.6-1.6" stroke="currentColor" stroke-width="1.4" fill="none"/></svg><svg class="moon" viewBox="0 0 16 16" aria-hidden="true" focusable="false"><path d="M13.6 10.2A6 6 0 0 1 5.8 2.4a6 6 0 1 0 7.8 7.8Z" fill="currentColor"/></svg></button>`;

/**
 * The switch's behaviour, as site/src/lib/theme.ts has it: a click picks the
 * other theme and remembers it; until the visitor picks, the page follows the
 * system. It goes at the end of the body.
 */
export const SWITCH_SCRIPT = `<script>(() => {
  const root = document.documentElement;
  const chrome = document.querySelector('meta[name="theme-color"][data-follow]');
  const buttons = document.querySelectorAll("[data-theme-switch]");
  const stored = () => { try { const v = localStorage.getItem(${JSON.stringify(KEY)}); return v === "dark" || v === "light" ? v : null; } catch { return null; } };
  const label = () => {
    const say = root.dataset.theme === "light" ? "Switch to the dark theme" : "Switch to the light theme";
    for (const b of buttons) { b.setAttribute("aria-label", say); b.title = say; }
  };
  const set = (theme, remember) => {
    if (remember) try { localStorage.setItem(${JSON.stringify(KEY)}, theme); } catch {}
    root.dataset.theme = theme;
    if (chrome) chrome.content = theme === "light" ? ${JSON.stringify(CHROME.light)} : ${JSON.stringify(CHROME.dark)};
    label();
  };
  for (const b of buttons) b.addEventListener("click", () => set(root.dataset.theme === "light" ? "dark" : "light", true));
  label();
  const query = matchMedia("(prefers-color-scheme: dark)");
  query.addEventListener("change", () => { if (!stored()) set(query.matches ? "dark" : "light", false); });
})();</script>`;
