//! Positive corruption observed in authenticated candidate-owned storage.

use super::census::Census;
use super::{accounting, Allocation, BundleCommitment, BundleRef, Charge, Record};
use crate::node::{Digest, Error, GraphBudget, Kind};
use std::path::Path;

#[derive(Clone, Debug)]
pub(super) enum Fact {
    Seal,
    Census(Box<Census>),
    Geometry,
    Mode,
    Member,
}

impl Fact {
    /// A count change or growth is a census fact; a per-member logical
    /// change is a geometry fact. A valid allocation release is never a fact.
    /// A census fact carries the totals that differ and the member a second
    /// walk of `root` finds most likely to explain them.
    pub(super) fn drift(
        drift: super::Drift,
        root: &Path,
        kind: Kind,
        recorded: Charge,
        measured: Charge,
    ) -> Self {
        match drift {
            super::Drift::Count | super::Drift::Growth => Self::Census(Box::new(Census::observe(
                root, kind, drift, recorded, measured,
            ))),
            super::Drift::Geometry => Self::Geometry,
        }
    }
}

/// Opaque evidence; only candidate-owned comparisons can construct this value.
#[derive(Clone)]
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
        match &self.fact {
            Fact::Seal => f.write_str("authenticated candidate bundle seal bytes changed"),
            Fact::Census(census) => {
                write!(
                    f,
                    "authenticated candidate bundle physical census changed: {census}"
                )
            }
            Fact::Geometry => f.write_str("authenticated candidate bundle sealed geometry changed"),
            Fact::Mode => f.write_str("authenticated candidate bundle member became writable"),
            Fact::Member => f.write_str("authenticated candidate bundle inventory member changed"),
        }
    }
}

impl Corruption {
    #[allow(clippy::too_many_arguments)]
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
        let record = Record::read_bundle_budgeted(root)?;
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

    pub(crate) fn allocation(&self) -> Allocation {
        self.allocation
    }
    pub(crate) fn reference(&self) -> BundleRef {
        self.reference
    }
}
