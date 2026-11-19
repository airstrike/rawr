// The site's look for the bot's pages, which the site's build doesn't draw:
// the status page (/bot, dashboard.ts) and /bot/invite (public/, which
// build.mjs fills in). They link the stylesheets every site page loads and
// carry the site bar and the footer as the built landing page has them
// (build.mjs copies them into site.gen.ts), so the colours, type, bar and
// footer are the site's, in both themes. The theme and the font gate are
// site/src/lib/theme-init.ts's, so the theme is the one the visitor picked on
// the site, under the same key; the bar's own script switches it.

import { CHROME, FONT_GATE, THEME_INIT } from "../../../../site/src/lib/theme-init.ts";
import { BAR as SITE_BAR, BAR_SCRIPT, FOOTER as SITE_FOOTER, STYLES } from "./site.gen.ts";

const FONT = "https://abitaro-sans.pages.dev/abitaro-sans.css?v=0.300";

/**
 * The <head>'s theme, icon, font and stylesheets, in the order the site's
 * pages have them. The bot's pages take the playground's icon (site/public/play.svg).
 */
export const HEAD = `<meta name="theme-color" content="${CHROME.light}" data-follow>
<script>${THEME_INIT}</script>
<link rel="icon" href="/play.svg" type="image/svg+xml">
<link rel="preconnect" href="https://abitaro-sans.pages.dev" crossorigin>
<link rel="stylesheet" href="${FONT}">
<script>${FONT_GATE}</script>
${STYLES.map((href) => `<link rel="stylesheet" href="${href}">`).join("\n")}`;

/** The site bar, with its theme switch, as the first thing in <body>. */
export const BAR = SITE_BAR;

/** The site's footer, then the bar's script, as the last things in <body>. */
export const FOOTER = `${SITE_FOOTER}\n${BAR_SCRIPT}`;

/**
 * A bot page's column and title, as the docs draw theirs: the site's width,
 * a tracked kicker, a heavy title over the 4px rule, and the site's section
 * headings and figures. The site's stylesheets give the rest.
 */
export const BASE = `body { font-variant-numeric: tabular-nums; }
main.wrap { max-width: 1280px; margin: 0 auto; padding: 32px var(--pad) 0; }
.red { color: var(--red); }
.kicker { color: var(--ink); margin-bottom: 14px; }
header.page { display: grid; grid-template-columns: 1fr auto; align-items: end; column-gap: 24px; border-bottom: var(--rule-thick) solid var(--rule); padding-bottom: 16px; }
h1 { font-weight: var(--w-heavy); font-size: var(--fs-title); line-height: 0.95; letter-spacing: var(--track-display); }
.meta { text-align: right; font-size: var(--fs-small); line-height: 1.6; }
.meta b { font-weight: var(--w-strong); }
.colophon { margin-top: 56px; color: var(--quiet); font-size: var(--fs-small); display: flex; flex-wrap: wrap; justify-content: space-between; gap: 4px 24px; }
footer.site { margin-top: 12px; }
@media (max-width: 640px) {
  main.wrap { padding-top: 24px; }
  header.page { grid-template-columns: 1fr; }
  .meta { text-align: left; margin-top: 16px; }
}`;
