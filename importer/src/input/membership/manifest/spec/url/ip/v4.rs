// Copyright 2013-2016 The rust-url developers. MIT OR Apache-2.0.

//! URL's radix-aware IPv4 parser and separate ends-in-number dispatch.

use rawr_toml::document;

use super::{Invalid, Text};
use crate::input::membership::manifest::{session, Step};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub(in super::super) struct Address {
    octets: [u8; 4],
}

impl Address {
    pub(in super::super) fn parse<A: document::Admission>(
        input: &str,
        session: &mut session::Session<'_, A>,
    ) -> Step<Result<Self, Invalid>> {
        session.string(input.len())?;
        super::finish(parse(input), session)
    }

    pub(in super::super) fn octets(&self) -> &[u8; 4] {
        &self.octets
    }

    pub(in super::super) fn serialize<A: document::Admission>(
        &self,
        session: &mut session::Session<'_, A>,
    ) -> Step<Text> {
        Text::render(
            |emit| {
                for (index, byte) in self.octets.iter().copied().enumerate() {
                    if index != 0 {
                        emit(b'.');
                    }
                    if byte >= 100 {
                        emit(b'0' + byte / 100);
                    }
                    if byte >= 10 {
                        emit(b'0' + (byte / 10) % 10);
                    }
                    emit(b'0' + byte % 10);
                }
            },
            session,
        )
    }
}

/// Routing after the caller's special-host normalization, not host validation.
pub(in super::super) fn candidate<A: document::Admission>(
    input: &str,
    session: &mut session::Session<'_, A>,
) -> Step<bool> {
    session.string(input.len())?;
    let mut parts = input.rsplit('.');
    let Some(last) = parts.next() else {
        return Ok(false);
    };
    let last = if last.is_empty() {
        let Some(last) = parts.next() else {
            return Ok(false);
        };
        last
    } else {
        last
    };
    if !last.is_empty() && last.bytes().all(|byte| byte.is_ascii_digit()) {
        return Ok(true);
    }
    Ok(number(last).is_ok())
}

/// A syntactically valid number can exceed u32 without ceasing to be numeric.
fn number(mut input: &str) -> Result<Option<u32>, ()> {
    if input.is_empty() {
        return Err(());
    }
    let radix = if input.starts_with("0x") || input.starts_with("0X") {
        input = &input[2..];
        16
    } else if input.len() >= 2 && input.starts_with('0') {
        input = &input[1..];
        8
    } else {
        10
    };
    if input.is_empty() {
        return Ok(Some(0));
    }
    // Syntax wins over overflow, as in the pinned full validation pass followed
    // by from_str_radix. Continue checking digits after the accumulator fills.
    let mut value = Some(0u32);
    for byte in input.bytes() {
        let digit = match byte {
            b'0'..=b'9' => u32::from(byte - b'0'),
            b'a'..=b'f' => u32::from(byte - b'a') + 10,
            b'A'..=b'F' => u32::from(byte - b'A') + 10,
            _ => return Err(()),
        };
        if digit >= radix {
            return Err(());
        }
        value = value
            .and_then(|value| value.checked_mul(radix))
            .and_then(|value| value.checked_add(digit));
    }
    Ok(value)
}

fn parse(input: &str) -> Result<Address, Invalid> {
    // Remove exactly one trailing empty part before counting. No part is
    // converted until the original parser's entire cardinality check succeeds.
    let input = input.strip_suffix('.').unwrap_or(input);
    let count = input.split('.').count();
    if count > 4 {
        return Err(Invalid::V4);
    }
    let mut numbers = [0u32; 4];
    for (index, part) in input.split('.').enumerate() {
        numbers[index] = match number(part) {
            Ok(Some(number)) => number,
            _ => return Err(Invalid::V4),
        };
    }
    // split always has one part, including the rejected empty-string case.
    let prefix = count - 1;
    let mut address = numbers[prefix];
    if address > u32::MAX >> (8 * prefix) {
        return Err(Invalid::V4);
    }
    if numbers[..prefix].iter().any(|number| *number > 255) {
        return Err(Invalid::V4);
    }
    for (index, number) in numbers[..prefix].iter().copied().enumerate() {
        address += number << (8 * (3 - index));
    }
    Ok(Address {
        octets: address.to_be_bytes(),
    })
}
