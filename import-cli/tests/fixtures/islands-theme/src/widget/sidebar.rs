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

impl<'a, Message: 'a> Default for Sidebar<'a, Message> {
    fn default() -> Self {
        Self {
            header: None,
            content: Content::default(),
        }
    }
}

impl<'a, Message: 'a> Default for Content<'a, Message> {
    fn default() -> Self {
        Self {
            items: Vec::new(),
            spacing: ui::Pixels(0.0),
        }
    }
}

impl<'a, Message: 'a> Sidebar<'a, Message> {
    pub fn header(
        mut self,
        title: &'a str,
        actions: impl IntoIterator<Item = Element<'a, Message>> + 'a,
    ) -> Self {
        self.header = Some(Header {
            title,
            actions: actions.into_iter().collect(),
        });
        self
    }

    pub fn content(mut self, content: impl IntoSections<'a, Message>) -> Self {
        self.content = content.into_sections();
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

    /// The sidebar's element, built from what native code keeps in it and
    /// leaves it empty, as a `&mut self` view moves its parts out (#1587).
    #[allow(clippy::wrong_self_convention)]
    pub fn to_element(&mut self) -> Element<'a, Message> {
        let mut children = Vec::new();
        if let Some(header) = &mut self.header {
            children.append(&mut header.actions);
        }
        children.append(&mut self.content.items);
        ui::Column::with_children(children)
            .spacing(self.content.spacing)
            .boxed()
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

/// What a sidebar takes as its content, as inboard's `IntoContent`: its
/// own `Content`, or any iterator of elements. Named apart from the two
/// other modules' `IntoContent`, as a method call doesn't yet weigh which
/// of them is in scope (#767).
pub trait IntoSections<'a, Message> {
    fn into_sections(self) -> Content<'a, Message>;
}

impl<'a, Message: 'a> IntoSections<'a, Message> for Content<'a, Message> {
    fn into_sections(self) -> Content<'a, Message> {
        self
    }
}

impl<'a, T, Message: 'a> IntoSections<'a, Message> for T
where
    T: IntoIterator<Item = Element<'a, Message>> + 'a,
{
    fn into_sections(self) -> Content<'a, Message> {
        Content {
            items: self.into_iter().collect(),
            spacing: ui::Pixels(0.0),
        }
    }
}

/// A sidebar with a header over `title` and `actions`, as inboard's
/// `sidebar::header`: `Sidebar::default()` takes `Message` from the
/// chain's later use.
pub fn header<'a, Message: 'a>(
    title: &'a str,
    actions: impl IntoIterator<Item = Element<'a, Message>> + 'a,
) -> Sidebar<'a, Message> {
    Sidebar::default().header(title, actions)
}

/// An empty sidebar, as inboard's `sidebar::default`, which nothing calls:
/// the tail's `Sidebar::default()` takes `Message` from the return type.
#[allow(dead_code)]
pub fn default<'a, Message: 'a>() -> Sidebar<'a, Message> {
    Sidebar::default()
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
