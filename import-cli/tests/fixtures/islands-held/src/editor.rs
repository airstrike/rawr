//! Native: an editor's content, as iced's `text_editor::Content`, which the
//! marked screen holds, and the registry and sidebar its view borrows, as
//! iced's `Element<'a, Message>` borrows its state.

/// What the screen's input holds.
#[derive(Debug, Default)]
pub struct Content {
    text: String,
}

impl Content {
    pub fn push(&mut self, text: &str) {
        self.text.push_str(text);
    }

    pub fn text(&self) -> String {
        self.text.clone()
    }

    pub fn len(&self) -> usize {
        self.text.len()
    }
}

/// What the app knows, which a sidebar lists.
pub struct Registry {
    names: Vec<String>,
}

impl Registry {
    pub fn new(names: &[&str]) -> Self {
        Self {
            names: names.iter().map(|name| (*name).to_owned()).collect(),
        }
    }
}

/// A sidebar, which borrows the screen's title and the registry's names.
pub struct Sidebar<'a> {
    title: &'a str,
    names: &'a [String],
}

impl Sidebar<'_> {
    pub fn draw(&self) -> String {
        format!("[{}: {}]", self.title, self.names.join(", "))
    }
}

/// The sidebar of `title` over `registry`.
pub fn sidebar<'a>(title: &'a str, registry: &'a Registry) -> Sidebar<'a> {
    Sidebar {
        title,
        names: &registry.names,
    }
}
