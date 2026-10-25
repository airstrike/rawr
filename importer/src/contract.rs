//! Opaque classified Rust-to-Rawr compiler contracts.

use crate::api;

pub mod fingerprint;

/// Uninhabited marker proving every selected path has one closed outcome.
///
/// ```compile_fail
/// let _ = rawr_import::Classified {};
/// ```
pub enum Classified {}

mod phase {
    pub trait Sealed {}
}

/// A closed phase accepted by [`Contract`].
pub trait Phase: phase::Sealed {}

impl phase::Sealed for Classified {}
impl Phase for Classified {}

/// A phase-indexed immutable Rawr compiler contract.
///
/// ```compile_fail
/// let _ = rawr_import::Contract::<rawr_import::Classified> {};
/// ```
///
/// ```compile_fail
/// # fn clone_contract(contract: &rawr_import::Contract<rawr_import::Classified>) {
/// let _: rawr_import::Contract<rawr_import::Classified> = contract.clone();
/// # }
/// ```
pub struct Contract<P: Phase> {
    graph: api::Graph<api::Extracted>,
    support: crate::classify::SupportLedger,
    definitions: Box<[crate::classify::DefinitionPlan]>,
    outcomes: Box<[crate::classify::OutcomeRecord]>,
    canonical: Box<[u8]>,
    fingerprint: fingerprint::Fingerprint,
    allocation: Usage,
    chain: Option<crate::node::Chain>,
    phase: std::marker::PhantomData<fn() -> P>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct Usage {
    pub(crate) items: usize,
    pub(crate) bytes: usize,
}

/// Opaque cumulative P2 allocation proof transferred between compiler transforms.
#[doc(hidden)]
pub struct Allocation {
    contract: fingerprint::Fingerprint,
    usage: Usage,
    item_limit: usize,
    byte_limit: usize,
    /// The item and byte limits are accounting targets; crossings are
    /// recorded on this surface account, never refused.
    account: crate::surface::account::Account,
}

impl Contract<Classified> {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn seal(
        graph: api::Graph<api::Extracted>,
        definitions: Box<[crate::classify::DefinitionPlan]>,
        outcomes: Box<[crate::classify::OutcomeRecord]>,
        support: crate::classify::SupportLedger,
        canonical: Box<[u8]>,
        fingerprint: fingerprint::Fingerprint,
        allocation: Usage,
        chain: Option<crate::node::Chain>,
    ) -> Self {
        Self {
            graph,
            support,
            definitions,
            outcomes,
            canonical,
            fingerprint,
            allocation,
            chain,
            phase: std::marker::PhantomData,
        }
    }

    /// Borrow the current generic-classification node for downstream joins.
    #[doc(hidden)]
    #[must_use]
    pub const fn node_chain(&self) -> Option<&crate::node::Chain> {
        self.chain.as_ref()
    }

    /// Consume the classified graph after demand sealing and transfer its exact
    /// normal-build proof into runner generation.
    #[doc(hidden)]
    pub fn into_native_proof(self) -> crate::input::proof::Proof {
        self.graph.into_freshness_proof()
    }

    /// Borrow the authoritative raw graph consumed by classification.
    #[must_use]
    pub const fn raw_graph(&self) -> &api::Graph<api::Extracted> {
        &self.graph
    }

    /// Return the raw API identity, independent of classifier policy.
    #[must_use]
    pub const fn raw_fingerprint(&self) -> api::Fingerprint {
        self.graph.fingerprint()
    }

    /// Return the independently versioned Rawr contract identity.
    #[must_use]
    pub const fn fingerprint(&self) -> fingerprint::Fingerprint {
        self.fingerprint
    }

    /// Borrow the byte-identical canonical classified encoding.
    #[must_use]
    pub fn canonical_bytes(&self) -> &[u8] {
        &self.canonical
    }

    /// Iterate over exactly one closed outcome per selected export path.
    pub fn outcomes(&self) -> impl ExactSizeIterator<Item = crate::classify::Outcome<'_>> {
        self.outcomes
            .iter()
            .map(|outcome| crate::classify::Outcome::new(outcome, &self.definitions))
    }

