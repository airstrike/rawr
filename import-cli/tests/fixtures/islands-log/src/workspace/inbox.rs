// rawr
//! The inbox, marked, as inboard's `workspace/inbox.rs`: it compares
//! `tracing`'s and `log`'s levels as their macros do.

/// `tracing`'s enabled check, a `Level` compared with a `LevelFilter`, and
/// `log`'s, each way round.
pub fn levels() -> String {
    use tracing::level_filters::{LevelFilter, STATIC_MAX_LEVEL};
    use tracing::Level;
    let level = Level::WARN;
    let compiled = level <= STATIC_MAX_LEVEL;
    let current = level <= LevelFilter::current();
    let off = Level::TRACE > LevelFilter::OFF;
    let filter = LevelFilter::INFO >= Level::DEBUG;
    let equal = LevelFilter::WARN == Level::WARN;
    let unequal = Level::ERROR != LevelFilter::WARN;
    let same = Level::ERROR < Level::WARN;
    let log = log::Level::Warn <= log::STATIC_MAX_LEVEL && log::Level::Debug > log::max_level();
    format!(
        "compiled {compiled} current {current} off {off} filter {filter} \
         equal {equal} unequal {unequal} same {same} log {log}"
    )
}
