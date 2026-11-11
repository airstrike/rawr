//! A `TypeId` a stored value holds, written as its text and read back as
//! the type of this process's that has that text (`super::Known`). The same
//! build gives a type the same text. A type the process doesn't know fails
//! the write or the read, and what holds it is made again.

use std::any::TypeId;

pub(super) fn text(type_id: TypeId) -> String {
    format!("{type_id:?}")
}

/// The type `text` names among the known ones.
fn known(text: &str) -> Option<TypeId> {
    super::known(|known| known.types.get(text).copied())
}

pub(crate) fn serialize<S: serde::Serializer>(
    type_id: &TypeId,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    let text = text(*type_id);
    if known(&text).is_none() {
        return Err(serde::ser::Error::custom(format_args!(
            "{text} is no type a read knows"
        )));
    }
    serializer.serialize_str(&text)
}

pub(crate) fn deserialize<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<TypeId, D::Error> {
    let text = <String as serde::Deserialize>::deserialize(deserializer)?;
    known(&text).ok_or_else(|| {
        serde::de::Error::custom(format_args!("{text} is no type this process knows"))
    })
}
