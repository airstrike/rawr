// Copyright 2013-2016 The rust-url developers. MIT OR Apache-2.0.
// See host/LICENSE-MIT and host/LICENSE-APACHE.

//! Complete pinned special and opaque host transformations over borrowed input.

mod percent;
mod special;

#[cfg(test)]
mod tests;

use rawr_toml::document;

use super::{idna, ip};
use crate::input::membership::manifest::{session, Step};

/// Converted host data. URL syntax, schemes and package identity are separate.
pub(super) struct Host<'input> {
    kind: Kind<'input>,
}

pub(super) enum Kind<'input> {
    Domain(Domain<'input>),
    Ipv4(ip::v4::Address),
    Ipv6(ip::v6::Address),
}

/// Original or transformed ASCII domain bytes, retaining their actual owner.
pub(super) struct Domain<'input> {
    bytes: idna::Bytes<'input>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub(super) enum Invalid {
    #[error("empty host")]
    Empty,
    #[error("invalid IDNA domain")]
    Idna,
    #[error("invalid IPv4 address")]
    Ipv4,
    #[error("invalid IPv6 address")]
    Ipv6,
    #[error("invalid opaque host character")]
    Character,
}

impl<'input> Host<'input> {
    pub(super) fn special<A: document::Admission>(
        input: &'input str,
        session: &mut session::Session<'_, A>,
    ) -> Step<Result<Self, Invalid>> {
        session.string(input.len())?;
        if input.starts_with('[') {
            return bracketed(input, session);
        }
        special::parse(input, session)
    }

    pub(super) fn opaque<A: document::Admission>(
        input: &'input str,
        session: &mut session::Session<'_, A>,
    ) -> Step<Result<Self, Invalid>> {
        session.string(input.len())?;
        if input.starts_with('[') {
            return bracketed(input, session);
        }
        // All forbidden characters are ASCII; scanning UTF-8 bytes preserves
        // the pinned whole-input character check before encoding owns output.
        if input.bytes().any(forbidden) {
            return invalid(Invalid::Character, session);
        }
        let bytes = percent::encode(input.as_bytes(), session)?;
        domain(bytes, session)
    }

    pub(super) fn kind(&self) -> &Kind<'input> {
        &self.kind
    }
}

impl Domain<'_> {
    pub(super) fn as_bytes(&self) -> &[u8] {
        self.bytes.as_slice()
    }
    pub(super) fn is_borrowed(&self) -> bool {
        matches!(self.bytes, idna::Bytes::Borrowed(_))
    }
}

fn bracketed<'input, A: document::Admission>(
    input: &str,
    session: &mut session::Session<'_, A>,
) -> Step<Result<Host<'input>, Invalid>> {
    if !input.ends_with(']') {
        return invalid(Invalid::Ipv6, session);
    }
    // A recognized leading '[' and final ']' are ASCII boundaries. Their
    // interior reaches the IP parser directly, without percent/IDNA processing.
    Ok(ip::v6::Address::parse(&input[1..input.len() - 1], session)?
        .map(|value| Host {
            kind: Kind::Ipv6(value),
        })
        .map_err(|_| Invalid::Ipv6))
}

fn domain<'input, A: document::Admission>(
    bytes: idna::Bytes<'input>,
    session: &mut session::Session<'_, A>,
) -> Step<Result<Host<'input>, Invalid>> {
    session.items(1)?;
    Ok(Ok(Host {
        kind: Kind::Domain(Domain { bytes }),
    }))
}

fn invalid<T, A: document::Admission>(
    error: Invalid,
    session: &mut session::Session<'_, A>,
) -> Step<Result<T, Invalid>> {
    session.items(1)?;
    Ok(Err(error))
}

fn forbidden(byte: u8) -> bool {
    matches!(
        byte,
        0 | b'\t'
            | b'\n'
            | b'\r'
            | b' '
            | b'#'
            | b'/'
            | b':'
            | b'<'
            | b'>'
            | b'?'
            | b'@'
            | b'['
            | b'\\'
            | b']'
            | b'^'
            | b'|'
    )
}
