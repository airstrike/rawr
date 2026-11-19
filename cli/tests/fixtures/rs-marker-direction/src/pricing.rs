//! Native Rust that uses an item of the marked crate root, as Cargo builds
//! it. Rawr's copy of the crate stands a trampoline for each marked item.
use crate::Item;

pub fn doubled(item: &Item) -> i64 {
    item.cents * 2
}
