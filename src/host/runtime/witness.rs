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

/// The authority of one runtime registry: the runner that built its bindings
/// and the script generation it serves.
///
/// Every generation of one runner shares the runner part. A witness matches
/// only its own generation, except through the native checks below, which
/// accept a value from an earlier generation of the same runner.
#[derive(Clone, Copy, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub(super) struct Authority {
    runner: usize,
    generation: usize,
}

fn next() -> usize {
    static NEXT: AtomicUsize = AtomicUsize::new(1);

    #[allow(deprecated)] // MSRV 1.91 predates `AtomicUsize::try_update`.
    NEXT.fetch_update(Ordering::Relaxed, Ordering::Relaxed, |value| {
        value.checked_add(1)
    })
    .expect("Rawr runtime witness authorities cannot be exhausted")
}

impl Authority {
    pub(super) fn issue() -> Self {
        let issued = next();
        Self {
            runner: issued,
            generation: issued,
        }
    }

    /// The same runner's authority for its next script generation.
    pub(super) fn next_generation(self) -> Self {
        Self {
            runner: self.runner,
            generation: next(),
        }
    }

    pub(super) const fn same_runner(self, other: Self) -> bool {
        self.runner == other.runner
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

    /// Match a native Rust value's witness from any generation of this runner.
    ///
    /// A native value holds no script code, and its Rust code cannot change
    /// inside one runner, so only the runner and the exact type must agree.
    pub(super) fn matches_native(&self, authority: Authority, ty: &Type) -> bool {
        self.authority.same_runner(authority) && *self.ty == *ty
    }

    pub(super) fn arguments_match(&self, authority: Authority, ty: &Type) -> bool {
        self.authority == authority && self.ty.arguments() == ty.arguments()
    }

    /// The witness of this type's argument at `index`, under the same
    /// authority: a `Task<T>` witness gives the witness of each `T` its
    /// native task yields.
    #[doc(hidden)]
    #[must_use]
    pub fn argument_witness(&self, index: usize) -> Option<Self> {
        let ty = self.ty.arguments().get(index)?.clone();
        Some(Self::issue(self.authority, ty))
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