    /// Whether a Rust path Rawr names (`alias::a::b`, root alias first)
    /// lies below a wholesale crate re-export (`pub use other_crate;`).
    /// Extraction names such a crate but walks it only on demand, so a path
    /// below it that is missing here may be demanded rather than unknown.
    pub fn reaches_through_crate_alias(&self, path: &str) -> bool {
        let mut segments = path.split("::");
        let Some(root) = segments.next() else {
            return false;
        };
        let segments = segments.collect::<Vec<_>>();
        self.graph.exports().any(|outcome| {
            let api::ledger::Path::Imported(export) = outcome else {
                return false;
            };
            let key = export.key();
            let length = key.path().count();
            key.root_alias() == root
                && length < segments.len()
                && key.path().eq(segments[..length].iter().copied())
                && self
                    .graph
                    .definitions()
                    .find(|definition| definition.id() == export.target())
                    .is_some_and(crate_root)
        })
    }

    /// What the name index of the dependency `path` (`alias::a::b`)
    /// starts with says about it, or `None` when that dependency was extracted
    /// whole (its exports are then complete) or is not a dependency.
    #[must_use]
    pub fn rust_lookup(&self, path: &str) -> Option<api::ledger::Lookup> {
        let mut segments = path.split("::");
        let root = segments.next()?;
        let index = self
            .graph
            .name_index()
            .find(|index| index.root_alias() == root)?;
        Some(index.lookup(&segments.collect::<Vec<_>>()))
    }

    /// Iterate every alias-neutral definition plan exactly once.
    pub fn definition_plans(
        &self,
    ) -> impl ExactSizeIterator<Item = &crate::classify::DefinitionPlan> {
        self.definitions.iter()
    }

    /// Iterate every recursive support outcome exactly once in canonical order.
    pub fn support(&self) -> crate::classify::SupportOutcomes<'_> {
        self.support.outcomes()
    }

    #[cfg(test)]
    pub(crate) const fn support_ledger(&self) -> &crate::classify::SupportLedger {
        &self.support
    }

    /// Borrow recursive support for one exact retained definition plan.
    pub fn definition_support(
        &self,
        definition: crate::surface::identity::Definition,
    ) -> Option<crate::classify::SupportOutcomes<'_>> {
        self.definitions
            .binary_search_by_key(&definition, crate::classify::DefinitionPlan::definition)
            .ok()?;
        self.support.definition_outcomes(definition)
    }

    /// Borrow recursive support for one exact retained implementation outcome.
    pub fn implementation_support(
        &self,
        implementation: crate::surface::identity::Implementation,
    ) -> Option<crate::classify::SupportOutcomes<'_>> {
        self.outcomes()
            .flat_map(|outcome| outcome.implementations())
            .find(|outcome| outcome.identity() == implementation)?;
        self.support.implementation_outcomes(implementation)
    }

    /// The sealed cumulative classified usage, without opening an allocation.
    #[cfg(feature = "rustc-driver")]
    pub(crate) const fn usage(&self) -> Usage {
        self.allocation
    }

    /// Start the cumulative allocation ledger for Rawr facade and demand transforms.
    #[doc(hidden)]
    pub fn allocation(
        &self,
        limits: &crate::cargo::Limits,
    ) -> Result<Allocation, crate::surface::Error> {
        Allocation::new(self.fingerprint, self.allocation, limits)
    }

    /// Inspect the sealed cumulative Contract usage at the driver audit boundary.
    #[cfg(feature = "rustc-driver")]
    #[doc(hidden)]
    #[must_use]
    pub const fn driver_retained_usage(&self) -> (usize, usize) {
        (self.allocation.items, self.allocation.bytes)
    }

    #[cfg(feature = "rustc-driver")]
    #[allow(
        dead_code,
        reason = "used only by the private nonpublishing classification evidence lane"
    )]
    pub(crate) fn classification_evidence(
        &self,
        scratch_bytes: usize,
        stage: crate::process::EvidenceStage,
    ) -> Option<crate::classify::ClassificationEvidence> {
        crate::classify::classification_evidence(
            &self.graph,
            &self.definitions,
            &self.outcomes,
            &self.support,
            self.canonical.len(),
            self.allocation,
            scratch_bytes,
            stage,
        )
    }

    /// Look up a classified outcome by its semantic surface identity.
    #[must_use]
    pub fn outcome(
        &self,
        identity: &crate::surface::identity::Identity,
    ) -> Option<crate::classify::Outcome<'_>> {
        self.outcomes().find_map(|outcome| {
            if outcome.identity().same_binding(identity)
                && outcome
                    .representability()
                    .contains_selector(outcome.identity().selector(), identity.selector())
            {
                return Some(outcome);
            }
            outcome
                .implementations()
                .flat_map(|implementation| implementation.associated())
                .find(|associated| {
                    associated.identity().same_binding(identity)
                        && associated.representability().contains_selector(
                            associated.identity().selector(),
                            identity.selector(),
                        )
                })
        })
    }

    /// Derive one exact alias-bound operation identity from a classified selector.
    #[doc(hidden)]
    pub fn select(
        &self,
        base: &crate::surface::identity::Identity,
        selector: crate::surface::identity::Selector,
    ) -> Result<Option<crate::surface::identity::Identity>, crate::classify::Error> {
        let Some(outcome) = self.outcome(base) else {
            return Ok(None);
        };
        let selected = base.selected(selector);
        Ok(outcome
            .representability()
            .contains_selector(outcome.identity().selector(), selected.selector())
            .then_some(selected))
    }

    /// Check one exact selector against an alias-bound classified identity
    /// without allocating the selected identity.
    #[doc(hidden)]
    #[must_use]
    pub fn accepts(
        &self,
        base: &crate::surface::identity::Identity,
        selector: &crate::surface::identity::Selector,
    ) -> bool {
        self.outcome(base).is_some_and(|outcome| {
            outcome
                .representability()
                .contains_selector(outcome.identity().selector(), selector)
        })
    }
}

