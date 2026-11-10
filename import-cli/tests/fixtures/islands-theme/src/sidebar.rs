// rawr
//! A marked module of widget builders, as inboard's `widget/sidebar.rs`:
//! boxed iterators of elements in its structs, a macro and extension traits
//! from a native crate, a struct update through its own `Default`, a
//! dependency's trait with only a lifetime argument, and closures handed to
//! crate widgets.
use common::space;
use common::text::IntoFragment;
use ui::{column, container, row, Fill, Padding, Pixels, Widget};

use crate::widget::{Container, TextInput};
use crate::{theme, Element, Theme};

pub struct Sidebar<'a, Message: 'a> {
    pub header: Option<Header<'a, Message>>,
    pub content: Content<'a, Message>,
}

impl<'a, Message: 'a> Default for Sidebar<'a, Message> {
    fn default() -> Self {
        Self {
            header: None,
            content: Content::default(),
        }
    }
}

impl<'a, Message: 'a> Sidebar<'a, Message> {
    pub fn content(mut self, content: impl IntoContent<'a, Message>) -> Self {
        self.content = content.into_content();
        self
    }

    pub fn header(
        mut self,
        title: &'a str,
        actions: impl IntoIterator<Item = Element<'a, Message>> + 'a,
    ) -> Self {
        self.header = Some(Header::new(title, actions));
        self
    }

    pub fn spacing(mut self, spacing: impl Into<Pixels>) -> Self {
        self.content.spacing = spacing.into();
        self
    }

    /// The header's next action, pulled by marked code from the iterator
    /// native code holds and pulls on from after.
    pub fn next_action(&mut self) -> Option<Element<'a, Message>> {
        match &mut self.header {
            Some(header) => header.actions.next(),
            None => None,
        }
    }

    #[allow(clippy::wrong_self_convention)]
    pub fn to_element(&mut self) -> Element<'a, Message> {
        match &mut self.header {
            Some(header) => column![header.to_element(), self.content.to_element()],
            None => column![self.content.to_element()],
        }
        .spacing(5)
        .width(Fill)
        .boxed()
    }
}

pub struct Header<'a, Message: 'a> {
    pub title: &'a str,
    pub actions: Box<dyn Iterator<Item = Element<'a, Message>> + 'a>,
}

impl<'a, Message: 'a> Header<'a, Message> {
    pub fn new(
        title: &'a str,
        actions: impl IntoIterator<Item = Element<'a, Message>> + 'a,
    ) -> Self {
        Self {
            title,
            actions: Box::new(actions.into_iter()),
        }
    }

    #[allow(clippy::wrong_self_convention)]
    pub fn to_element(&mut self) -> Element<'a, Message> {
        row([
            crate::widget::label(self.title)
                .style(theme::text::base)
                .boxed(),
            space!().boxed(),
        ]
        .into_iter()
        .chain(&mut self.actions))
        .boxed()
    }
}

pub struct Content<'a, Message: 'a> {
    pub scrollable: bool,
    pub content: Box<dyn Iterator<Item = Element<'a, Message>> + 'a>,
    pub spacing: Pixels,
    pub padding: Padding,
}

impl<'a, Message: 'a> Default for Content<'a, Message> {
    fn default() -> Self {
        Self {
            scrollable: false,
            content: Box::new(std::iter::empty()),
            spacing: 5.into(),
            padding: [4, 0].into(),
        }
    }
}

impl<'a, Message: 'a> Content<'a, Message> {
    pub fn with_children(content: impl IntoIterator<Item = Element<'a, Message>> + 'a) -> Self {
        Content {
            content: Box::new(content.into_iter()),
            ..Default::default()
        }
    }

    #[allow(clippy::wrong_self_convention)]
    pub fn to_element(&mut self) -> Element<'a, Message> {
        let base = ui::Column::with_children(&mut self.content)
            .spacing(self.spacing)
            .width(Fill);
        if self.scrollable {
            crate::widget::scrollable(base).boxed()
        } else {
            base.boxed()
        }
    }
}

pub trait IntoContent<'a, Message> {
    fn into_content(self) -> Content<'a, Message>;
}

impl<'a, Message: 'a> IntoContent<'a, Message> for Content<'a, Message> {
    fn into_content(self) -> Content<'a, Message> {
        self
    }
}

impl<'a, T, Message: 'a> IntoContent<'a, Message> for T
where
    T: IntoIterator<Item = Element<'a, Message>> + 'a,
{
    fn into_content(self) -> Content<'a, Message> {
        Content::with_children(self)
    }
}

pub fn header<'a, Message: 'a>(
    title: &'a str,
    actions: impl IntoIterator<Item = Element<'a, Message>> + 'a,
) -> Sidebar<'a, Message> {
    Sidebar::default().header(title, actions)
}

pub fn header_row<'a, Message: 'a>(
    title: &'a str,
    actions: impl IntoIterator<Item = Element<'a, Message>> + 'a,
) -> Element<'a, Message> {
    let mut h = Header::new(title, actions);
    h.to_element()
}

pub fn tab_label<'a, Message: Clone + 'a>(label: impl IntoFragment<'a>) -> Element<'a, Message> {
    crate::widget::label(label).boxed()
}

pub fn row_group<'a, Message: 'a>(
    content: impl IntoIterator<Item = Element<'a, Message>>,
) -> Container<'a, Message> {
    container(row(content).boxed()).style(theme::container::base)
}

pub fn bare_input<'a, Message: Clone + 'a>(
    placeholder: &str,
    value: &str,
) -> TextInput<'a, Message> {
    crate::widget::text_input(placeholder, value)
}

pub fn percentage_input<'a, Message: Clone + 'a>(
    value: &str,
    on_input: impl Fn(String) -> Message + 'a,
    on_submit: Message,
) -> Container<'a, Message> {
    row_group([
        bare_input("100", value)
            .on_input(on_input)
            .on_submit(on_submit)
            .boxed(),
        crate::widget::label("%").style(theme::text::base).boxed(),
    ])
}

/// What the panel's widgets give.
#[derive(Debug, Clone, PartialEq)]
pub enum Message {
    Tab(u32),
    Typed(String),
    Submitted,
    Pressed,
}

/// The panel native code draws, as inboard's views build their sidebars:
/// each builder above at the panel's own message, over the theme native
/// code lends.
pub fn panel(theme: &Theme) -> Element<'_, Message> {
    let tabs = [tab_label("one"), tab_label(String::from("two"))];
    let actions = [crate::widget::label("+").boxed()];
    let input = percentage_input("50", Message::Typed, Message::Submitted);
    let mut sidebar = header("tools", actions)
        .content([row(tabs).boxed(), input.boxed()])
        .spacing(4);
    let mut rows = Content::with_children([header_row("rows", [])]);
    column![sidebar.to_element(), rows.to_element()]
        .width(Fill.max(theme.accent() as f32))
        .boxed()
}

/// A column of an element marked code builds and one native code gives,
/// in one `Vec` (#1588).
pub fn mixed(given: Element<'_, Message>) -> Element<'_, Message> {
    let items = vec![crate::widget::label("built").boxed(), given];
    ui::Column::with_children(items).boxed()
}

/// What typing in the panel's input gives: the closure native code calls
/// is the marked `Message::Typed`, handed on to the crate's `on_input`.
pub fn typed() -> Option<Message> {
    let input: Element<'_, Message> =
        percentage_input("50", Message::Typed, Message::Submitted).boxed();
    input.message()
}
