//! How a snippet becomes a program, the same on the `?play` server
//! (rawr-play) and in a browser (rawr-web): a plain one runs as a program
//! ([`plain`]), and an iced or async one gets a message instead
//! ([`route`]). A report on the program is shown against the snippet as
//! written ([`report`]).

pub mod plain;
mod report;
pub mod route;

pub use plain::without_tests;
pub use report::{in_source, panic, report, without_positions};

/// Identity used for the wrapped source in diagnostics.
pub const SOURCE_NAME: &str = "play.rs";
