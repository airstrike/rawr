// The work plan at /todo, a page nothing links to. Bar length is relative
// complexity, how much code changes, and never time. All bars share one axis,
// from the first work to the last planned, and the page rescales it to the
// groups that are open.
//
// todo.json is written by plan/plan.mjs from plan/gantt.json and
// plan/items.json. Change the chart with a change file (the gantt skill),
// never by editing the JSON here.
import data from "./todo.json";

export type State = "done" | "now" | "next";

export interface Issue {
  number: number;
  t: string | null;
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
  refs: Record<string, number>;
}

export interface Group {
  id: string;
  title: string;
  subtitle: string;
  open: boolean;
  rows: Row[];
}

export const REPO: string = data.repo;
export const UPDATED: string = data.updated;
export const VERSION: string = data.version;
export const EXAMPLES: { running: number; total: number; note: string } = data.examples;
export const GROUPS = data.groups as Group[];

export const issueUrl = (n: number) => `https://github.com/${REPO}/issues/${n}`;

// A row's note with each T number it names split out, so the page links it.
export function noteParts(row: Row): { text: string; issue?: number }[] {
  return row.note
    .split(/\b(T\d{3,})\b/)
    .filter(Boolean)
    .map((text) => (row.refs[text] ? { text, issue: row.refs[text] } : { text }));
}
