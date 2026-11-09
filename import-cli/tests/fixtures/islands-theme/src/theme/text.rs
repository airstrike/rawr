//! Native: a style function of the marked `theme`'s, as inboard's
//! `theme/text.rs`, whose name is also the `text` of `widget`.
use crate::theme::Theme;

pub const SIZE: u32 = 12;

pub fn base(theme: &Theme) -> u32 {
    theme.colors().text * 3
}
