//! Native Rust below the marked root, which uses the root's items, and reads
//! the run's environment for it.
use super::Reading;

pub fn line(tick: i64) -> String {
    let reading = Reading {
        tick,
        value: super::scale(tick),
    };
    format!("{} value {}", reading.tick, reading.value)
}

/// How many ticks to count: `ISLANDS_TICKS`, or 3000.
pub fn ticks() -> i64 {
    std::env::var("ISLANDS_TICKS")
        .ok()
        .and_then(|ticks| ticks.parse().ok())
        .unwrap_or(3000)
}

/// Whether the file `ISLANDS_STOP` names exists.
pub fn stopped() -> bool {
    std::env::var_os("ISLANDS_STOP").is_some_and(|path| std::path::Path::new(&path).exists())
}
