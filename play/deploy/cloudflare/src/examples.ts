// The playground's examples, embedded when the Worker is built
// (wrangler.jsonc's "rules" import *.rs files as text). They are plain Rust
// programs, from play/tests/fixtures/rust, and the first is what a first
// visit opens. The Rustlings group is rustlings' solutions
// (https://github.com/rust-lang/rustlings, MIT licence), each crediting its
// source; play/tests/fixtures/rust/LICENSE-rustlings holds the licence.

import type { Example } from "./playpage.ts";
import enums from "../../../tests/fixtures/rust/enums.rs";
import histogram from "../../../tests/fixtures/rust/histogram.rs";
import iterators from "../../../tests/fixtures/rust/iterators.rs";
import moves from "../../../tests/fixtures/rust/moves.rs";
import strings from "../../../tests/fixtures/rust/strings.rs";
import traits from "../../../tests/fixtures/rust/traits.rs";
import useAfterMove from "../../../tests/fixtures/rust/use_after_move.rs";
import wordCount from "../../../tests/fixtures/rust/word_count.rs";
import rustlingsBoxDynError from "../../../tests/fixtures/rust/rustlings_box_dyn_error.rs";
import rustlingsCapitalize from "../../../tests/fixtures/rust/rustlings_capitalize.rs";
import rustlingsConsList from "../../../tests/fixtures/rust/rustlings_cons_list.rs";
import rustlingsEnums from "../../../tests/fixtures/rust/rustlings_enums.rs";
import rustlingsLifetimes from "../../../tests/fixtures/rust/rustlings_lifetimes.rs";
import rustlingsOptions from "../../../tests/fixtures/rust/rustlings_options.rs";
import rustlingsOwnership from "../../../tests/fixtures/rust/rustlings_ownership.rs";
import rustlingsQuestionMark from "../../../tests/fixtures/rust/rustlings_question_mark.rs";
import rustlingsRc from "../../../tests/fixtures/rust/rustlings_rc.rs";
import rustlingsScores from "../../../tests/fixtures/rust/rustlings_scores.rs";
import rustlingsStrings from "../../../tests/fixtures/rust/rustlings_strings.rs";
import rustlingsTraits from "../../../tests/fixtures/rust/rustlings_traits.rs";
import rustlingsTryFrom from "../../../tests/fixtures/rust/rustlings_try_from.rs";

/** Plain programs: each prints its output as text. The first opens on a first visit. */
export const EXAMPLES: Example[] = [
  ...[
    { name: "Letter histogram", code: histogram },
    { name: "Moves and borrowing", code: moves },
    { name: "Use after move (error)", code: useAfterMove },
    { name: "Enums, match and ?", code: enums },
    { name: "Traits and generics", code: traits },
    { name: "Closures and iterators", code: iterators },
    { name: "Word count", code: wordCount },
    { name: "Strings and let else", code: strings },
  ].map((example) => ({ ...example, group: "Rust" })),
  ...[
    { name: "Taking ownership", code: rustlingsOwnership },
    { name: "Enum variants with data", code: rustlingsEnums },
    { name: "Matching an Option", code: rustlingsOptions },
    { name: "&str and String", code: rustlingsStrings },
    { name: "Score table (HashMap)", code: rustlingsScores },
    { name: "Result from main", code: rustlingsQuestionMark },
    { name: "Box<dyn Error>", code: rustlingsBoxDynError },
    { name: "A trait for String", code: rustlingsTraits },
    { name: "Longest string (lifetimes)", code: rustlingsLifetimes },
    { name: "Capitalize words", code: rustlingsCapitalize },
    { name: "Cons list in a Box", code: rustlingsConsList },
    { name: "Rc reference counts", code: rustlingsRc },
    { name: "TryFrom a color", code: rustlingsTryFrom },
  ].map((example) => ({ ...example, group: "Rustlings" })),
];

/** What a first visit opens. */
export const FIRST: Example = EXAMPLES[0];
