//! What a value a runner keeps on disk names of the process that made it
//! (#1638): a Rust type by its `TypeId`, a facade by its root, an adapter
//! by the runner's carrier, a text the program holds for as long as it
//! runs.

pub(crate) mod carrier;
pub(crate) mod fingerprint;
pub(crate) mod text;
pub(crate) mod type_id;

use crate::Shared;
use std::any::TypeId;
use std::cell::RefCell;
use std::collections::BTreeMap;

thread_local! {
    /// What a write or a read under way knows (`Known::within`).
    static KNOWN: RefCell<Option<Known>> = const { RefCell::new(None) };
}

/// What a stored value may name: the runtime's own value types and the
/// types an engine's facades link, by their `TypeId`s' texts, the engine's
/// facades, by their roots, and its runner's carrier.
#[derive(Clone)]
pub(crate) struct Known {
    types: Shared<BTreeMap<Box<str>, TypeId>>,
    facades: Shared<BTreeMap<Box<str>, crate::host::facade::Fingerprint>>,
    carrier: Option<[u8; 32]>,
}

impl Known {
    /// The types `types` names, the facades `facades`, and the identity
    /// of the carrier `carrier`.
    pub(crate) fn new<'f>(
        types: impl IntoIterator<Item = TypeId>,
        facades: impl IntoIterator<Item = &'f crate::host::facade::Dependency>,
        carrier: Option<[u8; 32]>,
    ) -> Self {
        Self {
            types: Shared::new(
                types
                    .into_iter()
                    .map(|type_id| (type_id::text(type_id).into_boxed_str(), type_id))
                    .collect(),
            ),
            facades: Shared::new(
                facades
                    .into_iter()
                    .map(|facade| (facade.root().into(), facade.fingerprint().clone()))
                    .collect(),
            ),
            carrier,
        }
    }

    /// What `f` gives, writing and reading what these know.
    pub(crate) fn within<R>(&self, f: impl FnOnce() -> R) -> R {
        let earlier = KNOWN.replace(Some(self.clone()));
        let made = f();
        KNOWN.set(earlier);
        made
    }
}

/// What `find` finds among what the write or read under way knows.
fn known<T>(find: impl FnOnce(&Known) -> Option<T>) -> Option<T> {
    KNOWN.with_borrow(|known| find(known.as_ref()?))
}
