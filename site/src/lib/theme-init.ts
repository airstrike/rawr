// The theme and the font before the first paint: Base.astro and /todo inline
// THEME_INIT and FONT_GATE in <head>. Kept apart from theme.ts, so the pages'
// bundles don't carry this text.

export const KEY = "rawr-theme";

/** The page's background per theme (tokens.css's --paper), for the browser's own chrome. */
export const CHROME: Record<"dark" | "light", string> = { dark: "#1a1a1a", light: "#ffffff" };

/**
 * Runs in <head> before the body is parsed, so the first paint has the theme:
 * the visitor's choice, else the system's. It needs the theme-color meta
 * before it.
 */
export const THEME_INIT = `(() => {
  const root = document.documentElement;
  let theme;
  try { theme = localStorage.getItem(${JSON.stringify(KEY)}); } catch {}
  if (theme !== "dark" && theme !== "light") theme = matchMedia("(prefers-color-scheme: dark)").matches ? "dark" : "light";
  root.dataset.theme = theme;
  const chrome = document.querySelector('meta[name="theme-color"][data-follow]');
  if (chrome) chrome.content = theme === "light" ? ${JSON.stringify(CHROME.light)} : ${JSON.stringify(CHROME.dark)};
})();`;

/**
 * The page shows once its font has loaded, so no text draws in a fallback
 * face first (html[data-font="wait"] hides the body). A font that hasn't come
 * in 3 s shows the page in the fallback. It goes after the font's stylesheet,
 * which it waits for.
 */
export const FONT_GATE = `(() => {
  const root = document.documentElement;
  root.dataset.font = "wait";
  const show = () => delete root.dataset.font;
  setTimeout(show, 3000);
  document.fonts?.load('450 14px "Abitaro Sans"').then(show, show) ?? show();
})();`;
