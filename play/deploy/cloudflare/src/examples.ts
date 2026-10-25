// The playground's examples, embedded when the Worker is built
// (wrangler.jsonc's "rules" import *.rs files as text). Plain Rust programs
// come first, from play/tests/fixtures/rust, and the first is what a first
// visit opens. iced's examples follow, from play/tests/fixtures, simple ones
// first. A size is set where the default canvas would cut the view off.

import type { Example } from "./playpage.ts";
import enums from "../../../tests/fixtures/rust/enums.rs";
import histogram from "../../../tests/fixtures/rust/histogram.rs";
import iterators from "../../../tests/fixtures/rust/iterators.rs";
import moves from "../../../tests/fixtures/rust/moves.rs";
import strings from "../../../tests/fixtures/rust/strings.rs";
import tasks from "../../../tests/fixtures/rust/tasks.rs";
import traits from "../../../tests/fixtures/rust/traits.rs";
import useAfterMove from "../../../tests/fixtures/rust/use_after_move.rs";
import wordCount from "../../../tests/fixtures/rust/word_count.rs";
import arc from "../../../tests/fixtures/arc.rs";
import checkbox from "../../../tests/fixtures/checkbox.rs";
import comboBox from "../../../tests/fixtures/combo_box.rs";
import component from "../../../tests/fixtures/component.rs";
import counter from "../../../tests/fixtures/counter.rs";
import customQuad from "../../../tests/fixtures/custom_quad.rs";
import customWidget from "../../../tests/fixtures/custom_widget.rs";
import lazy from "../../../tests/fixtures/lazy.rs";
import modal from "../../../tests/fixtures/modal.rs";
import progressBar from "../../../tests/fixtures/progress_bar.rs";
import qrCode from "../../../tests/fixtures/qr_code.rs";
import sandpiles from "../../../tests/fixtures/sandpiles.rs";
import scrollable from "../../../tests/fixtures/scrollable.rs";
import slider from "../../../tests/fixtures/slider.rs";
import stopwatch from "../../../tests/fixtures/stopwatch.rs";
import styling from "../../../tests/fixtures/styling.rs";
import table from "../../../tests/fixtures/table.rs";
import text from "../../../tests/fixtures/text.rs";
import timed from "../../../tests/fixtures/timed.rs";
import tooltip from "../../../tests/fixtures/tooltip.rs";
import vectorialText from "../../../tests/fixtures/vectorial_text.rs";

/** Plain programs: each prints its output as text. The first opens on a first visit. */
const RUST: Example[] = [
  { name: "Letter histogram", code: histogram },
  { name: "Moves and borrowing", code: moves },
  { name: "Use after move (error)", code: useAfterMove },
  { name: "Enums, match and ?", code: enums },
  { name: "Traits and generics", code: traits },
  { name: "Closures and iterators", code: iterators },
  { name: "Word count", code: wordCount },
  { name: "Strings and let else", code: strings },
  { name: "Async tasks", code: tasks },
].map((example) => ({ ...example, group: "Rust" }));

const ICED: Example[] = [
  { name: "Counter", code: counter, messages: ["Message::Increment", "Message::Increment"] },
  { name: "Checkbox", code: checkbox },
  { name: "Slider", code: slider },
  { name: "Progress bar", code: progressBar },
  { name: "Tooltip", code: tooltip },
  { name: "Stopwatch", code: stopwatch, messages: ["Message::Toggle"] },
  { name: "Timed easing", code: timed, messages: ["Message::Toggle"] },
  {
    name: "Component",
    code: component,
    messages: ["Message::NumericInputChanged(Some(1))", "Message::NumericInputChanged(Some(2))"],
  },
  {
    name: "Custom widget",
    code: customWidget,
    messages: ["Message::RadiusChanged(70.0)", "Message::RadiusChanged(90.0)"],
  },
  { name: "Combo box", code: comboBox },
  {
    name: "Modal",
    code: modal,
    messages: ["Message::ShowModal", 'Message::Email("ferris@example.com".to_string())'],
  },
  {
    name: "Lazy",
    code: lazy,
    messages: ['Message::AddItem("Rawr".to_string())', "Message::ToggleOrder"],
  },
  {
    name: "Vectorial text",
    code: vectorialText,
    messages: ["Message::AngleChanged(20.0)", "Message::SizeChanged(50.0)"],
  },
  { name: "Text", code: text },
  { name: "Scrollable", code: scrollable },
  { name: "Table", code: table },
  {
    name: "QR code",
    code: qrCode,
    size: "480x740",
    messages: [
      'Message::DataChanged("rawr".to_string())',
      'Message::DataChanged("https://rawr.airstrike.fyi".to_string())',
    ],
  },
  {
    name: "Styling",
    code: styling,
    size: "480x640",
    messages: ["Message::SliderChanged(60.0)", "Message::CheckboxToggled(true)", "Message::TogglerToggled(true)"],
  },
  { name: "Custom quad", code: customQuad, size: "480x660" },
  { name: "Arc", code: arc },
  { name: "Sandpiles", code: sandpiles },
].map((example) => ({ ...example, group: "iced" }));

export const EXAMPLES: Example[] = [...RUST, ...ICED];

/** What a first visit opens. */
export const FIRST: Example = EXAMPLES[0];
