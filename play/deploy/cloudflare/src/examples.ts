// The playground's examples, embedded when the Worker is built
// (wrangler.jsonc's "rules" import *.rs files as text). They are plain Rust
// programs, from play/tests/fixtures/rust, and the first is what a first
// visit opens. iced and async programs don't run in the playground until
// generated bindings reach it, so none is listed.

import type { Example } from "./playpage.ts";
import enums from "../../../tests/fixtures/rust/enums.rs";
import histogram from "../../../tests/fixtures/rust/histogram.rs";
import iterators from "../../../tests/fixtures/rust/iterators.rs";
import moves from "../../../tests/fixtures/rust/moves.rs";
import strings from "../../../tests/fixtures/rust/strings.rs";
import traits from "../../../tests/fixtures/rust/traits.rs";
import useAfterMove from "../../../tests/fixtures/rust/use_after_move.rs";
import wordCount from "../../../tests/fixtures/rust/word_count.rs";

/** Plain programs: each prints its output as text. The first opens on a first visit. */
export const EXAMPLES: Example[] = [
  { name: "Letter histogram", code: histogram },
  { name: "Moves and borrowing", code: moves },
  { name: "Use after move (error)", code: useAfterMove },
  { name: "Enums, match and ?", code: enums },
  { name: "Traits and generics", code: traits },
  { name: "Closures and iterators", code: iterators },
  { name: "Word count", code: wordCount },
  { name: "Strings and let else", code: strings },
].map((example) => ({ ...example, group: "Rust" }));

/** What a first visit opens. */
export const FIRST: Example = EXAMPLES[0];
