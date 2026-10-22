//! Opaque evidence for one launch-local logical host instantiation.

use super::type_ref::Type;
use crate::Shared;
use std::fmt;
use std::sync::atomic::{AtomicUsize, Ordering};

/// Unforgeable evidence that a value belongs to one exact logical host type.
///
/// A witness can only be issued while an authenticated runtime item or
/// projection is executing. Cloning a value may clone its witness, but public
/// host code cannot construct or retarget one. The proven type is shared, so
/// cloning a witness never copies the type proof.
#[derive(Clone, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct Witness {
    authority: Authority,
    ty: Shared<Type>,
}

#[derive(Clone, Copy, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub(super) struct Authority(usize);

impl Authority {
    pub(super) fn issue() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(1);

        #[allow(deprecated)] // MSRV 1.91 predates `AtomicUsize::try_update`.
        let issued = NEXT
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |value| {
                value.checked_add(1)
            })
            .expect("Rawr runtime witness authorities cannot be exhausted");
        Self(issued)
    }
}

impl Witness {
    pub(super) fn issue(authority: Authority, ty: Type) -> Self {
        Self {
            authority,
            ty: Shared::new(ty),
        }
    }

    pub(super) fn matches(&self, authority: Authority, ty: &Type) -> bool {
        self.authority == authority && *self.ty == *ty
    }

    pub(super) fn arguments_match(&self, authority: Authority, ty: &Type) -> bool {
        self.authority == authority && self.ty.arguments() == ty.arguments()
    }

    #[cfg(not(feature = "no_function"))]
    pub(crate) fn argument(&self, index: usize) -> Option<&Type> {
        self.ty.arguments().get(index)
    }
}

impl fmt::Debug for Witness {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("Witness(<private>)")
    }
}
