//! The bindings of the frame shown, as a tree table: each binding a row,
//! and each value with parts opens to a row per part. What is open stays
//! open across stops, by each row's place under its binding's name, so a
//! step keeps the view.

use std::collections::HashSet;

use iced::widget::text;
use iced::{Element, Widget};

use crate::theme;
use crate::tree::{self, Part};

/// The frame's bindings and the rows open in them.
#[derive(Debug, Default)]
pub struct State {
    bindings: Vec<Binding>,
    unread: Option<String>,
    open: HashSet<String>,
}

/// One binding: its name and its value, or why it has none, and the
/// value's parts.
#[derive(Debug)]
pub struct Binding {
    name: String,
    value: Result<String, String>,
    parts: Vec<Part>,
}

#[derive(Clone, Debug)]
pub enum Message {
    /// Open or fold the row at this place.
    Toggle(String),
}

impl Binding {
    /// A binding from the debugger's answer: `{"name", "value"}`, with
    /// `children` when the value has parts, or `{"name", "error"}`.
    #[must_use]
    pub fn from_json(binding: &serde_json::Value) -> Self {
        let name = binding["name"].as_str().unwrap_or_default().to_owned();
        let value = match binding["value"].as_str() {
            Some(value) => Ok(value.to_owned()),
            None => Err(binding["error"].as_str().unwrap_or_default().to_owned()),
        };
        Self {
            name,
            value,
            parts: Part::all(&binding["children"]),
        }
    }
}

impl State {
    /// Show `bindings`, or why the frame has none to read.
    pub fn show(&mut self, bindings: Vec<Binding>, unread: Option<String>) {
        self.bindings = bindings;
        self.unread = unread;
    }

    pub fn update(&mut self, message: Message) {
        match message {
            Message::Toggle(place) => {
                if !self.open.remove(&place) {
                    self.open.insert(place);
                }
            }
        }
    }

    pub fn view(&self) -> Element<'_, Message> {
        if let Some(unread) = &self.unread {
            return text(format!("No bindings to read: {unread}"))
                .size(theme::SIZE)
                .style(text::secondary)
                .boxed();
        }
        let roots = self.bindings.iter().map(|binding| tree::Root {
            place: &binding.name,
            name: &binding.name,
            value: binding.value.as_deref().map_err(String::as_str),
            parts: &binding.parts,
        });
        tree::view(roots, &self.open, Message::Toggle)
    }
}
