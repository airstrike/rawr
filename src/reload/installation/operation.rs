//! Opaque host operation prepared without pre-admission execution authority.

use crate::host;

/// One inert issued operation sealed until final host admission.
#[doc(hidden)]
pub struct Prepared(host::operation::Pending);

impl Prepared {
    pub(super) const fn new(pending: host::operation::Pending) -> Self {
        Self(pending)
    }

    pub(super) fn into_inner(self) -> host::operation::Pending {
        self.0
    }
}

impl std::fmt::Debug for Prepared {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("Prepared(<private>)")
    }
}
