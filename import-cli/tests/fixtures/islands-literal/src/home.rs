// rawr
//! The home screen, marked, as inboard's `workspace/screen/home.rs`: a
//! greeting a private function picks from literals, and below it a
//! caption that passes a native function by name, as inboard's
//! `.style(theme::text::base)` does, and at the end of a builder chain,
//! as tooltip's `.gap(..).style(container::rounded_box)` does. It sums
//! the caption with a crate function generic over a const length.

use crate::{style, theme};

pub struct State {
    count: u32,
}

impl State {
    pub fn new() -> Self {
        Self { count: 0 }
    }

    pub fn tick(&mut self) {
        self.count += 1;
    }

    pub fn count(&self) -> u32 {
        self.count
    }

    pub fn greeting(&self) -> String {
        greeting(self.count as usize, "Ada")
    }
}

/// A greeting from the pool, in a caption.
fn greeting(index: usize, name: &str) -> String {
    let greetings: &[&str] = &["Ready when you are.", "Good morning, {name}."];
    let text = greetings[index % greetings.len()].replace("{name}", name);
    format!("{text} at {}", caption())
}

/// The caption's size, in the native style, passed to a crate function,
/// to a crate method and at the end of a builder chain, then summed by a
/// crate function generic over its array's length.
fn caption() -> u32 {
    let size = ui::styled(theme::text_size(), style::caption);
    let size = ui::Caption::new(size).style(style::plain).size();
    let size = ui::tip(size).gap(0).style(style::plain).size();
    ui::total([size, 0])
}
