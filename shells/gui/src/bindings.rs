//! The bindings of the frame shown, each value a tree that folds open
//! entry by entry. What is open stays open across stops, by each entry's
//! place under its binding's name, so a step keeps the view.

use std::collections::HashSet;

use iced::widget::{button, column, row, space, text};
use iced::{Element, Font};

use crate::value::Node;

/// Text longer than this shows its start.
const SHORT: usize = 96;

/// The frame's bindings and the entries open in them.
#[derive(Debug, Default)]
pub struct State {
    bindings: Vec<Binding>,
    unread: Option<String>,
    open: HashSet<String>,
}

/// One binding: its name and its value, or why it has none.
#[derive(Debug)]
pub struct Binding {
    name: String,
    value: Result<Node, String>,
}

#[derive(Clone, Debug)]
pub enum Message {
    /// Open or fold the entry at this place.
    Toggle(String),
}

impl Binding {
    /// A binding from the debugger's answer: `{"name", "value"}` or
    /// `{"name", "error"}`.
    #[must_use]
    pub fn from_json(binding: &serde_json::Value) -> Self {
        let name = binding["name"].as_str().unwrap_or_default().to_owned();
        let value = match binding["value"].as_str() {
            Some(value) => Ok(Node::parse(value)),
            None => Err(binding["error"].as_str().unwrap_or_default().to_owned()),
        };
        Self { name, value }
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
            return text(format!("No bindings to read: {unread}")).into();
        }
        let mut rows = Vec::new();
        for binding in &self.bindings {
            match &binding.value {
                Ok(node) => self.rows(&mut rows, &binding.name, Some(&binding.name), node, 0),
                Err(error) => rows.push(
                    row![
                        text(binding.name.as_str()).font(Font::MONOSPACE),
                        text(error.as_str()).style(text::danger),
                    ]
                    .spacing(8)
                    .into(),
                ),
            }
        }
        column(rows).spacing(2).into()
    }

    /// The rows of `node` at `place`, named `name`, `depth` entries deep.
    fn rows<'a>(
        &'a self,
        rows: &mut Vec<Element<'a, Message>>,
        place: &str,
        name: Option<&'a str>,
        node: &'a Node,
        depth: u16,
    ) {
        let open = node.is_group() && self.open.contains(place);
        let toggle: Element<'a, Message> = if node.is_group() {
            button(text(if open { "▾" } else { "▸" }).font(Font::MONOSPACE))
                .padding(0)
                .style(button::text)
                .on_press(Message::Toggle(place.to_owned()))
                .into()
        } else {
            text(" ").font(Font::MONOSPACE).into()
        };
        let mut line = row![space::horizontal().width(f32::from(depth) * 16.0), toggle].spacing(4);
        if let Some(name) = name.or(node.key.as_deref()) {
            line = line.push(text(name).font(Font::MONOSPACE).style(text::primary));
        }
        let shown = match (&node.group, open) {
            (Some(group), true) => format!("{} {}", node.head, group.open)
                .trim_start()
                .to_owned(),
            _ => short(&node.text),
        };
        rows.push(line.push(text(shown).font(Font::MONOSPACE)).into());
        let Some(group) = node.group.as_ref().filter(|_| open) else {
            return;
        };
        for (index, entry) in group.entries.iter().enumerate() {
            self.rows(rows, &format!("{place}.{index}"), None, entry, depth + 1);
        }
        rows.push(
            row![
                space::horizontal().width(f32::from(depth) * 16.0 + 20.0),
                text(group.close.to_string()).font(Font::MONOSPACE),
            ]
            .into(),
        );
    }
}

/// `text`, or its start when it is long.
fn short(text: &str) -> String {
    match text.char_indices().nth(SHORT) {
        Some((end, _)) => format!("{}…", &text[..end]),
        None => text.to_owned(),
    }
}
