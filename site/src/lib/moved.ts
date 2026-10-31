// Sections that moved to another page. A link to an old anchor that isn't
// on this page goes to the section's new home. The Worker redirects moved
// pages (DOCS_MOVED in its index.ts) and the browser keeps the anchor.
export const MOVED: Record<string, Record<string, string>> = {
  "/docs": {
    "status": "/docs/reference/requirements#status"
  },
  "/docs/embedding": {
    "call-fn": "/docs/embedding#example",
    "lifecycle": "/docs/embedding#reload"
  },
  "/docs/how-it-works": {
    "ship": "/docs/run#release",
    "snippets": "/docs/reference/play#sandbox"
  },
  "/docs/iced": {
    "async": "/docs/reference/iced#async",
    "async-rawr": "/docs/reference/iced#async-rawr",
    "available": "/docs/reference/iced#available",
    "available-widgets": "/docs/reference/iced#available-widgets",
    "blocking": "/docs/reference/iced#blocking",
    "canvas": "/docs/reference/iced#canvas",
    "components": "/docs/reference/iced#components",
    "custom-widgets": "/docs/reference/iced#custom-widgets",
    "diagnostics": "/docs/reference/iced#diagnostics",
    "examples": "/docs/reference/iced#examples",
    "hot-reload": "/docs/iced/reload",
    "main-statements": "/docs/iced/reload#main-statements",
    "not-available": "/docs/reference/iced#not-available",
    "previews": "/docs/iced/previews#previews",
    "rust-crates": "/docs/reference/iced#rust-crates",
    "subscriptions": "/docs/reference/iced#subscriptions",
    "views": "/docs/reference/iced#views"
  },
  "/docs/play": {
    "add-bot": "/docs/play/discord#add-bot",
    "allowance": "/docs/reference/play#iced",
    "animation-time": "/docs/reference/play#iced",
    "clock": "/docs/reference/play#clock",
    "crates": "/docs/reference/play#crates",
    "differences": "/docs/reference/play#differences",
    "discord": "/docs/play/discord#discord",
    "error-replies": "/docs/reference/play#error-replies",
    "frames": "/docs/reference/play#iced",
    "image-notes": "/docs/reference/play#iced",
    "images": "/docs/reference/play#iced",
    "limits": "/docs/reference/play#limits",
    "loops": "/docs/reference/play#iced",
    "messages": "/docs/reference/play#iced",
    "notes": "/docs/reference/play#notes",
    "options": "/docs/play/discord#discord",
    "output": "/docs/reference/play#output",
    "output-notes": "/docs/reference/play#output-notes",
    "prelude": "/docs/reference/play#iced",
    "sandbox": "/docs/reference/play#sandbox",
    "sippers": "/docs/reference/play#tokio",
    "size": "/docs/reference/play#iced",
    "spawn": "/docs/reference/play#tokio",
    "spawn-blocking": "/docs/reference/play#tokio",
    "tokio": "/docs/reference/play#tokio",
    "tokio-differences": "/docs/reference/play#tokio",
    "wrapping": "/docs/reference/play#iced"
  },
  "/docs/reference/commands": {
    "rawr-iced": "/docs/iced/previews#commands",
    "rawr-import": "/docs/imports"
  },
  "/docs/reference/overview": {
    "items": "/docs/reference/items",
    "types": "/docs/reference/types",
    "generics-traits": "/docs/reference/generics",
    "patterns": "/docs/reference/patterns",
    "moves": "/docs/reference/moves",
    "closures": "/docs/reference/closures",
    "async": "/docs/reference/closures#async",
    "macros": "/docs/reference/macros",
    "format": "/docs/reference/macros#format",
    "std": "/docs/reference/std"
  },
  "/docs/reference/play": {
    "allowance": "/docs/reference/play#iced",
    "animation-time": "/docs/reference/play#iced",
    "frames": "/docs/reference/play#iced",
    "image-notes": "/docs/reference/play#iced",
    "images": "/docs/reference/play#iced",
    "loops": "/docs/reference/play#iced",
    "messages": "/docs/reference/play#iced",
    "prelude": "/docs/reference/play#iced",
    "sippers": "/docs/reference/play#tokio",
    "size": "/docs/reference/play#iced",
    "spawn": "/docs/reference/play#tokio",
    "spawn-blocking": "/docs/reference/play#tokio",
    "tokio-differences": "/docs/reference/play#tokio",
    "wrapping": "/docs/reference/play#iced"
  },
  "/docs/start": {
    "first-app": "/docs/iced#new",
    "hot-reload": "/docs/iced/reload",
    "install-crate": "/docs/embedding#add",
    "install-iced": "/docs/iced#install",
    "install-import": "/docs/imports#install"
  }
};

/**
 * A docs page's inline script, when an anchor moved away from it: once the
 * page is parsed, an anchor that isn't on it goes to its new home.
 */
export function movedScript(path: string): string | null {
  const moved = MOVED[path];
  if (!moved) return null;
  return `(() => {
  const MOVED = ${JSON.stringify(moved)};
  let id = "";
  try { id = decodeURIComponent(location.hash.slice(1)); } catch {}
  const to = MOVED[id];
  if (to) addEventListener("DOMContentLoaded", () => { if (!document.getElementById(id)) location.replace(to); });
})();`;
}
