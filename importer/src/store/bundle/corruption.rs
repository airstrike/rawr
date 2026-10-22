//! Positive corruption observed in authenticated candidate-owned storage.

use super::{accounting, Allocation, BundleCommitment, BundleRef, Record};
use crate::node::{Digest, Error, GraphBudget, Kind};
use std::path::Path;

#[derive(Clone, Copy, Debug)]
pub(super) enum Fact {
    Seal,
    Census,
    Geometry,
    Mode,
    Member,
}

impl Fact {
    /// A count change or growth is a census fact; a per-member logical
    /// change is a geometry fact. A valid allocation release is never a fact.
    pub(super) const fn drift(drift: super::Drift) -> Self {
        match drift {
            super::Drift::Count | super::Drift::Growth => Self::Census,
            super::Drift::Geometry => Self::Geometry,
        }
    }
}

/// Opaque evidence; only candidate-owned comparisons can construct this value.
#[derive(Clone, Copy)]
pub struct Corruption {
    reference: BundleRef,
    allocation: Allocation,
    fact: Fact,
}

impl std::fmt::Debug for Corruption {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Corruption")
            .field("reference", &self.reference)
            .field("fact", &self.fact)
            .finish()
    }
}

impl std::fmt::Display for Corruption {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.fact {
            Fact::Seal => f.write_str("authenticated candidate bundle seal bytes changed"),
            Fact::Census => f.write_str("authenticated candidate bundle physical census changed"),
            Fact::Geometry => f.write_str("authenticated candidate bundle sealed geometry changed"),
            Fact::Mode => f.write_str("authenticated candidate bundle member became writable"),
            Fact::Member => f.write_str("authenticated candidate bundle inventory member changed"),
        }
    }
}

impl Corruption {
    pub(super) fn observed(
        root: &Path,
        store: &Path,
        commitment: BundleCommitment,
        kind: Kind,
        schema: u32,
        key: Digest,
        fact: Fact,
        budget: &GraphBudget,
    ) -> Result<Self, Error> {
        let record = Record::read_bundle_budgeted(root, budget)?;
        // Legacy incompatibility never fabricates corruption evidence.
        #[cfg(test)]
        let legacy = record.is_legacy_bundle() && !super::geometry::seam::is_legacy_writer();
        #[cfg(not(test))]
        let legacy = record.is_legacy_bundle();
        if legacy {
            return Err(Error::Codec(
                "legacy schema-4 runner bundle record is not corruption evidence",
            ));
        }
        record.validate_bundle_descriptor(
            kind,
            schema,
            key,
            commitment.seal_bytes,
            commitment.seal_digest,
        )?;
        accounting::registered_with_budget(store, &record, Some(budget))?;
        Ok(Self {
            reference: commitment.reference,
            allocation: record.allocation(),
            fact,
        })
    }

    pub(crate) fn member(
        evidence: crate::runner::artifact::retained::Mismatch,
        root: &Path,
        store: &Path,
        kind: Kind,
        schema: u32,
        key: Digest,
        budget: &GraphBudget,
    ) -> Result<Self, Error> {
        Self::observed(
            root,
            store,
            evidence.commitment(),
            kind,
            schema,
            key,
            Fact::Member,
            budget,
        )
    }

    pub(crate) fn allocation(self) -> Allocation {
        self.allocation
    }
    pub(crate) fn reference(self) -> BundleRef {
        self.reference
    }
}
