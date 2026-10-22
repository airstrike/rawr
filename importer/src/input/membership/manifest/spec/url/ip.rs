// Copyright 2013-2016 The rust-url developers. MIT OR Apache-2.0.
// See ip/LICENSE-MIT and ip/LICENSE-APACHE.

//! Pinned URL IP conversion, with fixed grammar-sized values and text.
//! Parsed addresses do not establish host, URL, source or owner identity.

mod text;
pub(super) mod v4;
pub(super) mod v6;

#[cfg(test)]
mod reference;
#[cfg(test)]
mod tests;

use rawr_toml::document;

use crate::input::membership::manifest::{session, Step};

pub(super) use text::Text;

/// The two failures exposed by the pinned URL IP parsers.
#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub(super) enum Invalid {
    #[error("invalid IPv4 address")]
    V4,
    #[error("invalid IPv6 address")]
    V6,
}

fn finish<T, A: document::Admission>(
    result: Result<T, Invalid>,
    session: &mut session::Session<'_, A>,
) -> Step<Result<T, Invalid>> {
    // A returned scalar or semantic error is one logical retained item.
    // Its inline representation requests no separate heap layout.
    session.items(1)?;
    Ok(result)
}
