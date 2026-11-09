//! Native: the app's widgets, as inboard's `widget`, with a `text` of its
//! own that a marked module imports beside `theme`, an `Element` alias
//! over the marked `Theme`, as inboard's `widget::Element` is, and the
//! crate's generic builders over that theme.
use crate::Theme;

pub mod sidebar;

pub use ui::vertical_space;

pub type Renderer = ui::Renderer;
pub type Element<'a, Message> = ui::Element<'a, Message, Theme, Renderer>;
pub type Scrollable<'a, Message> = ui::Scrollable<'a, Message, Element<'a, Message>, Theme>;

pub fn text(content: &str) -> String {
    format!("text {content}")
}

/// An element of nothing, as inboard's `widget::void`.
pub fn void<'a, Message: 'a>() -> Element<'a, Message> {
    use ui::Widget;
    ui::label("void").boxed()
}

/// Content that scrolls, as inboard's `widget::scrollable`.
pub fn scrollable<'a, Message>(
    content: impl ui::Widget<Message, Theme, Renderer> + 'a,
) -> Scrollable<'a, Message> {
    ui::scrollable(content.boxed())
}
