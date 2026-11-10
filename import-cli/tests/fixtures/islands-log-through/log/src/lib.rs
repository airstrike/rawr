//! inboard's `log` crate: it re-exports tracing's macros from a private
//! module and sets a subscriber, and it reads a time as inboard's
//! `common::time` does.
mod logger;

pub use logger::{init, warn};

/// A time read from text, as inboard's `Timestamp::from_rfc3339` reads one
/// and fails with chrono's `ParseError`.
pub mod time {
    use std::fmt;

    #[derive(Debug, Clone, Copy)]
    pub struct Timestamp(pub u64);

    #[derive(Debug)]
    pub struct ParseError;

    impl fmt::Display for ParseError {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str("input contains invalid characters")
        }
    }

    impl Timestamp {
        pub fn current() -> Timestamp {
            Timestamp(0)
        }

        pub fn from_rfc3339(text: &str) -> Result<Timestamp, ParseError> {
            text.parse().map(Timestamp).map_err(|_| ParseError)
        }
    }
}
