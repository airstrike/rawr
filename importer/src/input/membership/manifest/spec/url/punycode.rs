// Copyright 2013 The rust-url developers.
//
// Licensed under the Apache License, Version 2.0 or the MIT license, at your
// option. See punycode/LICENSE-APACHE and punycode/LICENSE-MIT.

//! Contained internal Punycode transformation with the pinned UTS46 work guards.
//!
//! Results carry no IDNA, DNS, URL, host, or package-identity authority.

mod decode;
mod decoder;
mod encode;
mod input;

#[cfg(test)]
mod tests;

use rawr_toml::document;

use crate::input::membership::manifest::{session, Step};

pub(super) use decoder::Decoder;
pub(super) use encode::encode;

/// Pinned semantic failure, separate from the original account's terminal stop.
#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub(super) enum Invalid {
    #[error("Punycode input exceeds its UTS46 work guard")]
    Length,
    #[error("Punycode contains an invalid or incomplete digit sequence")]
    Digit,
    #[error("Punycode arithmetic overflowed")]
    Overflow,
    #[error("Punycode decoded an invalid Unicode scalar")]
    Scalar,
}

const DECODE_MAX_INPUT_LENGTH: usize = 2000;
const ENCODE_MAX_INPUT_LENGTH: usize = 1000;
const BASE: u32 = 36;
const T_MIN: u32 = 1;
const T_MAX: u32 = 26;
const SKEW: u32 = 38;
const DAMP: u32 = 700;
const INITIAL_BIAS: u32 = 72;
const INITIAL_N: u32 = 0x80;

fn invalid<T, A: document::Admission>(
    error: Invalid,
    session: &mut session::Session<'_, A>,
) -> Step<Result<T, Invalid>> {
    session.items(1)?;
    Ok(Err(error))
}

fn adapt(mut delta: u32, num_points: u32, first_time: bool) -> u32 {
    delta /= if first_time { DAMP } else { 2 };
    delta += delta / num_points;
    let mut k = 0;
    while delta > ((BASE - T_MIN) * T_MAX) / 2 {
        delta /= BASE - T_MIN;
        k += BASE;
    }
    k + (((BASE - T_MIN + 1) * delta) / (delta + SKEW))
}

fn threshold(k: u32, bias: u32) -> u32 {
    if k <= bias {
        T_MIN
    } else if k >= bias + T_MAX {
        T_MAX
    } else {
        k - bias
    }
}
