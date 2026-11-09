// rawr
//! A marked screen, as inboard's `home.rs`: it imports the marked `theme`
//! and `widget`'s `text`, and names the theme's native child modules
//! `theme::container` and `theme::text` through the imported `theme`. Each
//! style function is a crate widget's `style`, and each calls its own.
use ui::{column, container, opaque, stack, Fill, Widget};

use crate::widget::text;
use crate::{theme, Element};

pub mod wizard;

pub fn heading() -> String {
    format!("{} at {}", text("home"), theme::text::SIZE)
}

pub fn framed() -> ui::Label<'static, theme::Theme> {
    ui::label("container").style(theme::container::base)
}

pub fn label() -> ui::Label<'static, theme::Theme> {
    ui::label("text").style(theme::text::base)
}

/// A view native code lends the theme to, as inboard's `home::view` gives
/// it on to `inbox::view(.., theme)`: a native style function, a crate
/// widget's `draw` and a native function, each taking `&Theme`, are given
/// the lent theme itself.
pub fn view(theme: &theme::Theme) -> String {
    format!(
        "{} {} {} {}",
        theme::text::base(theme),
        label().draw(theme),
        crate::style::button(theme),
        ui::base(theme)
    )
}

/// A badge that keeps the lent theme borrowed as long as native code holds
/// it, as inboard's `view<'a>` gives `inbox::view(.., theme)`'s element.
pub fn badge<'a>(theme: &'a theme::Theme) -> ui::Badge<'a, theme::Theme> {
    crate::style::badge(theme)
}

/// Nothing, as inboard's `pending_bar` gives `crate::widget::void()`: the
/// native alias's `Theme` is the marked one.
pub fn empty<'a>() -> Element<'a, u32> {
    crate::widget::void()
}

/// A caption over native code's own text, as inboard's
/// `sidebar::subtitle`: the crate's `Text`, invariant in its lifetime,
/// borrows the `&'a str` native code gives, and native code keeps it.
pub fn caption<'a>(label: &'a str) -> ui::Text<'a> {
    ui::text(label).size(12)
}

/// A panel native code builds through a chain of marked methods and keeps
/// between calls, as inboard's `Sidebar<'a, Message>`: it holds native
/// code's own `&'a str` and the elements native code gives it.
pub struct Panel<'a> {
    title: &'a str,
    items: Vec<Element<'a, Message>>,
}

impl<'a> Panel<'a> {
    pub fn new(title: &'a str) -> Self {
        Self {
            title,
            items: Vec::new(),
        }
    }

    pub fn item(mut self, item: Element<'a, Message>) -> Self {
        self.items.push(item);
        self
    }

    pub fn push(&mut self, item: Element<'a, Message>) {
        self.items.push(item);
    }

    pub fn title(&self) -> &'a str {
        self.title
    }

    pub fn draw(&self, theme: &theme::Theme) -> String {
        let items = self
            .items
            .iter()
            .map(|item| item.draw(theme))
            .collect::<Vec<_>>();
        format!("{} [{}]", self.title, items.join(", "))
    }
}

/// What the screen's elements give, as inboard's `home::Message`.
#[derive(Debug, Clone, PartialEq)]
pub enum Message {
    Inbox(crate::inbox::Message),
    Grid(crate::grid::Message),
    Wizard(wizard::Message),
}

/// The inbox's element over the lent theme, its message mapped into the
/// screen's, as inboard's `home::State::view` gives `inbox::view(..,
/// theme).map(Message::Inbox)`, whose title is borrowed for the call
/// alone (#1509).
pub fn inbox<'a>(theme: &'a theme::Theme) -> Element<'a, Message> {
    crate::inbox::view("inbox", theme)
        .map(Message::Inbox)
        .boxed()
}

/// An element that keeps both its lent title and the lent theme borrowed
/// (#1509).
pub fn titled<'a>(title: &'a str, theme: &'a theme::Theme) -> Element<'a, Message> {
    crate::inbox::titled(title, theme)
        .map(Message::Inbox)
        .boxed()
}

