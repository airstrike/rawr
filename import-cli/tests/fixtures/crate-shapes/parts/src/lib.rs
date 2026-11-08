//! The parts of a component crate, as icedxdy_widget is to icedxdy: a slot
//! a row takes its children as, built by `From` from a `&str`, a `String`,
//! a `Cow<str>`, the widget crate's text at the crate's own theme, and the
//! crate's own icon and label. Its theme and renderer default to the
//! crate's own, which each `From` impl names by leaving them out.

use std::borrow::Cow;
use std::marker::PhantomData;

use widgets::Plain;

/// The crate's own theme, the one its `Slot` takes text of.
pub struct Theme;

pub struct Slot<'a, Message, T = Theme, R = Plain> {
    label: String,
    message: PhantomData<(&'a Message, T, R)>,
}

impl<'a, Message, T, R> Slot<'a, Message, T, R> {
    fn new(label: String) -> Self {
        Slot {
            label,
            message: PhantomData,
        }
    }

    pub fn label(&self) -> &str {
        &self.label
    }
}

impl<'a, Message: 'a> From<&'a str> for Slot<'a, Message> {
    fn from(text: &'a str) -> Self {
        Slot::new(format!("str {text}"))
    }
}

impl<'a, Message: 'a> From<String> for Slot<'a, Message> {
    fn from(text: String) -> Self {
        Slot::new(format!("string {text}"))
    }
}

impl<'a, Message: 'a> From<Cow<'a, str>> for Slot<'a, Message> {
    fn from(text: Cow<'a, str>) -> Self {
        Slot::new(format!("cow {text}"))
    }
}

pub struct Icon {
    size: u32,
}

pub fn icon() -> Icon {
    Icon { size: 16 }
}

impl Icon {
    pub fn size(self, size: u32) -> Self {
        Icon { size }
    }
}

impl<'a, Message: 'a> From<Icon> for Slot<'a, Message> {
    fn from(icon: Icon) -> Self {
        Slot::new(format!("icon {}", icon.size))
    }
}

/// An icon that is the widget crate's text, its theme left to the caller,
/// as a component crate's `icon::badge_check` is.
pub fn badge_check<'a, Theme>() -> widgets::Text<'a, Theme> {
    widgets::text("check")
}

impl<'a, Message: 'a> From<widgets::Text<'a, Theme>> for Slot<'a, Message> {
    fn from(text: widgets::Text<'a, Theme>) -> Self {
        Slot::new(format!("text {}", text.describe()))
    }
}

/// The crate's own builder, beside the widget crate's `Text`.
pub struct Label<'a> {
    text: &'a str,
}

pub fn label(text: &str) -> Label<'_> {
    Label { text }
}

impl<'a, Message: 'a> From<Label<'a>> for Slot<'a, Message> {
    fn from(label: Label<'a>) -> Self {
        Slot::new(format!("label {}", label.text))
    }
}

pub struct Row<'a, Message> {
    children: Vec<Slot<'a, Message>>,
    spacing: u32,
}

impl<'a, Message: 'a> Row<'a, Message> {
    pub fn with_children(children: impl IntoIterator<Item = Slot<'a, Message>>) -> Self {
        Row {
            children: children.into_iter().collect(),
            spacing: 0,
        }
    }

    pub fn spacing(mut self, spacing: u32) -> Self {
        self.spacing = spacing;
        self
    }

    pub fn gap(&self) -> u32 {
        self.spacing
    }

    pub fn draw(&self) -> String {
        let labels: Vec<&str> = self.children.iter().map(Slot::label).collect();
        labels.join(" | ")
    }
}

/// A row is a slot of a row, as `row![..].into()` gives an `Element`.
impl<'a, Message: 'a> From<Row<'a, Message>> for Slot<'a, Message> {
    fn from(row: Row<'a, Message>) -> Self {
        Slot::new(format!("row {} [{}]", row.spacing, row.draw()))
    }
}

/// A rule between children, whose slot has no message: its `From` impl
/// fixes the slot's message type at `()`, so a row it is in has that one.
pub struct Rule;

pub fn rule() -> Rule {
    Rule
}

impl<'a> From<Rule> for Slot<'a, ()> {
    fn from(_: Rule) -> Self {
        Slot::new("rule".to_owned())
    }
}
