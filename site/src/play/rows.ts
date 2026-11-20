// The rows of a value tree (panes.tsx's ValueTable): a binding, or a part
// of one, a row each, and which of them a click edits in place.
import { partPlace, type Part } from "./debugger.ts";

/** One row of a tree: a binding, or a part of one, at its depth. */
export interface Row {
  /** Its path from the tree's root, which keeps it open across pauses. */
  path: string;
  depth: number;
  name: string;
  value?: string;
  error?: string;
  children?: Part[];
  /** Rust naming its value in the frame, when it has one to assign to. */
  place?: string;
}

/**
 * `rows` and, under each open one, its parts' rows, depth first. A part of
 * a value at a place is at the place Rust reaches it by.
 */
export function visible(rows: Row[], open: Set<string>): Row[] {
  const shown: Row[] = [];
  const walk = (row: Row) => {
    shown.push(row);
    if (!row.children?.length || !open.has(row.path)) return;
    for (const part of row.children) {
      const place = row.place !== undefined && row.value !== undefined ? partPlace(row.place, row.value, part.name) : undefined;
      walk({ path: `${row.path}/${part.name}`, depth: row.depth + 1, name: part.name, value: part.value, children: part.children, place });
    }
  };
  rows.forEach(walk);
  return shown;
}

/** Whether a row's value can be edited in place: a value without parts, at a place. */
export function editable(row: Row): boolean {
  return row.place !== undefined && row.value !== undefined && row.error === undefined && !row.children?.length;
}