/// The screen's page, as inboard's `home::State::view`: the grid native
/// code builds and gives by value inside an `Option`, whose `Element` names
/// the marked theme, its message mapped into the screen's.
pub fn page<'a>(
    grid: Option<Element<'a, crate::grid::Message>>,
    theme: &'a theme::Theme,
) -> Element<'a, Message> {
    let mut page = crate::inbox::view("inbox", theme)
        .map(Message::Inbox)
        .boxed();
    if let Some(grid) = grid {
        page = grid.map(Message::Grid).boxed();
    }
    page
}

/// A sidebar's sections, as inboard's `widget::sidebar::Content`.
pub struct Content<'a, Message> {
    items: Vec<Element<'a, Message>>,
}

/// What a sidebar takes as its content, as inboard's `IntoContent`: a
/// trait of the crate's own, generic in a lifetime and a message.
pub trait IntoContent<'a, Message> {
    fn into_content(self) -> Content<'a, Message>;
}

impl<'a, Message> IntoContent<'a, Message> for Content<'a, Message> {
    fn into_content(self) -> Content<'a, Message> {
        self
    }
}

/// A sidebar, as inboard's `widget::Sidebar`, whose generic impl's
/// methods bound their arguments by `IntoContent` over the impl's own
/// lifetime and message.
pub struct Sidebar<'a, Message> {
    content: Option<Content<'a, Message>>,
}

impl<'a, Message> Sidebar<'a, Message> {
    pub fn new() -> Self {
        Self { content: None }
    }

    pub fn content(mut self, content: impl IntoContent<'a, Message>) -> Self {
        self.content = Some(content.into_content());
        self
    }

    pub fn sections(&self) -> usize {
        self.content
            .as_ref()
            .map_or(0, |content| content.items.len())
    }
}

/// A sidebar's content, as inboard's free `sidebar::content`.
pub fn content<'a, Message: 'a>(
    content: impl IntoContent<'a, Message> + 'a,
) -> Content<'a, Message> {
    content.into_content()
}

/// A sidebar of the screen's elements, which native code calls: the
/// generic sidebar's methods at the screen's `Message`.
pub fn sidebar() -> usize {
    let items: Vec<Element<'static, Message>> = vec![crate::widget::void(), crate::widget::void()];
    Sidebar::new()
        .content(content(Content { items }))
        .sections()
}

/// Where the theme native code lends is, as native code reads it.
pub fn place(theme: &theme::Theme) -> usize {
    crate::style::place(theme)
}

/// The home screen native code keeps, as inboard's `home::State`, with its
/// open wizard, a native part of it.
#[derive(Debug)]
pub struct State {
    title: String,
    wizard: Option<wizard::State>,
}

impl State {
    pub fn new(title: &str) -> Self {
        Self {
            title: title.to_owned(),
            wizard: None,
        }
    }

    pub fn open_wizard(&mut self, step: u32) {
        self.wizard = Some(wizard::State::new(step));
    }

    /// The screen, as inboard's `home::State::view`: the inbox's element
    /// over the lent theme, a native grid's element mapped and boxed into
    /// a column, which the crate's generic builders over the marked theme
    /// scroll, center and style with a native style function, and the open
    /// wizard's element, over the catalog native code gives for this call,
    /// stacked on top.
    pub fn view<'a>(
        &'a self,
        catalog: wizard::View<'a>,
        grid: Option<Element<'a, crate::grid::Message>>,
        theme: &'a theme::Theme,
    ) -> Element<'a, Message> {
        let top_bar = crate::inbox::view(&self.title, theme).map(Message::Inbox);

        let mut content = column![top_bar].spacing(0).width(Fill);

        content = content.push(crate::widget::void());

        if let Some(grid) = grid {
            content = content.push(grid.map(Message::Grid).boxed());
        }

        let scrolling = crate::widget::scrollable(
            container(
                column![crate::widget::vertical_space().height(48), content]
                    .width(Fill.max(1480.0)),
            )
            .center_x(Fill),
        )
        .height(Fill);

        let page: Element<'a, Message> = container(scrolling)
            .style(theme::container::base)
            .width(Fill)
            .height(Fill)
            .boxed();

        match &self.wizard {
            Some(wizard) => stack![
                page,
                opaque(wizard.view(catalog, theme).map(Message::Wizard))
            ]
            .boxed(),
            None => page,
        }
    }
}