impl Usage {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn classified(
        graph: &crate::api::Graph<impl crate::api::Phase>,
        definitions: &[crate::classify::DefinitionPlan],
        outcomes: &[crate::classify::OutcomeRecord],
        support: &crate::classify::SupportLedger,
        canonical_bytes: usize,
        precharged: (usize, usize),
        item_limit: usize,
        byte_limit: usize,
    ) -> Result<Self, crate::classify::Error> {
        let graph_usage = graph.allocation();
        let mut items = graph_usage
            .items
            .checked_add(1)
            .ok_or(crate::classify::Error::Limit {
                resource: "classified retained items",
                limit: item_limit,
                actual: usize::MAX,
            })?;
        items = items
            .checked_add(definitions.len())
            .and_then(|items| items.checked_add(support.groups_len()))
            .and_then(|items| items.checked_add(support.len()))
            .and_then(|items| items.checked_add(support.rejections_len()))
            .and_then(|items| {
                outcomes.iter().try_fold(items, |items, outcome| {
                    let implementations = outcome.implementations().len();
                    let associated = outcome.implementations().iter().try_fold(
                        0_usize,
                        |count, implementation| {
                            count.checked_add(implementation.associated().len())
                        },
                    )?;
                    items
                        .checked_add(1)?
                        .checked_add(implementations)?
                        .checked_add(associated)
                })
            })
            .ok_or(crate::classify::Error::Limit {
                resource: "classified retained items",
                limit: item_limit,
                actual: usize::MAX,
            })?;
        if items > item_limit {
            return Err(crate::classify::Error::Limit {
                resource: "classified retained items",
                limit: item_limit,
                actual: items,
            });
        }
        if items != precharged.0 {
            return Err(crate::classify::Error::Cardinality);
        }
        if precharged.1 > byte_limit {
            return Err(crate::classify::Error::Limit {
                resource: "classified retained bytes",
                limit: byte_limit,
                actual: precharged.1,
            });
        }
        if canonical_bytes > precharged.1 || graph_usage.bytes > precharged.1 {
            return Err(crate::classify::Error::Cardinality);
        }
        Ok(Self {
            items,
            bytes: precharged.1,
        })
    }
}

impl Allocation {
    #[cfg(feature = "test-fixtures")]
    pub(crate) fn fixture(
        contract: fingerprint::Fingerprint,
        limits: &crate::cargo::Limits,
    ) -> Self {
        Self {
            contract,
            usage: Usage { items: 0, bytes: 0 },
            item_limit: limits.classified_contract_items,
            byte_limit: limits.classified_contract_bytes,
            account: crate::surface::account::Account::surface(),
        }
    }

