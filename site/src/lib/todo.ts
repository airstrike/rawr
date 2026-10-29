// The work plan at /todo, a page nothing links to. Its tabs are the Gantt
// chart, the changelog, the TODO items, and two private documents the Worker
// serves behind a password (play/deploy/cloudflare/src/todo.ts).
//
// On the chart, bar length is relative complexity, how much code changes, and
// never time. All bars share one axis, from the first work to the last
// planned, and the page rescales it to the groups that are open.
//
// todo.json is written by plan/plan.mjs from plan/gantt.json,
// plan/changelog.json and plan/items.json. Issues are named #N throughout.
// Change the records with a change file (the gantt, release and todo skills),
// never by editing the JSON here.
import data from "./todo.json";

export type State = "done" | "now" | "next";

export interface Issue {
  number: number;
  title: string;
  state: "open" | "closed";
}

export interface Row {
  id: string;
  name: string;
  start: number;
  size: number;
  state: State;
  note: string;
  issues: Issue[];
}

export interface Group {
  id: string;
  title: string;
  subtitle: string;
  open: boolean;
  rows: Row[];
}

export interface Entry {
  text: string;
  issues: number[];
  refs?: number[];
}

export interface Release {
  version: string;
  date: string;
  lede?: string;
  entries: Entry[];
}

export interface Changelog {
  intro: string[];
  unreleased: { notes: string[]; entries: Entry[] };
  releases: Release[];
}

export interface Item {
  issue: number;
  title: string;
  state: "open" | "closed";
  category: string;
  release?: string;
}

export const REPO: string = data.repo;
export const UPDATED: string = data.updated;
export const VERSION: string = data.version;
export const EXAMPLES: { running: number; total: number; note: string } = data.examples;
export const GROUPS = data.groups as Group[];
export const CHANGELOG = data.changelog as Changelog;
export const ITEMS = data.items as Item[];

export const issueUrl = (n: number) => `https://github.com/${REPO}/issues/${n}`;

/** A piece of a line of text: plain, `code`, or an issue named as #N. */
export type Part = { text: string; code?: true; issue?: number };

// Backticks mark code; outside code, each #N is an issue. `r#for` and
// `&#123;` are not issues.
export function parts(text: string): Part[] {
  const out: Part[] = [];
  text.split(/(`[^`]*`)/).forEach((piece, i) => {
    if (!piece) return;
    if (i % 2) return out.push({ text: piece.slice(1, -1), code: true });
    for (const bit of piece.split(/((?<![\w&])#\d+\b)/)) {
      if (!bit) continue;
      out.push(/^#\d+$/.test(bit) ? { text: bit, issue: +bit.slice(1) } : { text: bit });
    }
  });
  return out;
}

/** The categories in the order the items first name them, each with its items. */
export function categories(items: Item[]): { name: string; slug: string; items: Item[]; open: number }[] {
  const by = new Map<string, Item[]>();
  for (const item of items) by.set(item.category, [...(by.get(item.category) ?? []), item]);
  return [...by].map(([name, items]) => ({
    name,
    slug: `cat-${name.toLowerCase().replace(/[^a-z0-9]+/g, "-").replace(/^-|-$/g, "")}`,
    items,
    open: items.filter((i) => i.state === "open").length,
  }));
}
