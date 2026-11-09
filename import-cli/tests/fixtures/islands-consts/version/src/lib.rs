//! A crate whose only public item is a `&str` constant, as #1146 found.
pub const VALUE: &str = env!("CARGO_PKG_VERSION");
