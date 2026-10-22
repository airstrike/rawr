// Copyright the gitoxide contributors. MIT OR Apache-2.0.
//! gix-validate 0.11.2 complete reference-name validation, without sanitization.

use rawr_toml::document;

use crate::input::membership::manifest::{session::Session, Step};

#[derive(Debug, Eq, PartialEq, thiserror::Error)]
pub(in super::super) enum Failure {
    #[error("reference name contains invalid byte {byte:?}")]
    InvalidByte { byte: Vec<u8> },
    #[error("reference name starts with slash")]
    StartsWithSlash,
    #[error("reference name contains repeated slash")]
    RepeatedSlash,
    #[error("reference name contains repeated dot")]
    RepeatedDot,
    #[error("reference name has lock-file suffix")]
    LockFileSuffix,
    #[error("reference name contains a reflog portion")]
    ReflogPortion,
    #[error("reference name contains an asterisk")]
    Asterisk,
    #[error("reference name starts with dot")]
    StartsWithDot,
    #[error("reference name ends with dot")]
    EndsWithDot,
    #[error("reference name ends with slash")]
    EndsWithSlash,
    #[error("reference name is empty")]
    Empty,
    #[error("standalone reference name is not uppercase or underscore")]
    SomeLowercase,
}

pub(super) fn validate<A: document::Admission>(
    input: &[u8],
    session: &mut Session<'_, A>,
) -> Step<Result<(), Failure>> {
    session.check()?;
    if let Err(error) = tag(input, session)? {
        return Ok(Err(error));
    }
    // bstr::find_byte dispatches to the same exact memchr2.8.2 dependency
    // already contained for the pinned ARM host's matcher searches.
    session.items(1)?;
    let saw_slash = memchr::memchr(b'/', input).is_some();
    if !saw_slash {
        for byte in input {
            session.items(1)?;
            if !byte.is_ascii_uppercase() && *byte != b'_' {
                return failure(Failure::SomeLowercase, session);
            }
        }
    }
    Ok(Ok(()))
}

fn tag<A: document::Admission>(
    input: &[u8],
    session: &mut Session<'_, A>,
) -> Step<Result<(), Failure>> {
    session.items(1)?;
    if input.is_empty() {
        return failure(Failure::Empty, session);
    }
    if input.last() == Some(&b'/') {
        return failure(Failure::EndsWithSlash, session);
    }
    if input.first() == Some(&b'/') {
        return failure(Failure::StartsWithSlash, session);
    }

    let mut previous = 0;
    let mut component_end = 0;
    let last = input.len() - 1;
    for (byte_pos, &byte) in input.iter().enumerate() {
        session.items(1)?;
        match byte {
            b'\\' | b'^' | b':' | b'[' | b'?' | b' ' | b'~' | b'\0'..=b'\x1F' | b'\x7F' => {
                let byte = super::copy(&[byte], session)?;
                return failure(Failure::InvalidByte { byte }, session);
            }
            b'*' => return failure(Failure::Asterisk, session),
            b'.' if previous == b'.' => return failure(Failure::RepeatedDot, session),
            b'.' if previous == b'/' => return failure(Failure::StartsWithDot, session),
            b'{' if previous == b'@' => return failure(Failure::ReflogPortion, session),
            b'/' if previous == b'/' => return failure(Failure::RepeatedSlash, session),
            other => {
                if other == b'/' {
                    let component_start = component_end;
                    component_end = byte_pos;
                    if input[component_start..component_end].ends_with(b".lock") {
                        return failure(Failure::LockFileSuffix, session);
                    }
                }
                if byte_pos == last && input[component_end + 1..].ends_with(b".lock") {
                    return failure(Failure::LockFileSuffix, session);
                }
            }
        }
        previous = byte;
    }
    if input[0] == b'.' {
        return failure(Failure::StartsWithDot, session);
    }
    if input[last] == b'.' {
        return failure(Failure::EndsWithDot, session);
    }
    Ok(Ok(()))
}

/// PartialNameRef validates the same tag grammar without the complete-name suffix check.
pub(super) fn partial<A: document::Admission>(
    input: &[u8],
    session: &mut Session<'_, A>,
) -> Step<Result<(), Failure>> {
    session.check()?;
    tag(input, session)
}

fn failure<A: document::Admission>(
    error: Failure,
    session: &mut Session<'_, A>,
) -> Step<Result<(), Failure>> {
    session.items(1)?;
    Ok(Err(error))
}
