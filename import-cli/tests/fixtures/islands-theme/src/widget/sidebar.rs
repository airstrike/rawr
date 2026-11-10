// rawr
//! A marked widget module generic over the message, as inboard's
//! `widget/sidebar.rs`: native code calls it at its own message types,
//! directly and from a generic native helper.
use ui::Widget;

use crate::Element;

/// A sidebar row: a label that gives `message` when pressed.
pub fn row_group<'a, Message: Clone + 'a>(
    label: &'a str,
    message: Message,
) -> Element<'a, Message> {
    ui::label(&format!("{label} row")).on_press(message)
}

/// A sidebar native code builds through a chain of marked methods and
/// keeps, as inboard's `Sidebar<'a, Message>`: a header over native code's
/// own title and the elements native code gives it.
pub struct Sidebar<'a, Message: 'a> {
    pub header: Option<Header<'a, Message>>,
    pub content: Content<'a, Message>,
}

/// A sidebar's title and the actions beside it, as inboard's `Header`.
pub struct Header<'a, Message: 'a> {
    pub title: &'a str,
    pub actions: Vec<Element<'a, Message>>,
}

/// A sidebar's sections, as inboard's `Content`.
pub struct Content<'a, Message: 'a> {
    pub items: Vec<Element<'a, Message>>,
    pub spacing: ui::Pixels,
}

impl<'a, Message: 'a> Sidebar<'a, Message> {
    pub fn content(mut self, content: impl IntoIterator<Item = Element<'a, Message>>) -> Self {
        self.content.items = content.into_iter().collect();
        self
    }

    pub fn spacing(mut self, spacing: impl Into<ui::Pixels>) -> Self {
        self.content.spacing = spacing.into();
        self
    }

    pub fn title(&self) -> &'a str {
        self.header
            .as_ref()
            .map_or("untitled", |header| header.title)
    }

    pub fn push(&mut self, item: Element<'a, Message>) {
        self.content.items.push(item);
    }

    pub fn into_element(self) -> Element<'a, Message> {
        let Sidebar { header, content } = self;
        let mut items = Vec::new();
        let mut title = "untitled";
        if let Some(header) = header {
            title = header.title;
            items.extend(header.actions);
        }
        items.extend(content.items);
        let body = ui::Column::with_children(items)
            .spacing(content.spacing.0 as u32)
            .boxed();
        ui::Column::with_children([ui::label(&format!("{title} sidebar")).boxed(), body]).boxed()
    }
}

/// A sidebar with a header over `title` and `actions`, as inboard's
/// `sidebar::header`.
pub fn header<'a, Message: 'a>(
    title: &'a str,
    actions: impl IntoIterator<Item = Element<'a, Message>> + 'a,
) -> Sidebar<'a, Message> {
    Sidebar {
        header: Some(Header {
            title,
            actions: actions.into_iter().collect(),
        }),
        content: Content {
            items: Vec::new(),
            spacing: ui::Pixels(0.0),
        },
    }
}

/// A labeled group of rows, which native code turns into an element by
/// `.into()`.
pub struct Group<'a, Message> {
    pub label: &'a str,
    pub rows: Vec<Element<'a, Message>>,
}

impl<'a, Message: 'a> From<Group<'a, Message>> for Element<'a, Message> {
    fn from(group: Group<'a, Message>) -> Self {
        let rows = ui::Column::with_children(group.rows).boxed();
        ui::Column::with_children([ui::label(&format!("{} group", group.label)).boxed(), rows])
            .boxed()
    }
}