    pub(crate) fn new(
        contract: fingerprint::Fingerprint,
        usage: Usage,
        limits: &crate::cargo::Limits,
    ) -> Result<Self, crate::surface::Error> {
        let allocation = Self {
            contract,
            usage: Usage { items: 0, bytes: 0 },
            item_limit: limits.classified_contract_items,
            byte_limit: limits.classified_contract_bytes,
            account: crate::surface::account::Account::surface(),
        };
        let mut allocation = allocation;
        allocation.retain(usage.items, usage.bytes)?;
        Ok(allocation)
    }

    /// Precharge exact newly retained items and bytes before ownership or reserve.
    #[doc(hidden)]
    pub fn retain(&mut self, items: usize, bytes: usize) -> Result<(), crate::surface::Error> {
        let actual_items =
            self.usage
                .items
                .checked_add(items)
                .ok_or(crate::surface::Error::Limit {
                    resource: "P2 cumulative retained items",
                    limit: self.item_limit,
                    actual: usize::MAX,
                })?;
        let actual_bytes =
            self.usage
                .bytes
                .checked_add(bytes)
                .ok_or(crate::surface::Error::Limit {
                    resource: "P2 cumulative retained bytes",
                    limit: self.byte_limit,
                    actual: usize::MAX,
                })?;
        // The classified Contract limits are accounting targets. A
        // crossing is recorded with the truthful cumulative amount and the
        // charge is accepted; only checked-arithmetic overflow refuses.
        self.account.exceeded(
            "P2 cumulative retained items",
            self.item_limit,
            actual_items,
        );
        self.account.exceeded(
            "P2 cumulative retained bytes",
            self.byte_limit,
            actual_bytes,
        );
        self.usage = Usage {
            items: actual_items,
            bytes: actual_bytes,
        };
        Ok(())
    }

    /// Precharge checked aggregate ownership without disguising arithmetic overflow.
    #[doc(hidden)]
    pub fn retain_checked(
        &mut self,
        items: Option<usize>,
        bytes: Option<usize>,
    ) -> Result<(), crate::surface::Error> {
        let items = items.ok_or(crate::surface::Error::Limit {
            resource: "P2 cumulative retained items",
            limit: self.item_limit,
            actual: usize::MAX,
        })?;
        let bytes = bytes.ok_or(crate::surface::Error::Limit {
            resource: "P2 cumulative retained bytes",
            limit: self.byte_limit,
            actual: usize::MAX,
        })?;
        self.retain(items, bytes)
    }

    pub(crate) fn matches(&self, contract: fingerprint::Fingerprint) -> bool {
        self.contract == contract
    }

    pub(crate) const fn limits(&self) -> (usize, usize) {
        (self.item_limit, self.byte_limit)
    }

    /// The accounting targets this allocation crossed so far.
    #[must_use]
    pub const fn exceedances(&self) -> crate::exceedance::Record {
        self.account.exceedances()
    }

    pub(crate) const fn retained_usage(&self) -> (usize, usize) {
        (self.usage.items, self.usage.bytes)
    }

    #[cfg(feature = "rustc-driver")]
    #[doc(hidden)]
    #[must_use]
    pub const fn driver_usage(&self) -> (usize, usize) {
        (self.usage.items, self.usage.bytes)
    }

    #[cfg(test)]
    pub(crate) const fn usage(&self) -> (usize, usize) {
        (self.usage.items, self.usage.bytes)
    }
}

/// A crate root module is spelled by its crate name alone, once in its own
/// crate's fragment and crate-qualified (twice) in another's.
fn crate_root(definition: &api::model::Definition) -> bool {
    let name = definition.id().unit().crate_name();
    matches!(definition.kind(), api::model::Kind::Module)
        && matches!(definition.display_path().count(), 1 | 2)
        && definition.display_path().all(|segment| segment == name)
}

#[cfg(test)]
mod tests {
    use super::{fingerprint, Allocation, Usage};

    const CONTRACT: fingerprint::Fingerprint = fingerprint::Fingerprint([0x43; 32]);

