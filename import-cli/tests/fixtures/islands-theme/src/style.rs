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
