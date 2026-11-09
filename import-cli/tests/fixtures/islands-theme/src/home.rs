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
