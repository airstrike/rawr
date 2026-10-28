// The work plan at /todo, a page nothing links to. Bar length is relative
// complexity, how much code changes, and never time. All bars share one axis,
// from the first work to the last planned, and the page rescales it to the
// groups that are open. Edit the groups here.

export type State = "done" | "now" | "next";

export interface Row {
  name: string;
  start: number;
  size: number;
  state: State;
  note: string;
}

export interface Group {
  id: string;
  title: string;
  subtitle: string;
  open: boolean;
  rows: Row[];
}

export const UPDATED = "2026-10-28";
export const VERSION = "0.3.11";
export const EXAMPLES = { running: 7, total: 56, note: "Most others now stop further along" };

const row = (name: string, start: number, size: number, state: State, note = ""): Row => ({
  name,
  start,
  size,
  state,
  note,
});

// Where 0.3's work starts on the shared axis, after the history groups.
const O = 26;
const at = (name: string, start: number, size: number, state: State, note = ""): Row =>
  row(name, start + O, size, state, note);

export const GROUPS: Group[] = [
  {
    id: "v000",
    title: "0.0.0",
    subtitle: "Before rawr's own version numbers",
    open: false,
    rows: [
      row("Typed Rust language", 0, 5, "done", "Structs, enums, traits, generics, match"),
      row("Hot reload, state kept", 2, 3, "done", "Edits apply at the next event"),
      row("iced facade, rawr-iced", 4, 4, "done", "Hosts, widgets, previews"),
      row("Rust crates through rustc", 6, 5, "done", "The importer generates bindings"),
      row("Projects and the marker", 9, 2, "done", "A crate marked // rawr"),
      row("?play and the playground", 10, 3, "done", "Discord bot, browser via wasm"),
      row("Embedding in an app", 12, 2, "done", "Script, typed handles"),
      row("Async", 13, 3, "done", "Futures, tokio, async fn"),
    ],
  },
  {
    id: "v010",
    title: "0.1.0",
    subtitle: "First release under rawr's own numbers",
    open: false,
    rows: [
      row("Own name, own versions", 16, 1, "done", "Every crate 0.1.0"),
      row("Other hosts", 16, 2, "done", "Freya, ratatui"),
      row("Language gaps against rustc", 17, 2, "done", "Casts, moves, deref coercion"),
    ],
  },
  {
    id: "v020",
    title: "0.2.0",
    subtitle: "rawr checks borrows",
    open: false,
    rows: [
      row("Borrow checker", 19, 5, "done", "rustc's NLL and error codes, 11,409 programs"),
      row("No glue: first examples", 22, 2, "done", "counter, checkbox, slider, tooltip"),
      row("Marker after frontmatter", 23, 1, "done", "0.2.8"),
      row("impl Trait outputs", 24, 2, "done", "0.2.9. Optional callables"),
    ],
  },
  {
    id: "shipped",
    title: "0.3",
    subtitle: "Shipped",
    open: true,
    rows: [
      at("Runner on your own cargo", 0, 3, "done", "0.3.0–0.3.1. No added files, check build"),
      at("Scripts with dependencies", 3, 3, "done", "0.3.2–0.3.4"),
      at("Runtime once per machine", 6, 4, "done", "0.3.5–0.3.6. Second project in 13 s"),
      at("Deploy gate", 10, 1, "done", "Waits for Cloudflare's rollout"),
      at("pick_list, svg", 0, 2, "done", "No glue"),
      at("Aliases and names", 4, 3, "done", "0.3.7. slider runs again"),
      at("Script impls of crate traits", 7, 4, "done", "0.3.8. impl canvas::Program"),
      at("Host type parameters", 11, 2, "done", "0.3.9. 11 examples past it"),
      at("Subscription", 11, 3, "done", "0.3.10. url_handler runs"),
      at("Widget extension trait", 11, 3, "done", "0.3.11. .boxed(), impl Widget"),
    ],
  },
  {
    id: "bind",
    title: "Shared bindings",
    subtitle: "Built once per machine",
    open: true,
    rows: [
      at("Record extraction per crate", 11, 2, "done", "Replay any demand from it"),
      at("Macro $crate roots", 11, 2, "now", "column! without demand"),
      at("Crate-keyed entry", 13, 4, "now", "Extract once per crate"),
      at("Re-key plan and headers", 15, 3, "next", "Classify per crate, compose per program"),
      at("Key and publish", 17, 3, "next", "Atomic, with leases. T271"),
      at("Removal by last use", 19, 2, "next", "Kept across reinstalls. T272"),
      at("Key holes", 19, 2, "next", "T252, T254, T276"),
      at("Hit with no nightly", 21, 3, "next", "Key from lock and build outputs"),
      at("Drift, loudly", 23, 2, "next", "Names the crate. T275"),
    ],
  },
  {
    id: "gnar",
    title: "The gnarly snippet",
    subtitle: "After shared bindings",
    open: true,
    rows: [
      at("Raw identifiers", 25, 1, "next", "r#for"),
      at("Equality in bounds", 25, 2, "next", "Iterator<Item = Self::For>"),
      at("Qualified Self paths", 26, 1, "next", "<Self as T>::A"),
      at("Higher-ranked bounds", 27, 2, "next", "for<'a> Fn(&'a T)"),
      at("Impl for a reference", 28, 3, "next", "impl<T> For for &T. T320"),
    ],
  },
  {
    id: "now",
    title: "No glue",
    subtitle: "Running now",
    open: true,
    rows: [
      at("Time types", 14, 2, "now", "7 examples. T312, T338"),
      at("Boot returning a task", 14, 3, "now", "text, system_information, multi_window. T300"),
    ],
  },
  {
    id: "iced",
    title: "No glue",
    subtitle: "Next",
    open: true,
    rows: [
      at("Generic script impls", 17, 4, "next", "custom_quad, component. T322–T324, T341"),
      at("Struct literals, live edits", 18, 2, "next", "T325, T326"),
      at("Projections, function values", 18, 3, "next", "T317–T319, T327"),
      at("Runner-build gaps", 19, 3, "next", "events, exit, theme, modal. T335–T340"),
      at("Per-example gaps", 21, 6, "next", "About 18 examples. T314, T283, T328, T334"),
    ],
  },
  {
    id: "play",
    title: "Playground",
    subtitle: "Without rawr-iced",
    open: true,
    rows: [
      at("Bot, prebuilt bindings", 27, 5, "next", "Built once per image"),
      at("Browser, wasm bindings", 32, 5, "next", "rawr-web"),
    ],
  },
  {
    id: "end",
    title: "rawr-iced",
    subtitle: "Retired",
    open: true,
    rows: [
      at("Flip the default route", 37, 2, "next"),
      at("Delete rawr-iced", 39, 1, "next", "A no-op by then"),
    ],
  },
];
