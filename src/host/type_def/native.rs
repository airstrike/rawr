//! Opaque linked native type identity.

use std::any::TypeId;

/// A runtime identity that can only originate from a linked Rust type.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct Identity(TypeId);

impl Identity {
    pub(super) fn of<T: 'static>() -> Self {
        Self(TypeId::of::<T>())
    }

    pub(crate) const fn from_type_id(identity: TypeId) -> Self {
        Self(identity)
    }

    pub(crate) const fn type_id(self) -> TypeId {
        self.0
    }
}
