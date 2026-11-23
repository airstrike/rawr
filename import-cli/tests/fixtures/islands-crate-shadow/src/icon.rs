//! Native Rust: the code point of a copy icon, and an icon's size.

pub fn copy() -> u32 {
    0xF0C5
}

pub fn scaled(size: u32) -> u32 {
    size * 2
}
