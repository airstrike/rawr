// rawr
//! The home screen, marked to hot-reload, as inboard's `screen::home::State`:
//! native code holds it by value, makes it with `State::new()` and calls its
//! methods, and never reads its fields, one of which is native.

use crate::editor;

#[derive(Debug)]
pub struct State {
    input: editor::Content,
    title: String,
    ticks: usize,
}

impl State {
    pub fn new() -> Self {
        Self {
            input: editor::Content::default(),
            title: "home".to_owned(),
            ticks: 0,
        }
    }

    pub fn type_text(&mut self, text: &str) {
        self.input.push(text);
        self.title = format!("home ({})", self.input.len());
    }

    /// The screen's title, borrowed from it.
    pub fn title(&self) -> &str {
        &self.title
    }

    /// The sidebar, which borrows the screen's title and native code's
    /// registry both.
    pub fn sidebar<'a>(&'a self, registry: &'a editor::Registry) -> editor::Sidebar<'a> {
        editor::sidebar(&self.title, registry)
    }

    pub fn tick(&mut self) {
        self.ticks += 1;
    }

    pub fn view(&self) -> String {
        format!("ticks {} {}", self.ticks, self.input.text())
    }
}
