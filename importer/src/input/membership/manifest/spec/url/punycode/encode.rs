// Copyright 2013 The rust-url developers. MIT OR Apache-2.0.
//! Pinned internal encoder with prospectively admitted ASCII output.

use rawr_toml::document;

use crate::input::membership::manifest::{session, storage, Error, Step};

use super::{
    adapt, invalid, threshold, Invalid, BASE, ENCODE_MAX_INPUT_LENGTH, INITIAL_BIAS, INITIAL_N,
};

/// Complete ASCII Punycode output, excluding the caller-owned `xn--` prefix.
#[derive(Debug)]
pub(in crate::input::membership::manifest::spec::url) struct Encoded {
    bytes: storage::Buffer<u8>,
}

impl Encoded {
    pub(in crate::input::membership::manifest::spec::url) fn as_bytes(&self) -> &[u8] {
        self.bytes.as_slice()
    }

    fn push<A: document::Admission>(
        &mut self,
        byte: u8,
        session: &mut session::Session<'_, A>,
    ) -> Step<()> {
        let Some(length) = self.bytes.len().checked_add(1) else {
            return session.fail(Error::Overflow);
        };
        session.string(length)?;
        self.bytes.push(byte, session)?;
        Ok(())
    }
}

/// Encode using IDNA's internal arithmetic and its non-ASCII input work guard.
pub(in crate::input::membership::manifest::spec::url) fn encode<A: document::Admission>(
    input: &[char],
    session: &mut session::Session<'_, A>,
) -> Step<Result<Encoded, Invalid>> {
    session.check()?;
    if input.len() > ENCODE_MAX_INPUT_LENGTH && input.iter().any(|c| !c.is_ascii()) {
        return invalid(Invalid::Length, session);
    }
    let mut output = Encoded {
        bytes: storage::Buffer::new(),
    };
    let (mut input_length, mut basic_length) = (0u32, 0);
    for &c in input {
        let Some(next) = input_length.checked_add(1) else {
            return invalid(Invalid::Overflow, session);
        };
        input_length = next;
        if c.is_ascii() {
            output.push(c as u8, session)?;
            basic_length += 1;
        }
    }
    let Some(length) = input_length.checked_add(1) else {
        return invalid(Invalid::Overflow, session);
    };
    if length
        .checked_mul(u32::from(char::MAX) - INITIAL_N)
        .is_none()
    {
        return invalid(Invalid::Overflow, session);
    }
    if basic_length > 0 {
        output.push(b'-', session)?;
    }
    let mut code_point = INITIAL_N;
    let mut delta = 0u32;
    let mut bias = INITIAL_BIAS;
    let mut processed = basic_length;
    while processed < input_length {
        let Some(min_code_point) = input
            .iter()
            .map(|c| u32::from(*c))
            .filter(|c| *c >= code_point)
            .min()
        else {
            return session.fail(Error::State);
        };
        delta += (min_code_point - code_point) * (processed + 1);
        code_point = min_code_point;
        for &c in input {
            let c = u32::from(c);
            if c < code_point {
                delta += 1;
            }
            if c == code_point {
                let mut q = delta;
                let mut k = BASE;
                loop {
                    let t = threshold(k, bias);
                    if q < t {
                        break;
                    }
                    let value = t + ((q - t) % (BASE - t));
                    output.push(digit(value), session)?;
                    q = (q - t) / (BASE - t);
                    k += BASE;
                }
                output.push(digit(q), session)?;
                bias = adapt(delta, processed + 1, processed == basic_length);
                delta = 0;
                processed += 1;
            }
        }
        delta += 1;
        code_point += 1;
    }
    Ok(Ok(output))
}

fn digit(value: u32) -> u8 {
    // q < t <= 26 on the final digit, and t + remainder < BASE otherwise.
    if value < 26 {
        value as u8 + b'a'
    } else {
        value as u8 - 26 + b'0'
    }
}
