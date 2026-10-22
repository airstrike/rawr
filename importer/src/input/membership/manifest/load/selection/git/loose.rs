// Copyright the gitoxide contributors. MIT OR Apache-2.0.
//! The pinned loose-reference decode reached by repository discovery.
//! Closed file replay feeds this decoder; store/discovery ownership is separate.

use std::alloc::Layout;

use rawr_toml::document;

use crate::input::membership::manifest::{session::Session, Error, Step};

mod hash;
mod name;
pub(super) mod read;
pub(super) mod store;

#[cfg(test)]
mod tests;

/// Exactly the full names constructed by find_loose("HEAD"), in source order.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Candidate {
    Head,
    Ref,
    Tag,
    Branch,
    Remote,
}

impl Candidate {
    pub(super) const ALL: [Self; 5] =
        [Self::Head, Self::Ref, Self::Tag, Self::Branch, Self::Remote];

    pub(super) fn bytes(self) -> &'static [u8] {
        match self {
            Self::Head => b"HEAD",
            Self::Ref => b"refs/HEAD",
            Self::Tag => b"refs/tags/HEAD",
            Self::Branch => b"refs/heads/HEAD",
            Self::Remote => b"refs/remotes/HEAD",
        }
    }

    pub(super) fn name(self) -> Name<'static> {
        Name {
            candidate: self,
            bytes: self.bytes(),
        }
    }
}

/// A name borrowed from the source's reusable candidate buffer.
pub(super) struct Name<'buffer> {
    candidate: Candidate,
    bytes: &'buffer [u8],
}

#[derive(Debug, Eq, PartialEq)]
pub(super) enum Target {
    Object([u8; 20]),
    Symbolic(Vec<u8>),
}

/// Owned successful decode, with no repository or file freshness authority.
#[derive(Debug, Eq, PartialEq)]
pub(super) struct Reference {
    pub(super) name: Vec<u8>,
    pub(super) target: Target,
}

#[derive(Debug, Eq, PartialEq, thiserror::Error)]
pub(super) enum Failure {
    #[error("loose reference content could not be parsed: {content:?}")]
    Parse { content: Vec<u8> },
    #[error("invalid symbolic reference {path:?}: {source}")]
    RefnameValidation {
        source: name::Failure,
        path: Vec<u8>,
    },
}

/// The caller has completed this candidate's read. Copy its known full name
/// before parsing, preserving try_from_path's partial-construction lifetime.
pub(super) fn decode<A: document::Admission>(
    candidate: Candidate,
    contents: &[u8],
    session: &mut Session<'_, A>,
) -> Step<Result<Reference, Failure>> {
    let name = copy(candidate.bytes(), session)?;
    let target = if let Some(mut rest) = contents.strip_prefix(b"ref: ") {
        while rest.first() == Some(&b' ') {
            session.items(1)?;
            rest = &rest[1..];
        }
        let mut end = 0;
        for &byte in rest {
            session.items(1)?;
            if matches!(byte, 0 | b'\r' | b'\n') {
                break;
            }
            end += 1;
        }
        // Upstream first owns the target, then validates that owned value.
        let path = copy(&rest[..end], session)?;
        if let Err(source) = name::validate(&path, session)? {
            session.items(1)?;
            return Ok(Err(Failure::RefnameValidation { source, path }));
        }
        Target::Symbolic(path)
    } else {
        let Some(hex) = contents.get(..40) else {
            return parse_failure(contents, session);
        };
        // gix_ref::parse::hex_hash validates the first fixed hash width, then
        // the caller rejects a further immediately adjacent hexadecimal byte.
        session.items(1)?;
        if !hex.iter().all(u8::is_ascii_hexdigit)
            || contents.get(40).is_some_and(u8::is_ascii_hexdigit)
        {
            return parse_failure(contents, session);
        }
        Target::Object(hash::decode(hex, session)?)
    };
    session.items(1)?;
    Ok(Ok(Reference { name, target }))
}

fn parse_failure<A: document::Admission>(
    contents: &[u8],
    session: &mut Session<'_, A>,
) -> Step<Result<Reference, Failure>> {
    let content = copy(contents, session)?;
    session.items(1)?;
    Ok(Err(Failure::Parse { content }))
}

/// BString from a borrowed byte slice owns one exact byte copy. The same helper
/// covers the one-byte InvalidByte payload, without replacing it by a scalar.
fn copy<A: document::Admission>(bytes: &[u8], session: &mut Session<'_, A>) -> Step<Vec<u8>> {
    session.string(bytes.len())?;
    session.items(1)?;
    let layout = match Layout::array::<u8>(bytes.len()) {
        Ok(layout) => layout,
        Err(_) => return session.fail(Error::Overflow),
    };
    if !bytes.is_empty() {
        session.admit(document::Request::Allocation {
            bytes: layout.size(),
            alignment: layout.align(),
        })?;
    }
    let mut owned = Vec::new();
    if let Err(error) = owned.try_reserve_exact(bytes.len()) {
        return session.fail(Error::Allocation(error));
    }
    owned.extend_from_slice(bytes);
    Ok(owned)
}
