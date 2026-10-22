#![forbid(unsafe_code)]

/// Imported semantic anchor used by the framework-neutral profile proof.
pub struct Clock;

impl Clock {
    /// Advance one deterministic logical tick.
    pub const fn tick(value: u64) -> u64 {
        value + 1
    }
}
