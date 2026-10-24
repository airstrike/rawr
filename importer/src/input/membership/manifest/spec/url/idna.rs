// Copyright The rust-url developers. MIT OR Apache-2.0.
//! Pinned UTS46 at the URL caller's complete, fixed domain-to-ASCII entry.

#[cfg(feature = "unicode-hosts")]
mod ascii;
#[cfg(feature = "unicode-hosts")]
mod buffer;
#[cfg(feature = "unicode-hosts")]
mod domain;
#[cfg(feature = "unicode-hosts")]
mod normalize;
mod output;
#[cfg(feature = "unicode-hosts")]
mod properties;

#[cfg(test)]
mod tests;

use rawr_toml::document;

use crate::input::membership::manifest::{session, Step, Stop};

pub(super) use output::Bytes;

/// Complete ASCII domain data; URL host/IPv4 interpretation is separate.
pub(super) struct Ascii<'input> {
    bytes: output::Bytes<'input>,
}

/// The selected fail-fast entry exposes one semantic validity failure.
#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
#[error("invalid UTS46 domain")]
pub(super) struct Invalid;

enum Failure {
    Invalid(Invalid),
    Stopped(Stop),
}

type Work<T> = Result<T, Failure>;

impl From<Stop> for Failure {
    fn from(stop: Stop) -> Self {
        Self::Stopped(stop)
    }
}

impl<'input> Ascii<'input> {
    pub(super) fn interpret<A: document::Admission>(
        input: &'input [u8],
        session: &mut session::Session<'_, A>,
    ) -> Step<Result<Self, Invalid>> {
        session.check()?;
        session.string(input.len())?;
        #[cfg(not(feature = "unicode-hosts"))]
        {
            // A project's runner reads no manifest; without UTS46 data only
            // a host that is already lowercase ASCII is one.
            if input
                .iter()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'.' | b'-'))
            {
                return Ok(Ok(Self {
                    bytes: output::Bytes::Borrowed(input),
                }));
            }
            return Ok(Err(Invalid));
        }
        #[cfg(feature = "unicode-hosts")]
        match domain::Domain::interpret(input, session).and_then(|domain| domain.output(session)) {
            Ok(bytes) => Ok(Ok(Self { bytes })),
            Err(Failure::Invalid(error)) => Ok(Err(error)),
            Err(Failure::Stopped(stop)) => Err(stop),
        }
    }

    pub(super) fn as_bytes(&self) -> &[u8] {
        self.bytes.as_slice()
    }
    pub(super) fn is_borrowed(&self) -> bool {
        matches!(self.bytes, output::Bytes::Borrowed(_))
    }
    pub(super) fn into_bytes(self) -> Bytes<'input> {
        self.bytes
    }
}

fn invalid<T, A: document::Admission>(session: &mut session::Session<'_, A>) -> Work<T> {
    session.items(1)?;
    Err(Failure::Invalid(Invalid))
}
