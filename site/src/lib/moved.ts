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
    "application": "/docs/iced#when",
    "async": "/docs/run#async",
    "async-rawr": "/docs/run#async",
    "available": "/docs/reference/iced#bindings",
    "available-widgets": "/docs/reference/iced#bindings",
    "blocking": "/docs/run#async",
    "canvas": "/docs/reference/iced#bindings",
    "components": "/docs/reference/iced#bindings",
    "custom-widgets": "/docs/reference/iced#gaps",
    "diagnostics": "/docs/reference/iced#diagnostics",
    "examples": "/docs/reference/iced#examples",
    "hot-reload": "/docs/iced/reload",
    "iced-run": "/docs/iced#when",
    "main-statements": "/docs/iced/reload#hot-reload",
    "not-available": "/docs/reference/iced#gaps",
    "previews": "/docs/iced/previews#previews",
    "rust-crates": "/docs/reference/iced#rust-crates",
    "shapes": "/docs/iced#when",
    "subscriptions": "/docs/reference/iced#bindings",
    "timed": "/docs/iced#when",
    "views": "/docs/reference/iced#bindings",
    "without": "/docs/iced#when"
  },
  "/docs/iced/previews": {
    "commands": "/docs/iced/previews#status",
    "files": "/docs/iced/previews#status"
  },
  "/docs/iced/reload": {
    "main-statements": "/docs/iced/reload#hot-reload",
    "native": "/docs/iced/reload#hot-reload",
    "state": "/docs/iced/reload#hot-reload",
    "tasks": "/docs/run#async"
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
    "rawr-iced": "/docs/iced",
    "rawr-import": "/docs/imports"
  },
  "/docs/reference/iced": {
    "async": "/docs/run#async",
    "async-rawr": "/docs/run#async",
    "available": "/docs/reference/iced#bindings",
    "available-widgets": "/docs/reference/iced#bindings",
    "blocking": "/docs/run#async",
    "borrowed": "/docs/reference/iced#bindings",
    "canvas": "/docs/reference/iced#bindings",
    "components": "/docs/reference/iced#bindings",
    "custom-widgets": "/docs/reference/iced#gaps",
    "not-available": "/docs/reference/iced#gaps",
    "pane-grid": "/docs/reference/iced#bindings",
    "subscriptions": "/docs/reference/iced#bindings",
    "views": "/docs/reference/iced#bindings"
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
