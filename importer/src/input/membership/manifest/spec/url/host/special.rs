// Copyright 2013-2016 The rust-url developers. MIT OR Apache-2.0.

//! Special hosts normalize their complete decoded bytes before IPv4 dispatch.

use rawr_toml::document;

use super::super::{idna, ip};
use super::{percent, Host, Invalid, Kind};
use crate::input::membership::manifest::{session, Error, Step};

#[cfg(test)]
mod tests;

pub(super) fn parse<'input, A: document::Admission>(
    input: &'input str,
    session: &mut session::Session<'_, A>,
) -> Step<Result<Host<'input>, Invalid>> {
    let decoded = percent::decode(input.as_bytes(), session)?;
    let bytes = match normalize(decoded, session)? {
        Ok(bytes) => bytes,
        Err(error) => return Ok(Err(error)),
    };
    // Only the complete IDNA output enters this conversion. An invalid decoded
    // UTF-8 prefix cannot bypass IDNA or enter the IPv4 parser directly.
    let text = match std::str::from_utf8(bytes.as_slice()) {
        Ok(text) => text,
        Err(_) => return session.fail(Error::State),
    };
    if text.is_empty() {
        return super::invalid(Invalid::Empty, session);
    }
    if ip::v4::candidate(text, session)? {
        return Ok(ip::v4::Address::parse(text, session)?
            .map(|value| Host {
                kind: Kind::Ipv4(value),
            })
            .map_err(|_| Invalid::Ipv4));
    }
    super::domain(bytes, session)
}

fn normalize<'input, A: document::Admission>(
    input: idna::Bytes<'input>,
    session: &mut session::Session<'_, A>,
) -> Step<Result<idna::Bytes<'input>, Invalid>> {
    match input {
        idna::Bytes::Borrowed(input) => Ok(idna::Ascii::interpret(input, session)?
            .map(idna::Ascii::into_bytes)
            .map_err(|_| Invalid::Idna)),
        idna::Bytes::Owned(input) => {
            let ascii = match idna::Ascii::interpret(input.as_slice(), session)? {
                Ok(ascii) => ascii,
                Err(_) => return Ok(Err(Invalid::Idna)),
            };
            match ascii.into_bytes() {
                idna::Bytes::Owned(output) => Ok(Ok(idna::Bytes::Owned(output))),
                idna::Bytes::Borrowed(output) => {
                    // IDNA's unchanged result always borrows the whole input.
                    // Consume that borrow, then retain the original owner and
                    // admitted capacity; no self-reference or second copy.
                    if !std::ptr::eq(output, input.as_slice()) {
                        return session.fail(Error::State);
                    }
                    Ok(Ok(idna::Bytes::Owned(input)))
                }
            }
        }
    }
}
