//! A stand-in for iced's `Text::style`, which takes a function of the theme.

/// `size` in `style`.
pub fn styled(size: u32, style: impl Fn(u32) -> u32) -> u32 {
    style(size)
}
