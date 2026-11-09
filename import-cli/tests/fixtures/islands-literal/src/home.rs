// rawr
//! The home screen, marked, as inboard's `workspace/screen/home.rs`: a
//! greeting a private function picks from literals, and below it a
//! caption that passes a native function by name, as inboard's
//! `.style(theme::text::base)` does.

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

/// The caption's size, in the native style, passed to a crate function
/// and to a crate method.
fn caption() -> u32 {
    let size = ui::styled(theme::text_size(), style::caption);
    ui::Caption::new(size).style(style::plain).size()
}
