// Copyright 2013-2016 The rust-url developers. MIT OR Apache-2.0.

//! Pinned percent decoding and opaque-host CONTROLS encoding without Cow growth.

use rawr_toml::document;

use super::super::idna;
use crate::input::membership::manifest::{session, storage, Error, Step};

#[derive(Clone)]
struct Decode<'input> {
    input: &'input [u8],
}

impl Iterator for Decode<'_> {
    type Item = u8;

    fn next(&mut self) -> Option<u8> {
        let (&first, rest) = self.input.split_first()?;
        self.input = rest;
        if first == b'%'
            && let [high, low, rest @ ..] = rest
            && let (Some(high), Some(low)) = (hex(*high), hex(*low))
        {
            self.input = rest;
            return Some(high * 16 + low);
        }
        Some(first)
    }
}

fn hex(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

pub(super) fn decode<'input, A: document::Admission>(
    input: &'input [u8],
    session: &mut session::Session<'_, A>,
) -> Step<idna::Bytes<'input>> {
    session.check()?;
    let decoded = Decode { input };
    // Every decoded escape reduces length by two, so equality means no valid
    // escape occurred. Invalid/incomplete '%' sequences keep the original borrow.
    let length = decoded.clone().count();
    if length == input.len() {
        return Ok(idna::Bytes::Borrowed(input));
    }
    session.string(length)?;
    let mut output = storage::Buffer::new();
    for byte in decoded {
        output.push(byte, session)?;
    }
    Ok(idna::Bytes::Owned(output))
}

pub(super) fn encode<'input, A: document::Admission>(
    input: &'input [u8],
    session: &mut session::Session<'_, A>,
) -> Step<idna::Bytes<'input>> {
    session.check()?;
    let Some(length) = input.iter().try_fold(0usize, |length, byte| {
        length.checked_add(if encoded(*byte) { 3 } else { 1 })
    }) else {
        return session.fail(Error::Overflow);
    };
    if length == input.len() {
        return Ok(idna::Bytes::Borrowed(input));
    }
    session.string(length)?;
    let mut output = storage::Buffer::new();
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    for &byte in input {
        if encoded(byte) {
            output.push(b'%', session)?;
            output.push(HEX[usize::from(byte >> 4)], session)?;
            output.push(HEX[usize::from(byte & 15)], session)?;
        } else {
            output.push(byte, session)?;
        }
    }
    Ok(idna::Bytes::Owned(output))
}

fn encoded(byte: u8) -> bool {
    // AsciiSet::should_percent_encode includes every non-ASCII UTF-8 byte;
    // CONTROLS adds C0 and DEL while preserving percent signs and '+' literally.
    !(0x20..0x7f).contains(&byte)
}
