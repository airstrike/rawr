// The playground's split bars and the debugger's value trees, drawn by
// Playground.tsx.
//
// A split bar sits on the line between two panes, as elevato's iced_split
// divider does between its cards: a few pixels wide to grab, nothing more
// than the line at rest, the accent while hovered or dragged. A drag moves
// the line, stopping short of each pane's least size; arrow keys move it
// too. Each size is the viewer's own, kept in localStorage. On a phone the
// panes stack and the bars aren't drawn.
//
// A value tree is a compact table, one row per value, as DevTools' scope
// pane draws one: a value with parts (debugger.ts's Part) opens to a row
// per field, item or map entry, and a long value elides to one line with
// its whole text as the row's tooltip.
import { useCallback, useRef, useState, type KeyboardEvent, type PointerEvent } from "react";
import type { Part } from "./debugger.ts";

/** `value` kept within `low` and `high`; `low` wins when they cross. */
export function clamp(value: number, low: number, high: number): number {
  return Math.max(low, Math.min(value, high));
}

/** The viewer's stored storage, or null where the browser refuses it. */
function storage(): Storage | null {
  try {
    return window.localStorage;
  } catch {
    return null;
  }
}

/**
 * A size the viewer sets, kept under `key`: its value, a setter for the
 * drag, and `keep` to store the value once the drag ends.
 */
export function useStoredSize(key: string, fallback: number): [number, (value: number) => void, () => void] {
  const [size, setSize] = useState(() => {
    try {
      const stored = Number(storage()?.getItem(key));
      return Number.isFinite(stored) && stored > 0 ? stored : fallback;
    } catch {
      return fallback;
    }
  });
  const latest = useRef(size);
  const set = useCallback((value: number) => {
    latest.current = value;
    setSize(value);
  }, []);
  const keep = useCallback(() => {
    try {
      storage()?.setItem(key, String(latest.current));
    } catch {}
  }, [key]);
  return [size, set, keep];
}

export interface SplitBarProps {
  /** Which line it moves: `columns` a vertical line, `rows` a horizontal one. */
  between: "columns" | "rows";
  label: string;
  /** Takes the pointer's x (columns) or y (rows) while it drags. */
  onMove(at: number): void;
  /** The drag or the key ended: store the size. */
  onEnd(): void;
  /** The line's position now, for the arrow keys: x or y on the page. */
  at(): number;
  className?: string;
}

/** How far an arrow key moves a bar; Shift moves it further. */
const STEP = 16;

/** A bar on the line between two panes, which a drag or the arrow keys move. */
export function SplitBar({ between, label, onMove, onEnd, at, className }: SplitBarProps) {
  const [dragging, setDragging] = useState(false);
  const coordinate = (event: PointerEvent) => (between === "columns" ? event.clientX : event.clientY);
  const down = (event: PointerEvent<HTMLDivElement>) => {
    if (event.button !== 0) return;
    event.preventDefault();
    event.currentTarget.setPointerCapture(event.pointerId);
    setDragging(true);
  };
  const move = (event: PointerEvent<HTMLDivElement>) => {
    if (dragging) onMove(Math.round(coordinate(event)));
  };
  const up = (event: PointerEvent<HTMLDivElement>) => {
    if (!dragging) return;
    event.currentTarget.releasePointerCapture(event.pointerId);
    setDragging(false);
    onEnd();
  };
  const key = (event: KeyboardEvent<HTMLDivElement>) => {
    const back = between === "columns" ? "ArrowLeft" : "ArrowUp";
    const on = between === "columns" ? "ArrowRight" : "ArrowDown";
    if (event.key !== back && event.key !== on) return;
    event.preventDefault();
    const step = (event.shiftKey ? 4 : 1) * STEP;
    onMove(at() + (event.key === on ? step : -step));
    onEnd();
  };
  return (
    <div
      role="separator"
      tabIndex={0}
      aria-label={label}
      aria-orientation={between === "columns" ? "vertical" : "horizontal"}
      data-split={between}
      data-dragging={dragging || undefined}
      className={"split " + (className ?? "")}
      onPointerDown={down}
      onPointerMove={move}
      onPointerUp={up}
      onPointerCancel={up}
      onKeyDown={key}
    />
  );
}

