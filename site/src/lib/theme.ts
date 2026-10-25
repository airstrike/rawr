// One dark or light theme for the whole site, on <html data-theme>. The
// inline script in Base.astro (THEME_INIT) sets it before the first paint:
// a share's pinned theme, else the visitor's choice from localStorage, else
// the system's. The header's switch and the playground's DARK and LIGHT
// control both call setTheme, which tells every listener ("rawr-theme").

import { CHROME, KEY } from "./theme-init.ts";

export type Theme = "dark" | "light";
export { KEY };
const EVENT = "rawr-theme";

export function theme(): Theme {
  return document.documentElement.dataset.theme === "light" ? "light" : "dark";
}

function stored(): Theme | null {
  try {
    const value = localStorage.getItem(KEY);
    return value === "dark" || value === "light" ? value : null;
  } catch {
    return null;
  }
}

/** Sets the theme, remembers it unless `remember` is false, and tells the listeners. */
export function setTheme(next: Theme, remember = true): void {
  const root = document.documentElement;
  if (remember) {
    try {
      localStorage.setItem(KEY, next);
    } catch {}
    delete root.dataset.themePin;
  }
  if (root.dataset.theme === next) return;
  root.dataset.theme = next;
  const chrome = document.querySelector<HTMLMetaElement>('meta[name="theme-color"][data-follow]');
  if (chrome) chrome.content = CHROME[next];
  dispatchEvent(new CustomEvent<Theme>(EVENT, { detail: next }));
}

/** Calls `listener` on every change of theme; returns the function that stops it. */
export function onTheme(listener: (theme: Theme) => void): () => void {
  const handle = (event: Event) => listener((event as CustomEvent<Theme>).detail);
  addEventListener(EVENT, handle);
  return () => removeEventListener(EVENT, handle);
}

/** Until the visitor chooses, the page follows the system's theme as it changes. */
export function followSystem(): void {
  const query = matchMedia("(prefers-color-scheme: light)");
  query.addEventListener("change", () => {
    if (!stored() && !document.documentElement.dataset.themePin) setTheme(query.matches ? "light" : "dark", false);
  });
}

/** The header's switch: it names the theme it switches to. */
export function setupSwitch(button: HTMLButtonElement): void {
  const label = () => {
    const say = theme() === "dark" ? "Switch to the light theme" : "Switch to the dark theme";
    button.setAttribute("aria-label", say);
    button.title = say;
  };
  label();
  button.addEventListener("click", () => setTheme(theme() === "dark" ? "light" : "dark"));
  onTheme(label);
}
