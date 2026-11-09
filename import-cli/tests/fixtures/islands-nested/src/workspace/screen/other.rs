//! Native: a sibling of the marked `home`, private to `screen`. Its
//! `pub(super)` constant is visible in `screen` and below it, so in `home`.
pub(super) const SCALE: usize = 10;

pub fn helper(visits: usize) -> usize {
    visits * SCALE
}
