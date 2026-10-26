// rawr
//! pulldown-cmark's `examples/events.rs`, on a fixed text and printing to
//! stdout: every event of the text, indented by its nesting.

use pulldown_cmark::{Event, Parser};

pub fn main() {
    let text = String::from(
        "# Title\n\nSome *emphasis*, **strong** and `code`.\n\n- one\n- two [link](https://example.com)\n\n> quoted\n",
    );
    let mut width = 0;
    for event in Parser::new(&text) {
        if let Event::End(_) = event {
            width -= 2;
        }
        println!("  {:width$}{event:?}", "");
        if let Event::Start(_) = event {
            width += 2;
        }
    }
}
