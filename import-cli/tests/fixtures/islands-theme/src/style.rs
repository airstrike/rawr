//! Native style functions, as inboard's `theme/*.rs`: each reads the
//! theme's colors through `Theme::colors`, a reference into the theme.
use crate::theme::Theme;

pub fn button(theme: &Theme) -> u32 {
    theme.colors().accent + theme.colors().text
}

pub fn text(theme: &Theme) -> u32 {
    let colors = theme.colors();
    colors.text * 2
}

/// A badge that keeps the theme borrowed, as inboard's `inbox::view(..,
/// theme)` gives an `Element<'a, Message>` that does.
pub fn badge<'a>(theme: &'a Theme) -> ui::Badge<'a, Theme> {
    ui::badge("home", theme)
}

/// Where the theme it is given is, which shows it is native code's own.
pub fn place(theme: &Theme) -> usize {
    std::ptr::from_ref(theme).addr()
}
