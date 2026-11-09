//! Native: the inbox, as inboard's `workspace::inbox`. Its free `view`
//! keeps the theme it is lent borrowed in the element it gives, whose
//! `Theme` is the marked one, named through the crate root's re-export.
use crate::{Element, Theme};

#[derive(Debug, Clone, PartialEq)]
pub enum Message {
    Opened(u32),
}

/// Its `title` is borrowed for the call alone, as inboard's is.
pub fn view<'a>(title: &str, theme: &'a Theme) -> Element<'a, Message> {
    ui::label(title)
        .style(move |_: &Theme| theme.accent())
        .on_press(Message::Opened(theme.accent()))
}

/// A view that keeps its `title` borrowed as long as its element, beside
/// the theme.
pub fn titled<'a>(title: &'a str, theme: &'a Theme) -> Element<'a, Message> {
    use ui::Widget;
    ui::label(title)
        .style(move |_: &Theme| theme.accent() + title.len() as u32)
        .boxed()
}
