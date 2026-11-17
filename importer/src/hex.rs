//! A stored 32-byte digest, written as 64 lowercase hex digits (#1360,
//! #2170). serde writes a `[u8; 32]` as a JSON array of 32 decimal numbers,
//! about 115 bytes and slower to read; the digits are 66 bytes with their
//! quotes. Every stored value, inventory entry and facade plan names its
//! digests through this module: `#[serde(with = "crate::hex")]`, [`option`]
//! for an optional one, or [`Written`] in a hand-written serializer.

use serde::{Deserializer, Serializer};

const DIGITS: &[u8; 16] = b"0123456789abcdef";

pub(crate) fn serialize<S: Serializer>(bytes: &[u8; 32], serializer: S) -> Result<S::Ok, S::Error> {
    let mut text = [0_u8; 64];
    for (index, byte) in bytes.iter().enumerate() {
        text[2 * index] = DIGITS[usize::from(byte >> 4)];
        text[2 * index + 1] = DIGITS[usize::from(byte & 0x0f)];
    }
    let text = std::str::from_utf8(&text).map_err(serde::ser::Error::custom)?;
    serializer.serialize_str(text)
}

pub(crate) fn deserialize<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<[u8; 32], D::Error> {
    deserializer.deserialize_str(Digits)
}

/// A digest a hand-written serializer writes as a field, as the digits.
#[derive(Clone, Copy)]
pub(crate) struct Written<'a>(pub(crate) &'a [u8; 32]);

impl serde::Serialize for Written<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serialize(self.0, serializer)
    }
}

/// An optional digest: `null`, or the digits.
pub(crate) mod option {
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    /// The digest a field holds, borrowed for writing or read back.
    #[derive(Deserialize, Serialize)]
    #[serde(transparent)]
    struct Digest(#[serde(with = "super")] [u8; 32]);

    pub(crate) fn serialize<S: Serializer>(
        bytes: &Option<[u8; 32]>,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        bytes.map(Digest).serialize(serializer)
    }

    pub(crate) fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<Option<[u8; 32]>, D::Error> {
        Ok(Option::<Digest>::deserialize(deserializer)?.map(|Digest(bytes)| bytes))
    }
}

/// Reads 64 lowercase hex digits, borrowed from the value or not.
struct Digits;

impl serde::de::Visitor<'_> for Digits {
    type Value = [u8; 32];

    fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("a digest as 64 lowercase hex digits")
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

#[cfg(test)]
mod tests {
    use serde::{Deserialize, Serialize};

    #[derive(Debug, Deserialize, PartialEq, Serialize)]
    struct Held {
        #[serde(with = "crate::hex")]
        digest: [u8; 32],
        #[serde(with = "crate::hex::option")]
        absent: Option<[u8; 32]>,
        #[serde(with = "crate::hex::option")]
        present: Option<[u8; 32]>,
    }

    #[test]
    fn digests_round_trip_as_hex_digits() {
        let mut digest = [0_u8; 32];
        for (index, byte) in digest.iter_mut().enumerate() {
            *byte = u8::try_from(index * 8 + 7).unwrap();
        }
        let held = Held {
            digest,
            absent: None,
            present: Some([0xab; 32]),
        };
        let text = serde_json::to_string(&held).unwrap();
        let expected = format!(
            r#"{{"digest":"{}","absent":null,"present":"{}"}}"#,
            digest
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect::<String>(),
            "ab".repeat(32)
        );
        assert_eq!(text, expected);
        assert_eq!(serde_json::from_str::<Held>(&text).unwrap(), held);
    }
}
