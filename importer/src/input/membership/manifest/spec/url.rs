//! Complete pinned absolute URL interpretation without host or source authority.

mod host;
mod idna;
mod ip;
mod parser;
mod punycode;

pub(in crate::input::membership::manifest) use parser::canonical::Canonical;
pub(in crate::input::membership::manifest) use parser::{Accepted, BaseError};

pub(in crate::input::membership::manifest) use parser::{Invalid, Url};

// Compile the complete unchanged source as a test reference, including its
// internal caller API. Its deliberately slow upstream test remains ignored.
#[cfg(test)]
#[allow(
    dead_code,
    clippy::manual_repeat_n,
    reason = "pinned verbatim upstream source; tests use part of it"
)]
#[path = "../../../../../reference/punycode/punycode.rs"]
mod reference;
