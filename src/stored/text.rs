//! A `&'static str` a stored value holds, read back as the one copy of it
//! this process holds. Each text is copied once, the first time a read
//! meets it, and held while the process runs: a program's types name few.

use std::collections::BTreeSet;
use std::sync::{Mutex, PoisonError};

/// Each text a read met so far.
static HELD: Mutex<BTreeSet<&'static str>> = Mutex::new(BTreeSet::new());

pub(crate) fn serialize<S: serde::Serializer>(
    text: &&'static str,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    serializer.serialize_str(text)
}

pub(crate) fn deserialize<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<&'static str, D::Error> {
    <String as serde::Deserialize>::deserialize(deserializer).map(held)
}

/// [`serialize`] of a text that may be absent.
pub(crate) fn serialize_optional<S: serde::Serializer>(
    text: &Option<&'static str>,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    serde::Serialize::serialize(text, serializer)
}

/// [`deserialize`] of a text that may be absent.
pub(crate) fn deserialize_optional<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<&'static str>, D::Error> {
    <Option<String> as serde::Deserialize>::deserialize(deserializer).map(|text| text.map(held))
}

/// The copy of `text` this process holds.
fn held(text: String) -> &'static str {
    let mut held = HELD.lock().unwrap_or_else(PoisonError::into_inner);
    if let Some(text) = held.get(text.as_str()) {
        return text;
    }
    let text: &'static str = Box::leak(text.into_boxed_str());
    held.insert(text);
    text
}
