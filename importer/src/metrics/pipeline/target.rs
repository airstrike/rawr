//! Fixed-size observation of an actually retained resolution bundle.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::{node, store};

/// The two bundle-owning production seams observed by the fixture harness.
#[derive(Clone, Copy)]
pub enum Kind {
    Resolution,
    Runner,
}

/// An inert bundle association, never freshness or permission to mutate storage.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub struct Target {
    execution: [u8; 32],
    bundle: [u8; 32],
}

impl Target {
    pub(crate) fn observe(reference: store::bundle::BundleRef) -> Self {
        Self {
            execution: reference.execution().as_bytes(),
            bundle: reference.id().as_bytes(),
        }
    }

    pub(in crate::metrics) fn valid(self) -> bool {
        self.execution != [0; 32] && self.bundle != [0; 32]
    }

    /// Derive the observed bundle path using the production bundle namespace.
    #[must_use]
    pub fn bundle_relative(self) -> PathBuf {
        self.reference().relative()
    }

    /// Derive the exact resolution record path using the production layout.
    #[must_use]
    pub fn record_relative(self) -> PathBuf {
        crate::cargo::resolution::layout::observed_record(self.bundle_relative())
    }

    /// Derive the exact production lease path without interpreting store files.
    #[must_use]
    pub fn lease_relative(self) -> PathBuf {
        self.reference().lease_relative()
    }

    fn reference(self) -> store::bundle::BundleRef {
        store::bundle::BundleRef::new(
            node::ExecutionKey::from_bytes(self.execution),
            store::bundle::BundleId::from_bytes(self.bundle),
        )
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn observed_target_retains_the_actual_typed_bundle_association() {
        let reference = crate::store::bundle::BundleRef::new(
            crate::node::ExecutionKey::from_bytes([1; 32]),
            crate::store::bundle::BundleId::from_bytes([2; 32]),
        );
        let target = super::Target::observe(reference);
        assert!(target.valid());
        assert_eq!(target.bundle_relative(), reference.relative());
        assert_eq!(
            target.record_relative(),
            crate::cargo::resolution::layout::observed_record(reference.relative()),
        );
        assert_eq!(std::mem::size_of::<super::Target>(), 64);
    }

    #[test]
    fn missing_bundle_identifiers_are_invalid_observations() {
        assert!(!super::Target {
            execution: [0; 32],
            bundle: [1; 32]
        }
        .valid());
        assert!(!super::Target {
            execution: [1; 32],
            bundle: [0; 32]
        }
        .valid());
    }
}
