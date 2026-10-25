//! How a snippet becomes a program, the same on the `?play` server
//! (rawr-play) and in a browser (rawr-web): an iced snippet is wrapped as a
//! preview or an app ([`wrap`], [`live`]), and a plain one as a program
//! ([`plain`]). A report on the wrapped program is shown against the
//! snippet as written ([`report`]). Everything here works on text.

pub mod plain;
mod report;
mod wrap;

pub use report::{in_source, report, without_positions};
pub use wrap::{
    app_theme, has_top_level_mod, live, shape, without_tests, wrap, Options, Shape, MESSAGE,
};

/// Identity used for the wrapped source in diagnostics.
pub const SOURCE_NAME: &str = "play.rs";
