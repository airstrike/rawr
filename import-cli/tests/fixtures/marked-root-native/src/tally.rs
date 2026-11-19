//! Native Rust below the marked root, which uses the root's items.
use super::Reading;

pub fn line(tick: i64) -> String {
    let reading = Reading {
        tick,
        value: super::scale(tick),
    };
    format!("{} value {}", reading.tick, reading.value)
}
