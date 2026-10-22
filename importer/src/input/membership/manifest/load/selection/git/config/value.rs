// Copyright the gitoxide contributors. MIT OR Apache-2.0.
//! Pinned value normalization and Boolean/Integer conversion ownership.

use super::{own, storage, Session, Step};
use rawr_toml::document::Admission;
use std::borrow::Cow;

#[derive(Debug)]
pub(super) struct Failure {
    pub(super) message: &'static str,
    pub(super) input: Vec<u8>,
    pub(super) utf8: Option<std::str::Utf8Error>,
}

const BOOL: &str =
    "Booleans need to be 'no', 'off', 'false', '' or 'yes', 'on', 'true' or any number";
const INT: &str =
    "Integers needs to be positive or negative numbers which may have a suffix like 1k, 42, or 50G";

pub(super) fn boolean<A: Admission>(
    input: &[u8],
    session: &mut Session<'_, A>,
) -> Step<Result<bool, Failure>> {
    for value in [b"yes".as_slice(), b"on", b"true"] {
        session.items(input.len().max(value.len()))?;
        if input.eq_ignore_ascii_case(value) {
            return Ok(Ok(true));
        }
    }
    for value in [b"no".as_slice(), b"off", b"false"] {
        session.items(input.len().max(value.len()))?;
        if input.eq_ignore_ascii_case(value) {
            return Ok(Ok(false));
        }
    }
    if input.is_empty() {
        return Ok(Ok(false));
    }
    session.items(input.len())?;
    if let Ok(text) = std::str::from_utf8(input) {
        session.items(text.len())?;
        if let Ok(value) = text.parse::<i64>() {
            return Ok(Ok(value != 0));
        }
    }
    Ok(Err(Failure {
        message: BOOL,
        input: own(input, session)?,
        utf8: None,
    }))
}

pub(super) fn integer<A: Admission>(
    input: Cow<'_, [u8]>,
    session: &mut Session<'_, A>,
) -> Step<Result<i64, Failure>> {
    session.items(input.len())?;
    let text = match std::str::from_utf8(&input) {
        Ok(text) => text,
        Err(error) => {
            return Ok(Err(Failure {
                message: INT,
                input: own(&input, session)?,
                utf8: Some(error),
            }))
        }
    };
    session.items(text.len())?;
    if let Ok(value) = text.parse::<i64>() {
        return Ok(Ok(value));
    }
    if text.len() > 1 && text.is_char_boundary(text.len() - 1) {
        let (number, suffix) = text.split_at(text.len() - 1);
        session.items(number.len())?;
        let number = number.parse::<i64>();
        let multiplier = match suffix {
            "k" | "K" => Some(1024),
            "m" | "M" => Some(1024 * 1024),
            "g" | "G" => Some(1024 * 1024 * 1024),
            _ => None,
        };
        if let (Ok(number), Some(multiplier)) = (number, multiplier) {
            if let Some(value) = number.checked_mul(multiplier) {
                return Ok(Ok(value));
            }
            return Ok(Err(Failure {
                message: "Integer overflow",
                input: into_owned(input, session)?,
                utf8: None,
            }));
        }
    }
    Ok(Err(Failure {
        message: INT,
        input: own(&input, session)?,
        utf8: None,
    }))
}

pub(super) fn into_owned<A: Admission>(
    input: Cow<'_, [u8]>,
    session: &mut Session<'_, A>,
) -> Step<Vec<u8>> {
    match input {
        Cow::Borrowed(bytes) => own(bytes, session),
        Cow::Owned(bytes) => Ok(bytes),
    }
}

pub(super) fn normalize<'a, A: Admission>(
    mut input: Cow<'a, [u8]>,
    session: &mut Session<'_, A>,
) -> Step<Cow<'a, [u8]>> {
    session.items(2)?;
    if input.as_ref() == b"\"\"" {
        return Ok(Cow::Borrowed(b""));
    }
    while input.len() >= 3
        && input[0] == b'"'
        && input[input.len() - 1] == b'"'
        && input[input.len() - 2] != b'\\'
    {
        session.items(3)?;
        match &mut input {
            Cow::Borrowed(bytes) => *bytes = &bytes[1..bytes.len() - 1],
            Cow::Owned(bytes) => {
                session.items(bytes.len())?;
                bytes.pop();
                bytes.remove(0);
            }
        }
        if input.as_ref() == b"\"\"" {
            return Ok(Cow::Borrowed(b""));
        }
    }
    session.items(input.len())?;
    if !input.iter().any(|b| matches!(b, b'\\' | b'"')) {
        return Ok(input);
    }
    let mut output = storage::Vector::with_capacity(input.len(), session)?;
    let mut bytes = input.iter().copied();
    while let Some(byte) = bytes.next() {
        session.items(1)?;
        match byte {
            b'\\' => match bytes.next() {
                Some(b'n') => output.push(b'\n', session)?,
                Some(b't') => output.push(b'\t', session)?,
                Some(b'b') => {
                    output.pop();
                }
                Some(byte) => output.push(byte, session)?,
                None => break,
            },
            b'"' => {}
            _ => output.push(byte, session)?,
        }
    }
    Ok(Cow::Owned(output.into_values()))
}
