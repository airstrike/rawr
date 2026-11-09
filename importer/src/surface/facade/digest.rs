//! A plan's 32-byte identities, written as 64 hex digits (#1360): a JSON
//! array of 32 decimal numbers is about twice the bytes, and slower to read.

use serde::{Deserializer, Serializer};

const DIGITS: &[u8; 16] = b"0123456789abcdef";

pub(super) fn serialize<S: Serializer>(bytes: &[u8; 32], serializer: S) -> Result<S::Ok, S::Error> {
    let mut text = [0_u8; 64];
    for (index, byte) in bytes.iter().enumerate() {
        text[2 * index] = DIGITS[usize::from(byte >> 4)];
        text[2 * index + 1] = DIGITS[usize::from(byte & 0x0f)];
    }
    let text = std::str::from_utf8(&text).map_err(serde::ser::Error::custom)?;
    serializer.serialize_str(text)
}

pub(super) fn deserialize<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<[u8; 32], D::Error> {
    deserializer.deserialize_str(Digits)
}

/// Reads 64 lowercase hex digits, borrowed from the plan or not.
struct Digits;

impl serde::de::Visitor<'_> for Digits {
    type Value = [u8; 32];

    fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("an identity as 64 lowercase hex digits")
    }

    fn visit_str<E: serde::de::Error>(self, text: &str) -> Result<[u8; 32], E> {
        let text = text.as_bytes();
        if text.len() != 64 {
            return Err(E::invalid_length(text.len(), &self));
        }
        let digit = |byte: u8| match byte {
            b'0'..=b'9' => Ok(byte - b'0'),
            b'a'..=b'f' => Ok(byte - b'a' + 10),
            _ => Err(E::invalid_value(
                serde::de::Unexpected::Char(char::from(byte)),
                &self,
            )),
        };
        let mut bytes = [0_u8; 32];
        for (index, byte) in bytes.iter_mut().enumerate() {
            *byte = digit(text[2 * index])? << 4 | digit(text[2 * index + 1])?;
        }
        Ok(bytes)
    }
}
