//! How a snippet becomes a program, the same on the `?play` server
//! (rawr-play) and in a browser (rawr-web) ([`plain`]). A report on the
//! program is shown against the snippet as written ([`report`]).

pub mod plain;
mod report;

pub use plain::without_tests;
pub use report::{in_source, panic, report, without_positions};

/// Identity used for the wrapped source in diagnostics.
pub const SOURCE_NAME: &str = "play.rs";
