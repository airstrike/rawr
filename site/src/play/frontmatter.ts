// A snippet's cargo-script frontmatter block (RFC 3503), as /play reads
// it: the dependencies its manifest names, and the snippet with the block
// blanked, so a run in the browser sees the snippet's own lines and
// columns. The browser builds nothing: a snippet runs there when each
// dependency it names is a crate the browser's prebuilt runner has, at a
// version its manifest allows (universe.toml).

/** One dependency a manifest names. */
export interface Dependency {
  /** The package: its `package = ".."`, or the name it is declared by. */
  package: string;
  /** Its version requirement, when it has one. */
  version?: string;
  /** Its git repository and rev, when it comes from one. */
  git?: string;
  rev?: string;
  /** It names another source: a path, a registry, a branch or a tag. */
  other: boolean;
  /** The manifest table it is declared in: `dependencies`, or a target's. */
  table: string;
}

/** A frontmatter block that reads. */
export interface Frontmatter {
  /** The snippet with the block, and what comes before it, blanked. */
  blanked: string;
  /** What its manifest names. */
  dependencies: Dependency[];
}

/** Keys that name another source than crates.io or a git rev. */
const OTHER = ["path", "registry", "registry-index", "branch", "tag", "workspace", "base"];

/** The frontmatter block `code` starts with, or null when it has none that reads. */
export function frontmatter(code: string): Frontmatter | null {
  let at = code.startsWith("﻿") ? 1 : 0;
  // A shebang line, which `#![` is not.
  if (code.startsWith("#!", at) && !code.slice(at + 2).trimStart().startsWith("[")) {
    const end = code.indexOf("\n", at);
    if (end < 0) return null;
    at = end + 1;
  }
  // Blank lines before the opening fence.
  for (let end = code.indexOf("\n", at); end >= 0 && code.slice(at, end).trim() === ""; end = code.indexOf("\n", at)) at = end + 1;
  const fence = /^-{3,}/.exec(code.slice(at))?.[0];
  if (!fence) return null;
  const opened = code.indexOf("\n", at);
  if (opened < 0) return null;
  const info = code.slice(at + fence.length, opened).trim();
  if (info !== "" && info !== "cargo") return null;
  const closing = new RegExp(`\\n${fence}[ \\t]*(\\r?\\n|$)`).exec(code.slice(opened));
  if (!closing) return null;
  const manifest = code.slice(opened + 1, opened + closing.index + 1);
  const end = opened + closing.index + closing[0].length;
  const blanked = code.slice(0, end).replace(/[^\n]/g, " ") + code.slice(end);
  return { blanked, dependencies: dependencies(manifest) };
}

/**
 * The dependencies `manifest` declares in its dependency tables, a
 * target's among them: `name = "1"`, `name = { .. }` on one line, and
 * `[dependencies.name]` tables.
 */
export function dependencies(manifest: string): Dependency[] {
  const found: Dependency[] = [];
  // The table a line is in, and the dependency a `[dependencies.name]` table declares.
  let table = "";
  let current: Dependency | null = null;
  for (const raw of manifest.split("\n")) {
    const line = raw.replace(/\s+#.*$/, "").trim();
    const header = /^\[([^\[\]]+)\]$/.exec(line);
    if (header) {
      const name = header[1].replace(/\s+/g, "");
      const named = /^(.*(?:^|\.)(?:dev-|build-)?dependencies)\.([A-Za-z0-9_-]+|"[^"]+")$/.exec(name);
      table = named ? named[1] : name;
      current = named ? { package: named[2].replaceAll('"', ""), other: false, table } : null;
      if (current) found.push(current);
      continue;
    }
    const entry = /^([A-Za-z0-9_-]+|"[^"]+")\s*=\s*(.+)$/.exec(line);
    if (!entry) continue;
    const key = entry[1].replaceAll('"', "");
    if (current) {
      field(current, key, entry[2]);
    } else if (/(^|\.)(dev-|build-)?dependencies$/.test(table)) {
      const dependency: Dependency = { package: key, other: false, table };
      const value = entry[2].trim();
      if (value.startsWith("{")) {
        for (const pair of value.slice(1, value.lastIndexOf("}")).matchAll(/([A-Za-z0-9_-]+)\s*=\s*("[^"]*"|\[[^\]]*\]|[^,]+)/g)) {
          field(dependency, pair[1], pair[2]);
        }
      } else field(dependency, "version", value);
      found.push(dependency);
    }
  }
  return found;
}

/** Record a dependency's key `key`, whose TOML value is `value`. */
function field(dependency: Dependency, key: string, value: string) {
  const text = /^"([^"]*)"$/.exec(value.trim())?.[1];
  if (key === "package" && text) dependency.package = text;
  else if (key === "version") dependency.version = text;
  else if (key === "git") dependency.git = text;
  else if (key === "rev") dependency.rev = text;
  else if (OTHER.includes(key)) dependency.other = true;
}

/** The crates the browser's runner has: its manifest's dependencies a browser builds. */
export function offered(manifest: string): Dependency[] {
  return dependencies(manifest).filter(
    (dependency) => !/^target\./.test(dependency.table) || (/wasm32/.test(dependency.table) && !/not\(target_arch/.test(dependency.table)),
  );
}

/** Whether `wanted`, a snippet's dependency, is one of `offered` at a version it allows. */
export function satisfied(wanted: Dependency, offered: Dependency[]): boolean {
  const has = offered.find((dependency) => dependency.package === wanted.package);
  if (!has || wanted.other) return false;
  if (has.git || wanted.git) return wanted.git === has.git && wanted.rev === has.rev && (wanted.version === undefined || has.version === wanted.version);
  return wanted.version === undefined || wanted.version.trim() === "*" || (has.version !== undefined && series(wanted.version) === series(has.version));
}

/**
 * The versions a requirement allows a crate to be compatible with, as a
 * caret requirement reads them: its first non-zero component and those
 * before it, so `1.40` is `1` and `0.30.2` is `0.30`. Null for a
 * requirement it can't read.
 */
function series(requirement: string): string | null {
  const parts = /^\^?=?\s*(\d+)(?:\.(\d+))?(?:\.(\d+))?/.exec(requirement.trim());
  if (!parts) return null;
  const [major, minor, patch] = [parts[1], parts[2], parts[3]];
  if (major !== "0") return major;
  if (minor === undefined || minor !== "0") return minor === undefined ? "0" : `0.${minor}`;
  return patch === undefined ? "0.0" : `0.0.${patch}`;
}
