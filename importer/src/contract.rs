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
    support: std::sync::Arc<crate::classify::SupportLedger>,
    definitions: std::sync::Arc<[crate::classify::DefinitionPlan]>,
    outcomes: std::sync::Arc<[crate::classify::OutcomeRecord]>,
    canonical: std::sync::Arc<[u8]>,
    fingerprint: fingerprint::Fingerprint,
    allocation: Usage,
    chain: Option<crate::node_parts::Chain>,
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
}

impl Contract<Classified> {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn seal(
        graph: api::Graph<api::Extracted>,
        definitions: std::sync::Arc<[crate::classify::DefinitionPlan]>,
        outcomes: std::sync::Arc<[crate::classify::OutcomeRecord]>,
        support: std::sync::Arc<crate::classify::SupportLedger>,
        canonical: std::sync::Arc<[u8]>,
        fingerprint: fingerprint::Fingerprint,
        allocation: Usage,
        chain: Option<crate::node_parts::Chain>,
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
    pub const fn node_chain(&self) -> Option<&crate::node_parts::Chain> {
        self.chain.as_ref()
    }

    /// Consume the classified graph after demand sealing and transfer its exact
    /// normal-build proof into runner generation.
    #[cfg(not(target_arch = "wasm32"))]
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

    /// `owner` (`alias::a::Type`) under each other selected root whose
    /// table reaches `alias`'s crate, by its hidden segment or a whole
    /// re-export (`other::__rawr_crate_<name>_<id>::a::Type`,
    /// `other::ratatui::a::Type`), or the crate defining the type by its
    /// hidden segment, at the type's own path there. An `owner` whose root
    /// is a crate no root is, as rustc names a type only another crate
    /// reaches, is under each root that reaches that crate's hidden
    /// segment (`other::__rawr_crate_<name>_<id>::Type`). A trait of that other
    /// crate may give the type a member by an impl for it, as ratzilla's
    /// `impl<B> WebRenderer for Terminal<B>` gives ratatui's terminal
    /// `draw_web`, and only that crate's table reaches the trait (#1450).
    #[must_use]
    pub fn hidden_owner_paths(&self, owner: &str) -> Vec<String> {
        let mut segments = owner.split("::");
        let Some(root) = segments.next() else {
            return Vec::new();
        };
        let rest = segments.collect::<Vec<_>>();
        let exports = || {
            self.graph.exports().filter_map(|outcome| match outcome {
                api::ledger::Path::Imported(export) => Some(export),
                _ => None,
            })
        };
        // `alias`'s own paths, each with what it names.
        let own = exports()
            .filter(|export| export.key().root_alias() == root)
            .map(|export| (export.key().path().collect::<Vec<_>>(), export.target()))
            .collect::<std::collections::HashMap<_, _>>();
        let mut hidden = Vec::new();
        // Another root reaches `alias`'s crate at a prefix below which it
        // names what `alias` names at the same path: its hidden segment, or
        // a module re-exporting the crate whole, as ratzilla's `pub use
        // ratatui;`. The type is at `a::Type` there; the shortest prefix
        // is the crate's root.
        for export in exports().filter(|export| export.key().root_alias() != root) {
            let path = export.key().path().collect::<Vec<_>>();
            let reached =
                (1..path.len()).find(|&split| own.get(&path[split..]) == Some(&export.target()));
            if let Some(split) = reached {
                hidden.push(
                    std::iter::once(export.key().root_alias())
                        .chain(path[..split].iter().copied())
                        .chain(rest.iter().copied())
                        .collect::<Vec<_>>()
                        .join("::"),
                );
            }
        }
        // The crate that defines the type, as ratatui re-exports
        // ratatui-core's `Terminal`: under each root whose table reaches
        // that crate's segment, the type at its own path there.
        if let Some(target) = own.get(&rest)
            && let Some(definition) = self
                .graph
                .definitions()
                .find(|definition| definition.id() == *target)
        {
            let unit = target.unit();
            let segment = format!(
                "{}{}_{}",
                api::ledger::CRATE_SEGMENT,
                unit.crate_name(),
                unit.stable_crate_id()
            );
            let below = definition.display_path().collect::<Vec<_>>().join("::");
            for export in exports() {
                let key = export.key();
                if key.root_alias() != root && key.path().next() == Some(segment.as_str()) {
                    hidden.push(format!("{}::{segment}::{below}", key.root_alias()));
                }
            }
        }
        // A crate no root is, named as rustc names it, as `tracing_core::
        // Level` is named where only inboard's `log` reaches tracing: under
        // each root whose table reaches that crate's segment, the type at
        // the same path there.
        if own.is_empty() {
            let prefix = format!("{}{root}_", api::ledger::CRATE_SEGMENT);
            for export in exports() {
                let key = export.key();
                if let Some(segment) = key.path().next()
                    && segment.strip_prefix(prefix.as_str()).is_some_and(|id| {
                        id.len() == 16 && id.bytes().all(|b| b.is_ascii_hexdigit())
                    })
                {
                    hidden.push(
                        std::iter::once(key.root_alias())
                            .chain(std::iter::once(segment))
                            .chain(rest.iter().copied())
                            .collect::<Vec<_>>()
                            .join("::"),
                    );
                }
            }
        }
        hidden.sort_unstable();
        hidden.dedup();
        hidden
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
    pub(crate) fn support_ledger(&self) -> &crate::classify::SupportLedger {
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

    /// Borrow recursive support for one exact retained implementation
    /// outcome. A trait's members through its `Self` are an implementation
    /// the classifier makes, which retains no support of its own.
    pub fn implementation_support(
        &self,
        implementation: crate::surface::identity::Implementation,
    ) -> Option<crate::classify::SupportOutcomes<'_>> {
        self.outcomes()
            .flat_map(|outcome| outcome.implementations())
            .find(|outcome| outcome.identity() == implementation)?;
        Some(
            self.support
                .implementation_outcomes(implementation)
                .unwrap_or_else(|| self.support.no_outcomes()),
        )
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
        limits: &crate::limits::Limits,
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
        self.outcomes()
            .find_map(|outcome| binding_outcome(outcome, identity))
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
                    items.checked_add(outcome.retained_items()?)
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
        limits: &crate::limits::Limits,
    ) -> Self {
        Self {
            contract,
            usage: Usage { items: 0, bytes: 0 },
            item_limit: limits.classified_contract_items,
            byte_limit: limits.classified_contract_bytes,
        }
    }

    pub(crate) fn new(
        contract: fingerprint::Fingerprint,
        usage: Usage,
        limits: &crate::limits::Limits,
    ) -> Result<Self, crate::surface::Error> {
        let allocation = Self {
            contract,
            usage: Usage { items: 0, bytes: 0 },
            item_limit: limits.classified_contract_items,
            byte_limit: limits.classified_contract_bytes,
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

/// `outcome`, or the associated outcome below it, that binds `identity`:
/// a member of a type, or of the opaque type a trait's method gives, which
/// is below that method (`classify::method_opaque_implementations`).
fn binding_outcome<'contract>(
    outcome: crate::classify::Outcome<'contract>,
    identity: &crate::surface::identity::Identity,
) -> Option<crate::classify::Outcome<'contract>> {
    if outcome.identity().same_binding(identity)
        && outcome
            .representability()
            .contains_selector(outcome.identity().selector(), identity.selector())
    {
        return Some(outcome);
    }
    outcome
        .implementations()
        .flat_map(crate::classify::ImplementationOutcome::associated)
        .find_map(|associated| binding_outcome(associated, identity))
}

#[cfg(test)]
mod tests {
    use super::{fingerprint, Allocation, Usage};

    const CONTRACT: fingerprint::Fingerprint = fingerprint::Fingerprint([0x43; 32]);

    #[test]
    fn allocation_seeds_the_exact_classified_contract_authority_atomically() {
        let limits = crate::limits::Limits::standard();
        let exact = Usage {
            items: limits.classified_contract_items,
            bytes: limits.classified_contract_bytes,
        };
        let allocation = Allocation::new(CONTRACT, exact, &limits)
            .expect("the exact classified Contract boundary is accepted");
        assert_eq!(allocation.usage(), (exact.items, exact.bytes));
        assert_eq!(allocation.limits(), (exact.items, exact.bytes));
    }

    #[test]
    fn classified_contract_allocation_is_isolated_from_unrelated_authorities() {
        let standard = crate::limits::Limits::standard();
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
    }

    #[test]
    fn later_retain_remains_one_atomic_cumulative_transaction() {
        let limits = crate::limits::Limits::standard();
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
        allocation
            .retain(usize::MAX, 0)
            .expect_err("checked overflow refuses");
        assert_eq!(allocation.usage(), retained);
    }
}
