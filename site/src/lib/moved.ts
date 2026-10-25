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
    "allowance": "/docs/reference/play#allowance",
    "animation-time": "/docs/reference/play#animation-time",
    "clock": "/docs/reference/play#clock",
    "crates": "/docs/reference/play#crates",
    "differences": "/docs/reference/play#differences",
    "discord": "/docs/play/discord#discord",
    "error-replies": "/docs/reference/play#error-replies",
    "frames": "/docs/reference/play#frames",
    "image-notes": "/docs/reference/play#image-notes",
    "images": "/docs/reference/play#images",
    "limits": "/docs/reference/play#limits",
    "loops": "/docs/reference/play#loops",
    "messages": "/docs/reference/play#messages",
    "notes": "/docs/reference/play#notes",
    "options": "/docs/play/discord#options",
    "output": "/docs/reference/play#output",
    "output-notes": "/docs/reference/play#output-notes",
    "prelude": "/docs/reference/play#prelude",
    "sandbox": "/docs/reference/play#sandbox",
    "sippers": "/docs/reference/play#sippers",
    "size": "/docs/reference/play#size",
    "spawn": "/docs/reference/play#spawn",
    "spawn-blocking": "/docs/reference/play#spawn-blocking",
    "tokio": "/docs/reference/play#tokio",
    "tokio-differences": "/docs/reference/play#tokio-differences",
    "wrapping": "/docs/reference/play#wrapping"
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
