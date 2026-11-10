//! A `TypeId` a stored value holds, written as its text and read back as
//! the type of this process's that has that text (`Known`). The same build
//! gives a type the same text. A type the process doesn't know fails the
//! write or the read, and what holds it is made again.

use crate::Shared;
use std::any::TypeId;
use std::cell::RefCell;
use std::collections::BTreeMap;

thread_local! {
    /// The types a write or a read under way knows (`Known::within`).
    static KNOWN: RefCell<Option<Known>> = const { RefCell::new(None) };
}

/// The types a stored value may name, by their texts: the runtime's own
/// value types and the types an engine's facades link.
#[derive(Clone)]
pub(crate) struct Known(Shared<BTreeMap<Box<str>, TypeId>>);

impl Known {
    /// The types `types` names.
    pub(crate) fn new(types: impl IntoIterator<Item = TypeId>) -> Self {
        Self(Shared::new(
            types
                .into_iter()
                .map(|type_id| (text(type_id).into_boxed_str(), type_id))
                .collect(),
        ))
    }

    /// What `f` gives, writing and reading the types these know.
    pub(crate) fn within<R>(&self, f: impl FnOnce() -> R) -> R {
        let earlier = KNOWN.replace(Some(self.clone()));
        let made = f();
        KNOWN.set(earlier);
        made
    }
}

fn text(type_id: TypeId) -> String {
    format!("{type_id:?}")
}

/// The type `text` names among the known ones.
fn known(text: &str) -> Option<TypeId> {
    KNOWN.with_borrow(|known| known.as_ref()?.0.get(text).copied())
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
