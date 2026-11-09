//! Native: rule style functions of the marked `theme`'s, as inboard's
//! `theme/rule.rs`.
#![allow(dead_code)]
use crate::theme::Theme;

pub fn accent(theme: &Theme) -> u32 {
    theme.accent()
}

pub fn divider(theme: &Theme) -> u32 {
    theme.colors().text
}
