// iced's examples from play/tests/fixtures, embedded when the Worker is
// built (wrangler.jsonc's "rules" import *.rs files as text).

import type { Example } from "./playpage.ts";
import checkbox from "../../../tests/fixtures/checkbox.rs";
import comboBox from "../../../tests/fixtures/combo_box.rs";
import counter from "../../../tests/fixtures/counter.rs";
import progressBar from "../../../tests/fixtures/progress_bar.rs";
import slider from "../../../tests/fixtures/slider.rs";
import tooltip from "../../../tests/fixtures/tooltip.rs";

export const COUNTER: Example = {
  name: "Counter",
  code: counter,
  messages: ["Message::Increment", "Message::Increment"],
};

export const EXAMPLES: Example[] = [
  COUNTER,
  { name: "Checkbox", code: checkbox },
  { name: "Slider", code: slider },
  { name: "Progress bar", code: progressBar },
  { name: "Tooltip", code: tooltip },
  { name: "Combo box", code: comboBox },
];
