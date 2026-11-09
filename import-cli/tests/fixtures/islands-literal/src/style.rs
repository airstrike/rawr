//! Native: a caption's style, as inboard's `theme::text::base`, which the
//! marked screen passes by name.

pub fn caption(size: u32) -> u32 {
    size * 2
}

/// A caption's style that keeps its size.
pub fn plain(size: u32) -> u32 {
    size
}
