// The theme before the first paint: Base.astro inlines THEME_INIT in <head>.
// Kept apart from theme.ts, so the pages' bundles don't carry this text.

export const KEY = "rawr-theme";

/** The page's background per theme, for the browser's own chrome. */
export const CHROME: Record<"dark" | "light", string> = { dark: "#141416", light: "#f7f7f8" };

/** Runs in <head> before the body is parsed, so the first paint has the theme. */
export const THEME_INIT = `(() => {
  const root = document.documentElement;
  let theme = root.dataset.themePin;
  if (theme !== "dark" && theme !== "light") { try { theme = localStorage.getItem(${JSON.stringify(KEY)}); } catch {} }
  if (theme !== "dark" && theme !== "light") theme = matchMedia("(prefers-color-scheme: light)").matches ? "light" : "dark";
  root.dataset.theme = theme;
  const chrome = document.querySelector('meta[name="theme-color"][data-follow]');
  if (chrome) chrome.content = theme === "light" ? ${JSON.stringify(CHROME.light)} : ${JSON.stringify(CHROME.dark)};
})();`;

