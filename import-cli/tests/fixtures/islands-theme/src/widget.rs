//! Native: the app's widgets, as inboard's `widget`, with a `text` of its
//! own that a marked module imports beside `theme`, and an `Element` alias
//! over the marked `Theme`, as inboard's `widget::Element` is.
use crate::Theme;

pub type Element<'a, Message> = ui::Element<'a, Message, Theme>;

pub fn text(content: &str) -> String {
    format!("text {content}")
}

/// An element of nothing, as inboard's `widget::void`.
pub fn void<'a, Message: 'a>() -> Element<'a, Message> {
    ui::label("void").boxed()
}
