//! The parts of a component crate, as icedxdy_widget is to icedxdy: a slot
//! a row takes its children as, built by `From` from a `&str`, a `String`,
//! a `Cow<str>`, the widget crate's text at the crate's own theme, and the
//! crate's own icon and label. Its theme and renderer default to the
//! crate's own, which each `From` impl names by leaving them out. A
//! `Compose` implementor is a slot by `Slot::new`, as icedxdy_widget's
//! `Slot::new(child: impl Compose<Message, T, R> + 'a)` boxes one.

use std::borrow::Cow;
use std::marker::PhantomData;

use widgets::Plain;

/// The crate's own theme, the one its `Slot` takes text of.
pub struct Theme;

pub struct Slot<'a, Message, T = Theme, R = Plain> {
    label: String,
    message: PhantomData<(&'a Message, T, R)>,
}

/// What a slot holds, as icedxdy_widget's `Compose` is: its theme and
/// renderer default to the crate's own, so an impl that leaves them out
/// fixes them.
pub trait Compose<Message, T = Theme, R = Plain> {
    fn compose(&self) -> String;
}

impl<'a, Message: 'a, T: 'a, R: 'a> Slot<'a, Message, T, R> {
    /// Wrap any `Compose` implementor: its impl ties the slot's message,
    /// theme and renderer to the child's.
    pub fn new(child: impl Compose<Message, T, R> + 'a) -> Self {
        Slot::labeled(format!("composed {}", child.compose()))
    }
}

/// `Slot::new` with the bound in a where-clause.
pub fn wrap<'a, Message: 'a, C>(child: C) -> Slot<'a, Message>
where
    C: Compose<Message> + 'a,
{
    Slot::new(child)
}

impl<'a, Message, T, R> Slot<'a, Message, T, R> {
    fn labeled(label: String) -> Self {
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
        Slot::labeled(format!("str {text}"))
    }
}

impl<'a, Message: 'a> From<String> for Slot<'a, Message> {
    fn from(text: String) -> Self {
        Slot::labeled(format!("string {text}"))
    }
}

impl<'a, Message: 'a> From<Cow<'a, str>> for Slot<'a, Message> {
    fn from(text: Cow<'a, str>) -> Self {
        Slot::labeled(format!("cow {text}"))
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
        Slot::labeled(format!("icon {}", icon.size))
    }
}

/// An icon that is the widget crate's text, its theme left to the caller,
/// as a component crate's `icon::badge_check` is.
pub fn badge_check<'a, Theme>() -> widgets::Text<'a, Theme> {
    widgets::text("check")
}

impl<'a, Message: 'a> From<widgets::Text<'a, Theme>> for Slot<'a, Message> {
    fn from(text: widgets::Text<'a, Theme>) -> Self {
        Slot::labeled(format!("text {}", text.describe()))
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
        Slot::labeled(format!("label {}", label.text))
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

/// A row composes at the crate's own theme and renderer, its message the
/// slot's, as icedxdy_widget's `impl Compose<Message> for Row` does.
impl<'a, Message: 'a> Compose<Message> for Row<'a, Message> {
    fn compose(&self) -> String {
        format!("row {} [{}]", self.spacing, self.draw())
    }
}

/// A row is a slot of a row, as `row![..].into()` gives an `Element`.
impl<'a, Message: 'a> From<Row<'a, Message>> for Slot<'a, Message> {
    fn from(row: Row<'a, Message>) -> Self {
        Slot::labeled(format!("row {} [{}]", row.spacing, row.draw()))
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
        Slot::labeled("rule".to_owned())
    }
}

/// Any widget is a slot and any slot a widget, as icedxdy_widget's
/// `From<Element> for Slot` and `From<Slot> for Element` make them.
impl<'a, Message: 'a, T: 'a, R: 'a> From<widgets::Element<'a, Message, T, R>>
    for Slot<'a, Message, T, R>
{
    fn from(element: widgets::Element<'a, Message, T, R>) -> Self {
        Slot::labeled(format!("element {}", element.describe))
    }
}

impl<'a, Message: 'a, T: 'a, R: 'a> From<Slot<'a, Message, T, R>>
    for widgets::Element<'a, Message, T, R>
{
    fn from(slot: Slot<'a, Message, T, R>) -> Self {
        widgets::Element::described(slot.label)
    }
}

impl<'a, Message, T, R> Compose<Message, T, R> for widgets::Element<'a, Message, T, R> {
    fn compose(&self) -> String {
        format!("element {}", self.describe)
    }
}
