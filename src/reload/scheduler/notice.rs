//! Non-install revision proofs for live reload progress.

use std::fmt;
use std::sync::Arc;

use super::{Revision, Stamp};

/// Proof that one relevant event created an exact reload revision.
pub struct Observed {
    stamp: Arc<Stamp>,
    revision: Revision,
}

/// One revision-bound progress value without compile or install authority.
#[must_use = "a progress notice should be delivered or retired"]
pub struct Value<T> {
    value: T,
}

impl Observed {
    pub(super) fn new(stamp: Arc<Stamp>, revision: Revision) -> Self {
        Self { stamp, revision }
    }

    pub(super) fn into_parts(self) -> (Arc<Stamp>, Revision) {
        (self.stamp, self.revision)
    }
}

impl<T> Value<T> {
    pub(super) const fn new(_revision: Revision, value: T) -> Self {
        Self { value }
    }

    /// Consume this notice into its authority-free progress value.
    #[must_use]
    pub fn into_inner(self) -> T {
        self.value
    }
}

impl fmt::Debug for Observed {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("Observed(<private>)")
    }
}

impl<T> fmt::Debug for Value<T> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("Value(<private>)")
    }
}
