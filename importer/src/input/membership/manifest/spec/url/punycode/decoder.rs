// Copyright 2013 The rust-url developers. MIT OR Apache-2.0.
//! Reusable admitted insertion storage for the pinned internal decoder.

use rawr_toml::document;

use crate::input::membership::manifest::{session, storage, Step};

use super::{
    adapt, decode, input, invalid, threshold, Invalid, BASE, DECODE_MAX_INPUT_LENGTH, INITIAL_BIAS,
    INITIAL_N,
};

#[derive(Debug)]
pub(super) struct Insertion {
    pub(super) position: usize,
    pub(super) c: char,
}

/// Reusable flat scratch; clearing never refunds an original account charge.
#[derive(Debug)]
pub(in crate::input::membership::manifest::spec::url) struct Decoder {
    insertions: storage::Buffer<Insertion>,
}

impl Decoder {
    pub(in crate::input::membership::manifest::spec::url) const fn new() -> Self {
        Self {
            insertions: storage::Buffer::new(),
        }
    }

    /// Decode byte code units, lowercasing the unchanged base as IDNA does.
    pub(in crate::input::membership::manifest::spec::url) fn bytes<
        'scratch,
        'input,
        A: document::Admission,
    >(
        &'scratch mut self,
        input: &'input [u8],
        session: &mut session::Session<'_, A>,
    ) -> Step<Result<decode::Decode<'scratch, 'input>, Invalid>> {
        self.decode(input::Input::Bytes(input), session)
    }

    /// Decode scalar code units without lowercasing or accepting uppercase digits.
    pub(in crate::input::membership::manifest::spec::url) fn chars<
        'scratch,
        'input,
        A: document::Admission,
    >(
        &'scratch mut self,
        input: &'input [char],
        session: &mut session::Session<'_, A>,
    ) -> Step<Result<decode::Decode<'scratch, 'input>, Invalid>> {
        self.decode(input::Input::Chars(input), session)
    }

    fn decode<'scratch, 'input, A: document::Admission>(
        &'scratch mut self,
        input: input::Input<'input>,
        session: &mut session::Session<'_, A>,
    ) -> Step<Result<decode::Decode<'scratch, 'input>, Invalid>> {
        session.check()?;
        self.insertions.clear();
        if input.len() > DECODE_MAX_INPUT_LENGTH {
            return invalid(Invalid::Length, session);
        }
        let (base, mut input) = if let Some(position) = input.delimiter() {
            let (base, tail) = input.split(position);
            (base, if position > 0 { tail.split(1).1 } else { input })
        } else {
            input.split(0)
        };

        let mut length = base.len() as u32;
        let mut code_point = INITIAL_N;
        let mut bias = INITIAL_BIAS;
        let mut i = 0u32;
        loop {
            let previous_i = i;
            let mut weight = 1u32;
            let mut k = BASE;
            let Some(mut digit) = input.digit() else {
                break;
            };
            loop {
                let Some(value) = digit else {
                    return invalid(Invalid::Digit, session);
                };
                let Some(product) = value.checked_mul(weight) else {
                    return invalid(Invalid::Overflow, session);
                };
                let Some(next) = i.checked_add(product) else {
                    return invalid(Invalid::Overflow, session);
                };
                i = next;
                let t = threshold(k, bias);
                if value < t {
                    break;
                }
                let Some(next) = weight.checked_mul(BASE - t) else {
                    return invalid(Invalid::Overflow, session);
                };
                weight = next;
                k += BASE;
                let Some(next) = input.digit() else {
                    return invalid(Invalid::Digit, session);
                };
                digit = next;
            }

            bias = adapt(i - previous_i, length + 1, previous_i == 0);
            let Some(next) = code_point.checked_add(i / (length + 1)) else {
                return invalid(Invalid::Overflow, session);
            };
            code_point = next;
            i %= length + 1;
            let Some(c) = char::from_u32(code_point) else {
                return invalid(Invalid::Scalar, session);
            };
            for insertion in self.insertions.as_mut_slice() {
                if insertion.position >= i as usize {
                    insertion.position += 1;
                }
            }
            self.insertions.push(
                Insertion {
                    position: i as usize,
                    c,
                },
                session,
            )?;
            length += 1;
            i += 1;
        }

        // Shifting every old position >= i before inserting at i preserves
        // uniqueness. Stable-sort tie behavior is therefore unreachable.
        storage::sort(self.insertions.as_mut_slice(), |left, right| {
            left.position.cmp(&right.position)
        });
        Ok(Ok(decode::Decode::assembled(
            base,
            self.insertions.as_slice(),
        )))
    }
}