    #[test]
    fn allocation_seeds_the_exact_classified_contract_authority_atomically() {
        let limits = crate::cargo::Limits::standard();
        let exact = Usage {
            items: limits.classified_contract_items,
            bytes: limits.classified_contract_bytes,
        };
        let allocation = Allocation::new(CONTRACT, exact, &limits)
            .expect("the exact classified Contract boundary is accepted");
        assert_eq!(allocation.usage(), (exact.items, exact.bytes));
        assert_eq!(allocation.limits(), (exact.items, exact.bytes));

        assert!(!allocation.exceedances().is_exceeded());

        // One over either target is accepted and recorded truthfully.
        let item_over = Usage {
            items: exact.items + 1,
            bytes: 0,
        };
        let allocation = Allocation::new(CONTRACT, item_over, &limits)
            .expect("the item target records and admits");
        let axis = *allocation
            .exceedances()
            .axis("P2 cumulative retained items")
            .unwrap();
        assert_eq!((axis.target, axis.high_water), (6_500_000, 6_500_001));

        let byte_over = Usage {
            items: 0,
            bytes: exact.bytes + 1,
        };
        let allocation = Allocation::new(CONTRACT, byte_over, &limits)
            .expect("the byte target records and admits");
        let axis = *allocation
            .exceedances()
            .axis("P2 cumulative retained bytes")
            .unwrap();
        assert_eq!((axis.target, axis.high_water), (671_088_640, 671_088_641));
    }

    #[test]
    fn classified_contract_allocation_is_isolated_from_unrelated_authorities() {
        let standard = crate::cargo::Limits::standard();
        let mut isolated = standard;
        isolated.items = 0;
        isolated.json_bytes = 0;
        isolated.canonical_items = 0;
        isolated.canonical_bytes = 0;
        isolated.input_proof_bytes = 0;
        isolated.retained_graph_items = 0;
        isolated.retained_graph_bytes = 0;

        let usage = Usage {
            items: standard.classified_contract_items,
            bytes: standard.classified_contract_bytes,
        };
        let allocation = Allocation::new(CONTRACT, usage, &isolated)
            .expect("unrelated authorities do not constrain Contract retention");
        assert_eq!(
            allocation.limits(),
            (
                standard.classified_contract_items,
                standard.classified_contract_bytes,
            )
        );

        isolated.items = usize::MAX;
        isolated.json_bytes = usize::MAX;
        isolated.classified_contract_items = usage.items - 1;
        let allocation = Allocation::new(CONTRACT, usage, &isolated)
            .expect("the item target records and admits");
        let axis = *allocation
            .exceedances()
            .axis("P2 cumulative retained items")
            .unwrap();
        assert_eq!(axis.target as usize, usage.items - 1);
        assert_eq!(axis.high_water as usize, usage.items);
    }

    #[test]
    fn later_retain_remains_one_atomic_cumulative_transaction() {
        let limits = crate::cargo::Limits::standard();
        let seed = Usage {
            items: limits.classified_contract_items - 1,
            bytes: limits.classified_contract_bytes - 1,
        };
        let mut allocation = Allocation::new(CONTRACT, seed, &limits)
            .expect("the classified Contract seed is within its authority");
        allocation
            .retain(1, 1)
            .expect("the exact later cumulative boundary is accepted");
        let retained = allocation.usage();
        assert_eq!(
            retained,
            (
                limits.classified_contract_items,
                limits.classified_contract_bytes,
            )
        );

        assert!(!allocation.exceedances().is_exceeded());

        // Past both targets the cumulative charge is still one atomic
        // transaction; it is accepted and both crossings are recorded.
        allocation
            .retain(1, 1)
            .expect("the targets record and admit");
        assert_eq!(allocation.usage(), (retained.0 + 1, retained.1 + 1));
        let record = allocation.exceedances();
        assert_eq!(
            record
                .axis("P2 cumulative retained items")
                .map(|axis| axis.high_water),
            Some(6_500_001)
        );
        assert_eq!(
            record
                .axis("P2 cumulative retained bytes")
                .map(|axis| axis.high_water),
            Some(671_088_641)
        );
        allocation
            .retain(usize::MAX, 0)
            .expect_err("checked overflow still refuses");
        assert_eq!(allocation.usage(), (retained.0 + 1, retained.1 + 1));
    }
}
