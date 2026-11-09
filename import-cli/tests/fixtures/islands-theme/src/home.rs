// rawr
//! A marked screen, as inboard's `home.rs`: it imports the marked `theme`
//! and `widget`'s `text`, and names the theme's native child modules
//! `theme::container` and `theme::text` through the imported `theme`. Each
//! style function is a crate widget's `style`, and each calls its own.
use crate::widget::text;
use crate::{theme, Element};

pub fn heading() -> String {
    format!("{} at {}", text("home"), theme::text::SIZE)
}

pub fn container() -> ui::Label<'static, theme::Theme> {
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

/// Where the theme native code lends is, as native code reads it.
pub fn place(theme: &theme::Theme) -> usize {
    crate::style::place(theme)
}
