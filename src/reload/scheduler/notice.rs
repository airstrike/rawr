//! Non-install revision proofs for live reload progress.

use std::fmt;

/// Proof that one relevant event created a reload revision, which carries
/// no compile or install authority.
pub struct Observed(());

impl Observed {
    pub(super) const fn new() -> Self {
        Self(())
    }
}

impl fmt::Debug for Observed {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("Observed(<private>)")
    }
}
