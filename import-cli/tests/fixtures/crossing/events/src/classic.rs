//! Widgets in the shape of iced's before its widgets took their content
//! as a type parameter: each carries its message, theme and renderer, as
//! `Column<'a, Message, Theme, Renderer>`, an element is made from each by
//! an impl of `From` of its own, and a function takes its content as `impl
//! Into<Element<..>>`. A view of text alone names its message, theme and
//! renderer only by its return type, `Element<'_, Message>`.

use crate::widgets::Widget;

/// Any widget, boxed, at the crate's theme and renderer by default.
pub struct Element<'a, Message, Theme = crate::Theme, Renderer = crate::widgets::Renderer>(
    Box<dyn Widget<Message, Theme, Renderer> + 'a>,
);

impl<Message, Theme, Renderer> Widget<Message, Theme, Renderer>
    for Element<'_, Message, Theme, Renderer>
{
    fn labels(&self, labels: &mut Vec<String>) {
        self.0.labels(labels);
    }

    fn press(&self, label: &str) -> Option<Message> {
        self.0.press(label)
    }
}

/// Text at a theme and renderer, which sends no message.
pub struct Text<'a, Theme, Renderer> {
    text: String,
    look: std::marker::PhantomData<&'a (Theme, Renderer)>,
}

/// Text showing `value`.
pub fn text<'a, Theme, Renderer>(value: impl ToString) -> Text<'a, Theme, Renderer> {
    Text {
        text: value.to_string(),
        look: std::marker::PhantomData,
    }
}

impl<Message, Theme, Renderer> Widget<Message, Theme, Renderer> for Text<'_, Theme, Renderer> {
    fn labels(&self, labels: &mut Vec<String>) {
        labels.push(self.text.clone());
    }

    fn press(&self, _label: &str) -> Option<Message> {
        None
    }
}

impl<'a, Message, Theme: 'a, Renderer: 'a> From<Text<'a, Theme, Renderer>>
    for Element<'a, Message, Theme, Renderer>
{
    fn from(text: Text<'a, Theme, Renderer>) -> Self {
        Element(Box::new(text))
    }
}

/// Widgets one above another.
pub struct Column<'a, Message, Theme, Renderer> {
    children: Vec<Element<'a, Message, Theme, Renderer>>,
}

/// `content` in the middle, as iced's `center`.
pub fn center<'a, Message, Theme, Renderer>(
    content: impl Into<Element<'a, Message, Theme, Renderer>>,
) -> Column<'a, Message, Theme, Renderer> {
    Column {
        children: vec![content.into()],
    }
}

impl<Message, Theme, Renderer> Widget<Message, Theme, Renderer>
    for Column<'_, Message, Theme, Renderer>
{
    fn labels(&self, labels: &mut Vec<String>) {
        labels.push("center".to_owned());
        for child in &self.children {
            child.labels(labels);
        }
    }

    fn press(&self, label: &str) -> Option<Message> {
        self.children.iter().find_map(|child| child.press(label))
    }
}

impl<'a, Message: 'a, Theme: 'a, Renderer: 'a> From<Column<'a, Message, Theme, Renderer>>
    for Element<'a, Message, Theme, Renderer>
{
    fn from(column: Column<'a, Message, Theme, Renderer>) -> Self {
        Element(Box::new(column))
    }
}