/** The colour class of a value's text: strings and numbers, as DevTools tints them. */
function tint(value: string): string {
  if (/^["']/.test(value)) return "value str";
  if (/^-?\d/.test(value)) return "value num";
  return "value";
}

/** One row of a tree: a binding, or a part of one, at its depth. */
export interface Row {
  /** Its path from the tree's root, which keeps it open across pauses. */
  path: string;
  depth: number;
  name: string;
  value?: string;
  error?: string;
  children?: Part[];
}

/** `rows` and, under each open one, its parts' rows, depth first. */
function visible(rows: Row[], open: Set<string>): Row[] {
  const shown: Row[] = [];
  const walk = (row: Row) => {
    shown.push(row);
    if (!row.children?.length || !open.has(row.path)) return;
    for (const part of row.children) {
      walk({ path: `${row.path}/${part.name}`, depth: row.depth + 1, name: part.name, value: part.value, children: part.children });
    }
  };
  rows.forEach(walk);
  return shown;
}

/** Which rows are open, by path: `toggle` opens or closes one. */
export function useOpen(): [Set<string>, (path: string) => void] {
  const [open, setOpen] = useState(() => new Set<string>());
  const toggle = useCallback((path: string) => {
    setOpen((now) => {
      const next = new Set(now);
      if (!next.delete(path)) next.add(path);
      return next;
    });
  }, []);
  return [open, toggle];
}

/** The disclosure triangle of a row with parts, or its room on one without. */
export function Twisty({ row, open, toggle }: { row: Row; open: Set<string>; toggle(path: string): void }) {
  if (!row.children?.length) return <span className="twisty" aria-hidden="true" />;
  const expanded = open.has(row.path);
  return (
    <button
      type="button"
      className="twisty"
      aria-expanded={expanded}
      aria-label={`${expanded ? "Close" : "Open"} ${row.name}`}
      onClick={(event) => {
        event.stopPropagation();
        toggle(row.path);
      }}
    />
  );
}

/**
 * The rows as a two-column table: the name, indented by its depth, and the
 * value on one line. A top-level row carries `data-binding` with its name,
 * a part `data-part` with its path below the root.
 */
export function ValueTable({ rows, open, toggle, className }: { rows: Row[]; open: Set<string>; toggle(path: string): void; className?: string }) {
  return (
    <table className={"tree " + (className ?? "")}>
      <tbody>
        {visible(rows, open).map((row) => (
          <tr
            key={row.path}
            data-binding={row.depth === 0 ? row.name : undefined}
            data-part={row.depth > 0 ? row.path.slice(row.path.indexOf("/") + 1) : undefined}
            data-depth={row.depth}
            onClick={row.children?.length ? () => toggle(row.path) : undefined}
            className={row.children?.length ? "opens" : undefined}
          >
            <td className="name" style={{ paddingLeft: `${4 + row.depth * 12}px` }}>
              <Twisty row={row} open={open} toggle={toggle} />
              {row.name}
            </td>
            {row.error !== undefined
              ? <td className="value err" title={row.error}>{row.error}</td>
              : <td className={tint(row.value ?? "")} title={row.value}>{row.value}</td>}
          </tr>
        ))}
      </tbody>
    </table>
  );
}

/** The parts of an answer, under its line in the console, while it is open. */
export function PartsOf({ path, parts, open, toggle }: { path: string; parts: Part[]; open: Set<string>; toggle(path: string): void }) {
  const rows: Row[] = parts.map((part) => ({ path: `${path}/${part.name}`, depth: 1, name: part.name, value: part.value, children: part.children }));
  return <ValueTable rows={rows} open={open} toggle={toggle} className="parts" />;
}

