//! Native: the home screen's grid, as inboard's `dataplane::surface::grid`.
//! Its `home` gives an element over the lent theme by value inside an
//! `Option`, as inboard's `dashboards::view_home(.., theme)` does.
use crate::{Element, Theme};

#[derive(Debug, Clone, PartialEq)]
pub enum Message {
    Tile(u32),
}

pub fn home<'a>(theme: &'a Theme) -> Option<Element<'a, Message>> {
    Some(
        ui::label("grid")
            .style(move |_: &Theme| theme.accent() + 1)
            .on_press(Message::Tile(theme.accent())),
    )
}
