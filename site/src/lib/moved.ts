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
    "bindings": "/docs/imports",
    "borrowed": "/docs/imports",
    "gaps": "/docs/imports#gaps",
    "pane-grid": "/docs/imports",
    "rust-crates": "/docs/imports",
    "application": "/docs/iced#when",
    "async": "/docs/run#async",
    "async-rawr": "/docs/run#async",
    "available": "/docs/imports",
    "available-widgets": "/docs/imports",
    "blocking": "/docs/run#async",
    "canvas": "/docs/imports",
    "components": "/docs/imports",
    "custom-widgets": "/docs/imports#gaps",
    "diagnostics": "/docs/reference/overview",
    "examples": "/docs/iced#works",
    "hot-reload": "/docs/iced/reload",
    "iced-run": "/docs/iced#when",
    "main-statements": "/docs/iced/reload#hot-reload",
    "not-available": "/docs/imports#gaps",
    "previews": "/docs/iced/previews#previews",
    "rust-crates": "/docs/imports",
    "shapes": "/docs/iced#when",
    "subscriptions": "/docs/imports",
    "timed": "/docs/iced#when",
    "views": "/docs/imports",
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
    "allowance": "/docs/reference/play#crates",
    "animation-time": "/docs/reference/play#crates",
    "clock": "/docs/reference/play#clock",
    "crates": "/docs/reference/play#crates",
    "differences": "/docs/reference/play#differences",
    "discord": "/docs/play/discord#discord",
    "error-replies": "/docs/reference/play#error-replies",
    "frames": "/docs/reference/play#crates",
    "image-notes": "/docs/reference/play#crates",
    "images": "/docs/reference/play#crates",
    "limits": "/docs/reference/play#limits",
    "loops": "/docs/reference/play#crates",
    "messages": "/docs/reference/play#crates",
    "notes": "/docs/reference/play#notes",
    "options": "/docs/play/discord#discord",
    "output": "/docs/reference/play#output",
    "output-notes": "/docs/reference/play#output-notes",
    "prelude": "/docs/reference/play#crates",
    "sandbox": "/docs/reference/play#sandbox",
    "sippers": "/docs/reference/play#async-main",
    "size": "/docs/reference/play#crates",
    "spawn": "/docs/reference/play#async-main",
    "spawn-blocking": "/docs/reference/play#async-main",
    "tokio": "/docs/reference/play#async-main",
    "tokio-differences": "/docs/reference/play#async-main",
    "wrapping": "/docs/reference/play#crates"
  },
  "/docs/reference/commands": {
    "rawr-iced": "/docs/iced",
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
    "allowance": "/docs/reference/play#crates",
    "animation-time": "/docs/reference/play#crates",
    "frames": "/docs/reference/play#crates",
    "image-notes": "/docs/reference/play#crates",
    "images": "/docs/reference/play#crates",
    "loops": "/docs/reference/play#crates",
    "messages": "/docs/reference/play#crates",
    "prelude": "/docs/reference/play#crates",
    "sippers": "/docs/reference/play#async-main",
    "size": "/docs/reference/play#crates",
    "spawn": "/docs/reference/play#async-main",
    "spawn-blocking": "/docs/reference/play#async-main",
    "tokio-differences": "/docs/reference/play#async-main",
    "wrapping": "/docs/reference/play#crates"
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
