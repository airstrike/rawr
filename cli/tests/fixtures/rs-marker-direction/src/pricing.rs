//! Native Rust that uses an item of the marked crate root. Cargo builds
//! this crate, and Rawr refuses it: Rawr builds the native modules without
//! the marked ones.
use crate::Item;

pub fn doubled(item: &Item) -> i64 {
    item.cents * 2
}
