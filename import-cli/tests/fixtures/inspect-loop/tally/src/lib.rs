//! What the looping program calls on each turn, and the process it runs in.

/// The count after `count`.
pub fn next(count: u64) -> u64 {
    count + 1
}

/// The process's id, which a restart would change.
pub fn pid() -> u32 {
    std::process::id()
}
