//! Native: the inbox, as inboard's `workspace::inbox`. Its free `view`
//! keeps the theme it is lent borrowed in the element it gives, whose
//! `Theme` is the marked one, named through the crate root's re-export.
use crate::{Element, Theme};

#[derive(Debug, Clone, PartialEq)]
pub enum Message {
    Opened(u32),
}

pub fn view<'a>(theme: &'a Theme) -> Element<'a, Message> {
    ui::label("inbox")
        .style(move |_: &Theme| theme.accent())
        .on_press(Message::Opened(theme.accent()))
}
