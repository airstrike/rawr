//! An [`Identifier`] a stored value holds, written as its text. The
//! string type's own serde support is a feature of its crate, which a
//! runner leaves as the project has it.

use crate::Identifier;

pub(crate) fn serialize<S: serde::Serializer>(
    identifier: &Identifier,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    serializer.serialize_str(identifier)
}

pub(crate) fn deserialize<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Identifier, D::Error> {
    <crate::ImmutableString as serde::Deserialize>::deserialize(deserializer).map(Into::into)
}
