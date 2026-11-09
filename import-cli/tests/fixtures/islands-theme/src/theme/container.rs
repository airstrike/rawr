//! Native: a style function of the marked `theme`'s, as inboard's
//! `theme/container.rs`.
use crate::theme::Theme;

pub fn base(theme: &Theme) -> u32 {
    theme.accent() + 1
}
