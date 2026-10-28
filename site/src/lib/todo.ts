// The work plan at /todo, a page nothing links to. Bar length is relative
// complexity, how much code changes, and never time. Edit the rows here.

export type State = "done" | "now" | "next";

export interface Row {
  lane: string;
  name: string;
  start: number;
  size: number;
  state: State;
  note: string;
}

export const UPDATED = "2026-10-28";
export const VERSION = "0.3.9";
export const EXAMPLES = { running: 6, total: 56, note: "Most others now stop further along" };

export const LANES: Record<string, string> = {
  done: "Shipped",
  bind: "Shared bindings",
  gnar: "The gnarly snippet",
  now: "No-glue, running",
  iced: "No-glue, next",
  play: "Playground",
  end: "rawr-iced",
};

const row = (lane: string, name: string, start: number, size: number, state: State, note = ""): Row => ({
  lane,
  name,
  start,
  size,
  state,
  note,
});

export const ROWS: Row[] = [
  row("done", "Runner on your own cargo", 0, 3, "done", "0.3.0–0.3.1. No added files, check build"),
  row("done", "Scripts with dependencies", 3, 3, "done", "0.3.2–0.3.4"),
  row("done", "Runtime once per machine", 6, 4, "done", "0.3.5–0.3.6. Second project in 13 s"),
  row("done", "Deploy gate", 10, 1, "done", "Waits for Cloudflare's rollout"),
  row("done", "First five examples", 0, 4, "done", "counter, checkbox, tooltip, pick_list, svg"),
  row("done", "Aliases and names", 4, 3, "done", "0.3.7. slider runs again"),
  row("done", "Script impls of crate traits", 7, 4, "done", "0.3.8. impl canvas::Program"),
  row("done", "Host type parameters", 11, 2, "done", "0.3.9. 11 examples past it"),
  row("bind", "Measure entry shapes", 11, 2, "now", "Per module, per item or lazy"),
  row("bind", "Macro $crate roots", 11, 2, "now", "column! without demand"),
  row("bind", "Crate-keyed entry", 13, 4, "next", "Extract once per crate"),
  row("bind", "Re-key plan and headers", 15, 3, "next", "Classify per crate, compose per program"),
  row("bind", "Key and publish", 17, 3, "next", "Atomic, with leases. T271"),
  row("bind", "Removal by last use", 19, 2, "next", "Kept across reinstalls. T272"),
  row("bind", "Key holes", 19, 2, "next", "T252, T254, T276"),
  row("bind", "Hit with no nightly", 21, 3, "next", "Key from lock and build outputs"),
  row("bind", "Drift, loudly", 23, 2, "next", "Names the crate. T275"),
  row("gnar", "Raw identifiers", 25, 1, "next", "r#for"),
  row("gnar", "Equality in bounds", 25, 2, "next", "Iterator<Item = Self::For>"),
  row("gnar", "Qualified Self paths", 26, 1, "next", "<Self as T>::A"),
  row("gnar", "Higher-ranked bounds", 27, 2, "next", "for<'a> Fn(&'a T)"),
  row("gnar", "Impl for a reference", 28, 3, "next", "impl<T> For for &T. T320"),
  row("now", "Subscription", 11, 3, "now", "6 examples. T309"),
  row("now", "Widget extension trait", 11, 3, "now", "5 examples. T311"),
  row("iced", "Time types", 14, 2, "next", "4 examples. T312"),
  row("iced", "Generic script impls", 14, 4, "next", "custom_quad, component. T322–T324"),
  row("iced", "Struct literals, live edits", 18, 2, "next", "T325, T326"),
  row("iced", "Projections, function values", 18, 3, "next", "T317–T319, T327"),
  row("iced", "Per-example gaps", 21, 6, "next", "About 18 examples. T314, T300, T283, T328"),
  row("play", "Bot, prebuilt bindings", 27, 5, "next", "Built once per image"),
  row("play", "Browser, wasm bindings", 32, 5, "next", "rawr-web"),
  row("end", "Flip the default route", 37, 2, "next"),
  row("end", "Delete rawr-iced", 39, 1, "next", "A no-op by then"),
];
