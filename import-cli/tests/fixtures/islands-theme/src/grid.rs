//! Native: the home screen's data-tile grid, as inboard's
//! `dataplane::surface::grid`, whose element native code gives the marked
//! home screen by value.
use crate::{Element, Theme};

#[derive(Debug, Clone, PartialEq)]
pub enum Message {
    Resized(u32),
}

pub fn view(theme: &Theme) -> Element<'_, Message> {
    ui::label("grid")
        .style(move |_: &Theme| theme.accent() * 2)
        .on_press(Message::Resized(2))
}
