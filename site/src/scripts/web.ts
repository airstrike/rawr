// Where a page finds rawr-web's files (web/build.sh: rawr-web, the browser's
// universe and its debugger build). They live in R2 apart from the site, as
// sets: each set under its version, at /web/<version>/<file>, never changed
// once written. /web/current.json, the pointer, names the set the site runs
// now: its version, the commit it was built from and the protocol it speaks.
// play/deploy/cloudflare/upload-web.mjs writes a set's files first and the
// pointer last, and the Worker serves both (play/deploy/cloudflare/src/web.ts).
//
// A page reads the pointer before it loads any module, and loads every file
// from the set the pointer named, so a glue file and its wasm always come
// from one build. A set that speaks another protocol than the page's is
// never loaded: the page says so instead.

/**
 * The version of the interface between the page and the modules: rawr-web's
 * exports (`Web` in play/session.ts, from web/src/lib.rs), an instance's
 * (`Instance` in universe.ts, from src/universe/browser.rs and debugger.rs),
 * the JSON they answer with and the files a set holds. A change that a page
 * built before it can't use on the new modules, or a page built after it
 * can't use on the old ones, bumps it.
 */
export const WEB_PROTOCOL = 1;

/** The pointer's address on the site, which is also its key in the bucket without the leading slash. */
export const POINTER = "/web/current.json";

/** A set's version: the first 16 hex digits of the SHA-256 of its SHA256SUMS. */
export const VERSION = /^[0-9a-f]{16}$/;

/** The pointer's content. */
export interface Pointer {
  version: string;
  /** The commit the set was built from, or "" when it isn't known. */
  commit: string;
  protocol: number;
}

/** The pointer in `value` (its parsed JSON), or null when it isn't one. */
export function parsePointer(value: unknown): Pointer | null {
  if (typeof value !== "object" || value === null) return null;
  const { version, commit, protocol } = value as Record<string, unknown>;
  if (typeof version !== "string" || !VERSION.test(version)) return null;
  if (typeof commit !== "string" || !Number.isInteger(protocol)) return null;
  return { version, commit, protocol: protocol as number };
}

/** Why a page that speaks `expected` can't load the set `pointer` names, or null when it can. */
export function mismatch(pointer: Pointer, expected: number = WEB_PROTOCOL): string | null {
  if (pointer.protocol === expected) return null;
  if (pointer.protocol < expected) {
    return `this page needs rawr-web protocol ${expected}, and the site's modules are still protocol ${pointer.protocol}. They update with the next module build.`;
  }
  return `the site's modules are rawr-web protocol ${pointer.protocol}, newer than this page's protocol ${expected}. Reload the page.`;
}

/**
 * The page's files: the address of a file of the set the pointer names. The
 * pointer is read once per page; a read that failed, or a set this page
 * can't load, is read again on the next call, so a run after the modules
 * update loads them.
 */
export function webFiles(fetch: typeof globalThis.fetch, expected: number = WEB_PROTOCOL): (name: string) => Promise<string> {
  let base: Promise<string> | null = null;
  const read = async () => {
    const response = await fetch(POINTER);
    if (!response.ok) throw new Error(`rawr-web's pointer (${POINTER}) answered ${response.status}`);
    const pointer = parsePointer(await response.json().catch(() => null));
    if (!pointer) throw new Error(`rawr-web's pointer (${POINTER}) is not one`);
    const why = mismatch(pointer, expected);
    if (why) throw new Error(why);
    return `/web/${pointer.version}/`;
  };
  return async (name) => {
    base ??= read().catch((error) => {
      base = null;
      throw error;
    });
    return (await base) + name;
  };
}
