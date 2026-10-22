//! Phase-indexed canonical normal-build Rust API evidence.

use std::io::{self, Write};

use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};

#[cfg(feature = "rustc-driver")]
use crate::cargo::{self, unit};
use crate::{Error, Result};

pub(crate) mod decode;
pub mod identity;
pub mod ledger;
pub mod model;
pub(crate) mod semantic;

#[cfg(feature = "rustc-driver")]
pub(crate) mod cached;
#[cfg(feature = "rustc-driver")]
pub(crate) mod memo;
#[cfg(feature = "rustc-driver")]
pub(crate) mod parts;
#[cfg(feature = "rustc-driver")]
pub(crate) mod retained;
#[cfg(feature = "rustc-driver")]
pub use retained::Continuation;

const CANONICALIZER_VERSION: u32 = 13;

/// Uninhabited marker proving compiler sidecars were captured and canonicalized.
pub enum Captured {}

/// Uninhabited marker reserved for freshness-proven normal-build extraction.
///
/// ```compile_fail
/// let _ = rawr_import::api::Extracted {};
/// ```
pub enum Extracted {}

mod phase {
    pub trait Sealed {}
}

/// A closed compiler-evidence phase used by [`Graph`].
pub trait Phase: phase::Sealed {
    #[doc(hidden)]
    type Evidence;
}

impl phase::Sealed for Captured {}
impl Phase for Captured {
    type Evidence = ();
}

impl phase::Sealed for Extracted {}
impl Phase for Extracted {
    type Evidence = FreshEvidence;
}

/// Fresh input proof plus the optional current content-addressed API receipt.
#[doc(hidden)]
pub struct FreshEvidence {
    input: crate::input::proof::Proof,
    chain: Option<crate::node::Chain>,
}

/// A SHA-256 identity of canonical semantic API evidence.
///
/// ```compile_fail
/// let _ = rawr_import::api::Fingerprint([0; 32]);
/// ```
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct Fingerprint([u8; 32]);

/// A phase-indexed immutable API proof bundle.
///
/// ```compile_fail
/// let _ = rawr_import::api::Graph::<rawr_import::api::Extracted> {};
/// ```
///
/// ```compile_fail
/// # fn clone_graph(graph: &rawr_import::api::Graph<rawr_import::api::Extracted>) {
/// let _: rawr_import::api::Graph<rawr_import::api::Extracted> = graph.clone();
/// # }
/// ```
pub struct Graph<P: Phase> {
    invocations: Box<[Invocation]>,
    definitions: Box<[model::Definition]>,
    exports: Box<[ledger::Path]>,
    implementations: Box<[model::Implementation]>,
    scalar_predicate_proofs: Box<[model::ScalarPredicateProof]>,
    support: Box<[ledger::Support]>,
    anonymous: Box<[ledger::AnonymousImport]>,
    /// Each demanded dependency's name index, outside the fingerprint.
    index: Box<[ledger::Index]>,
    canonical: Box<[u8]>,
    fingerprint: Fingerprint,
    semantic_fingerprint: Fingerprint,
    stored: Option<StoredGraph>,
    universe: Option<[u8; 32]>,
    transaction: Usage,
    retained: Usage,
    evidence: P::Evidence,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct Usage {
    pub(crate) items: usize,
    pub(crate) bytes: usize,
}

/// Allocation-free recursive ownership audit for an immutable Graph.
///
/// Fixed-size values are charged by the containing slice or box. Every
/// semantic node and owned string is one retained item; dynamically owned
/// storage contributes its exact byte length.
pub(crate) struct Retained {
    items: usize,
    bytes: usize,
    category: RetainedCategory,
    categories: [Usage; RetainedCategory::COUNT],
}

#[derive(Clone, Copy)]
pub(crate) enum RetainedCategory {
    Context,
    Invocation,
    Definition,
    Kind,
    Type,
    Predicate,
    Generic,
    Instantiation,
    Export,
    Implementation,
    Support,
    Anonymous,
    Canonical,
}

impl RetainedCategory {
    const COUNT: usize = 13;

    const fn index(self) -> usize {
        self as usize
    }
}

/// Fixed scalar decomposition of exact recursively retained Graph ownership.
#[cfg(feature = "rustc-driver")]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct RetainedEvidence {
    pub(crate) context: Usage,
    pub(crate) invocation: Usage,
    pub(crate) definition: Usage,
    pub(crate) kind: Usage,
    pub(crate) ty: Usage,
    pub(crate) predicate: Usage,
    pub(crate) generic: Usage,
    pub(crate) instantiation: Usage,
    pub(crate) export: Usage,
    pub(crate) implementation: Usage,
    pub(crate) support: Usage,
    pub(crate) anonymous: Usage,
    pub(crate) canonical: Usage,
    pub(crate) total: Usage,
}

#[cfg(feature = "rustc-driver")]
impl RetainedEvidence {
    pub(crate) fn category_sum(self) -> Option<Usage> {
        [
            self.context,
            self.invocation,
            self.definition,
            self.kind,
            self.ty,
            self.predicate,
            self.generic,
            self.instantiation,
            self.export,
            self.implementation,
            self.support,
            self.anonymous,
            self.canonical,
        ]
        .into_iter()
        .try_fold(Usage { items: 0, bytes: 0 }, |sum, category| {
            Some(Usage {
                items: sum.items.checked_add(category.items)?,
                bytes: sum.bytes.checked_add(category.bytes)?,
            })
        })
    }
}

impl Retained {
    const fn new() -> Self {
        Self {
            items: 0,
            bytes: 0,
            category: RetainedCategory::Context,
            categories: [Usage { items: 0, bytes: 0 }; RetainedCategory::COUNT],
        }
    }

    pub(crate) fn node(&mut self) -> Option<()> {
        self.items = self.items.checked_add(1)?;
        let category = &mut self.categories[self.category.index()];
        category.items = category.items.checked_add(1)?;
        Some(())
    }

    pub(crate) fn bytes(&mut self, bytes: usize) -> Option<()> {
        self.bytes = self.bytes.checked_add(bytes)?;
        let category = &mut self.categories[self.category.index()];
        category.bytes = category.bytes.checked_add(bytes)?;
        Some(())
    }

    pub(crate) fn category<T>(
        &mut self,
        category: RetainedCategory,
        retain: impl FnOnce(&mut Self) -> Option<T>,
    ) -> Option<T> {
        let previous = self.category;
        self.category = category;
        let result = retain(self);
        self.category = previous;
        result
    }

    pub(crate) fn string(&mut self, value: &str) -> Option<()> {
        self.node()?;
        self.bytes(value.len())
    }

    pub(crate) fn slice<T>(&mut self, values: &[T]) -> Option<()> {
        self.bytes(std::mem::size_of_val(values))
    }

    pub(crate) fn boxed<T>(&mut self) -> Option<()> {
        self.bytes(std::mem::size_of::<T>())
    }

    const fn usage(&self) -> Usage {
        Usage {
            items: self.items,
            bytes: self.bytes,
        }
    }

    fn categories_sum(&self) -> Option<Usage> {
        self.categories
            .iter()
            .try_fold(Usage { items: 0, bytes: 0 }, |sum, category| {
                Some(Usage {
                    items: sum.items.checked_add(category.items)?,
                    bytes: sum.bytes.checked_add(category.bytes)?,
                })
            })
    }

    #[cfg(feature = "rustc-driver")]
    fn evidence(&self) -> Option<RetainedEvidence> {
        let total = self.usage();
        if self.categories_sum()? != total {
            return None;
        }
        let category = |category: RetainedCategory| self.categories[category.index()];
        Some(RetainedEvidence {
            context: category(RetainedCategory::Context),
            invocation: category(RetainedCategory::Invocation),
            definition: category(RetainedCategory::Definition),
            kind: category(RetainedCategory::Kind),
            ty: category(RetainedCategory::Type),
            predicate: category(RetainedCategory::Predicate),
            generic: category(RetainedCategory::Generic),
            instantiation: category(RetainedCategory::Instantiation),
            export: category(RetainedCategory::Export),
            implementation: category(RetainedCategory::Implementation),
            support: category(RetainedCategory::Support),
            anonymous: category(RetainedCategory::Anonymous),
            canonical: category(RetainedCategory::Canonical),
            total,
        })
    }
}

/// Pre-policy semantic evidence encoded before selected-outcome filtering.
pub(crate) struct Raw {
    canonical: Vec<u8>,
    semantic: Vec<u8>,
    universe: Option<[u8; 32]>,
}

/// Capability-free graph bytes retained by the content store.
pub(crate) struct StoredGraph {
    canonical: Shared,
    selected: Shared,
    selected_usage: Usage,
}

pub(crate) const MAGIC: &[u8; 8] = b"RAWRAPI3";
pub(crate) const HEADER_BYTES: usize = 40;

/// Lay out captured semantic storage as its node payload: the header, the
/// semantic canonical bytes, then the selected ledger `write` appends.
fn stored(
    canonical: Vec<u8>,
    selected: usize,
    usage: Usage,
    write: impl FnOnce(&mut Vec<u8>) -> crate::Result<()>,
) -> crate::Result<StoredGraph> {
    let canonical_end = HEADER_BYTES + canonical.len();
    let mut encoded = Vec::new();
    encoded
        .try_reserve_exact(canonical_end.saturating_add(selected))
        .map_err(|source| crate::Error::Allocation {
            operation: "stored API graph",
            source,
        })?;
    encoded.extend_from_slice(&header(usage, canonical.len(), selected));
    encoded.extend_from_slice(&canonical);
    drop(canonical);
    write(&mut encoded)?;
    let end = encoded.len();
    let [canonical, selected] = Shared::split(
        encoded.into_boxed_slice(),
        [HEADER_BYTES..canonical_end, canonical_end..end],
    );
    Ok(StoredGraph {
        canonical,
        selected,
        selected_usage: usage,
    })
}

pub(crate) fn header(usage: Usage, canonical: usize, selected: usize) -> [u8; HEADER_BYTES] {
    let mut header = [0; HEADER_BYTES];
    header[..8].copy_from_slice(MAGIC);
    for (index, value) in [usage.items, usage.bytes, canonical, selected]
        .into_iter()
        .enumerate()
    {
        header[8 + 8 * index..16 + 8 * index].copy_from_slice(&(value as u64).to_le_bytes());
    }
    header
}

/// A range of one shared buffer, so a decoded store payload is viewed in
/// place rather than copied into fresh boxes.
pub(crate) struct Shared {
    buffer: std::sync::Arc<Box<[u8]>>,
    range: std::ops::Range<usize>,
}

impl Shared {
    pub(crate) fn split<const N: usize>(
        buffer: Box<[u8]>,
        ranges: [std::ops::Range<usize>; N],
    ) -> [Self; N] {
        let buffer = std::sync::Arc::new(buffer);
        ranges.map(|range| Self {
            buffer: std::sync::Arc::clone(&buffer),
            range,
        })
    }
}

impl From<Vec<u8>> for Shared {
    fn from(bytes: Vec<u8>) -> Self {
        let range = 0..bytes.len();
        let [shared] = Self::split(bytes.into_boxed_slice(), [range]);
        shared
    }
}

impl std::ops::Deref for Shared {
    type Target = [u8];
    fn deref(&self) -> &[u8] {
        &self.buffer[self.range.clone()]
    }
}

impl AsRef<[u8]> for Shared {
    fn as_ref(&self) -> &[u8] {
        self
    }
}

enum Semantic {
    Captured(Vec<u8>),
    Replayed(Fingerprint),
}

/// Canonical semantic evidence for one Cargo artifact and metadata-probe correlation.
#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub(crate) struct Invocation {
    package_id: Box<str>,
    root_aliases: Box<[Box<str>]>,
    target: Box<str>,
    target_kind: Box<str>,
    crate_types: Box<[Box<str>]>,
    crate_name: Box<str>,
    edition: Box<str>,
    compilation: Box<str>,
    host: Box<str>,
    selected_target: Box<str>,
    features: Box<[Box<str>]>,
    artifact: Artifact,
    compiler_commit: Box<str>,
    unit_correlation: Box<str>,
    unit_key: Box<str>,
    dependencies: Box<[Box<str>]>,
    sysroot: Box<[identity::Sysroot]>,
}

#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub(crate) struct Artifact {
    pub(crate) length: u64,
    pub(crate) digest: Box<str>,
    pub(crate) profile: Box<str>,
    pub(crate) fresh: bool,
    pub(crate) crate_hash: Box<str>,
    pub(crate) stable_crate_id: Box<str>,
}

/// Exact Cargo/toolchain inputs that contextualize one extracted graph.
#[derive(Deserialize, Serialize)]
pub struct Context {
    graph_schema: u32,
    probe_schema: u32,
    canonicalizer: u32,
    metadata: [u8; 32],
    build_unit: [u8; 32],
    toolchain_channel: Box<str>,
    compiler_commit: Box<str>,
    cargo_commit: Box<str>,
    host: Box<str>,
    target: Box<str>,
    profile: Box<str>,
    resolution_universe: [u8; 32],
    resolution_proof: [u8; 32],
    #[serde(skip)]
    final_lock: Option<[u8; 32]>,
    selections: Box<[Selection]>,
}

#[derive(Deserialize, Serialize)]
struct Selection {
    alias: Box<str>,
    package_id: Box<str>,
    roles: Box<[Box<str>]>,
}

#[cfg(any(test, feature = "test-fixtures"))]
#[derive(Serialize)]
struct Canonical<'a, C: Serialize, I: Serialize> {
    context: C,
    invocations: I,
    definitions: &'a [model::Definition],
    exports: &'a [ledger::Path],
    implementations: &'a [model::Implementation],
    scalar_predicate_proofs: &'a [model::ScalarPredicateProof],
    support: &'a [ledger::Support],
    anonymous: &'a [ledger::AnonymousImport],
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct StoredSelected {
    universe: Option<[u8; 32]>,
    definitions: Box<[model::Definition]>,
    exports: Box<[ledger::Path]>,
    implementations: Box<[model::Implementation]>,
    scalar_predicate_proofs: Box<[model::ScalarPredicateProof]>,
    support: Box<[ledger::Support]>,
    anonymous: Box<[ledger::AnonymousImport]>,
    index: Box<[ledger::Index]>,
}

#[derive(Serialize)]
struct Selected<'graph> {
    universe: Option<[u8; 32]>,
    definitions: &'graph [model::Definition],
    exports: &'graph [ledger::Path],
    implementations: &'graph [model::Implementation],
    scalar_predicate_proofs: &'graph [model::ScalarPredicateProof],
    support: &'graph [ledger::Support],
    anonymous: &'graph [ledger::AnonymousImport],
    index: &'graph [ledger::Index],
}

struct ExactCanonical<'expected> {
    expected: &'expected [u8],
    offset: usize,
    matches: bool,
}

impl std::io::Write for ExactCanonical<'_> {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        let end = self.offset.saturating_add(bytes.len());
        if self.expected.get(self.offset..end) != Some(bytes) {
            self.matches = false;
        }
        self.offset = end;
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

struct Measured {
    bytes: usize,
    limit: usize,
    overflow: Option<usize>,
}

impl<P: Phase> Graph<P> {
    /// Borrow the preselected Rawr-owned resolution universe identity, when present.
    #[doc(hidden)]
    #[must_use]
    pub const fn resolution_universe(&self) -> Option<&[u8; 32]> {
        self.universe.as_ref()
    }

    /// The number of exactly correlated normal-build compiler invocations.
    pub fn invocation_count(&self) -> usize {
        self.invocations.len()
    }

    /// Canonical definitions, each present exactly once.
    pub fn definitions(&self) -> impl ExactSizeIterator<Item = &model::Definition> {
        self.definitions.iter()
    }

    /// Canonically ordered selected-path outcomes.
    pub fn exports(&self) -> impl ExactSizeIterator<Item = &ledger::Path> {
        self.exports.iter()
    }

    /// Canonically ordered relevant implementation edges.
    pub fn implementations(&self) -> impl ExactSizeIterator<Item = &model::Implementation> {
        self.implementations.iter()
    }

    /// Canonically ordered completed closed-scalar predicate queries.
    pub fn scalar_predicate_proofs(
        &self,
    ) -> impl ExactSizeIterator<Item = &model::ScalarPredicateProof> {
        self.scalar_predicate_proofs.iter()
    }

    /// Canonically ordered transitive-support outcomes.
    pub fn support(&self) -> impl ExactSizeIterator<Item = &ledger::Support> {
        self.support.iter()
    }

    /// The name index of each dependency extracted from demand.
    pub fn name_index(&self) -> impl ExactSizeIterator<Item = &ledger::Index> {
        self.index.iter()
    }

    /// Canonical compiler-authenticated imports that intentionally bind no name.
    pub fn anonymous_imports(&self) -> impl ExactSizeIterator<Item = &ledger::AnonymousImport> {
        self.anonymous.iter()
    }

    /// The fingerprint of the exact current canonical graph.
    pub const fn fingerprint(&self) -> Fingerprint {
        self.fingerprint
    }

    pub(crate) const fn semantic_fingerprint(&self) -> Fingerprint {
        self.semantic_fingerprint
    }

    /// Existing callback-visible exact evidence and accounting, without changing
    /// when a callback runs or certifying its treatment of external inputs.
    pub(crate) fn callback_observations(
        &self,
        input: crate::input::fingerprint::Fingerprint,
    ) -> [u8; 96] {
        let mut bytes = [0; 96];
        bytes[..32].copy_from_slice(self.fingerprint.as_bytes());
        bytes[32..64].copy_from_slice(input.as_bytes());
        for (slot, value) in bytes[64..].as_chunks_mut::<8>().0.iter_mut().zip([
            self.retained.items,
            self.retained.bytes,
            self.transaction.items,
            self.transaction.bytes,
        ]) {
            slot.copy_from_slice(&u64::try_from(value).unwrap_or(u64::MAX).to_le_bytes());
        }
        bytes
    }

    /// Borrow the exact current canonical encoding.
    pub fn canonical_bytes(&self) -> &[u8] {
        &self.canonical
    }

    pub(crate) const fn allocation(&self) -> Usage {
        self.retained
    }

    pub(crate) const fn transaction(&self) -> Usage {
        self.transaction
    }

    /// Inspect the sealed Phase 1 allocation seed at the driver audit boundary.
    #[cfg(feature = "rustc-driver")]
    #[doc(hidden)]
    #[must_use]
    pub const fn driver_retained_usage(&self) -> (usize, usize) {
        (self.retained.items, self.retained.bytes)
    }

    /// Inspect the distinct cumulative canonical transaction proof.
    #[cfg(feature = "rustc-driver")]
    #[doc(hidden)]
    #[must_use]
    pub const fn driver_transaction_usage(&self) -> (usize, usize) {
        let usage = self.transaction();
        (usage.items, usage.bytes)
    }

    /// Independently recount the recursively retained immutable Graph.
    #[cfg(feature = "rustc-driver")]
    #[doc(hidden)]
    #[must_use]
    pub fn driver_recount_retained_usage(&self) -> Option<(usize, usize)> {
        let usage = self.recount_retained()?.usage();
        Some((usage.items, usage.bytes))
    }

    #[cfg(feature = "rustc-driver")]
    pub(crate) fn retained_evidence(&self) -> Option<RetainedEvidence> {
        self.recount_retained()?.evidence()
    }

    fn recount_retained(&self) -> Option<Retained> {
        let mut retained = Retained::new();
        retained.node()?;
        retained.category(RetainedCategory::Invocation, |retained| {
            retained.slice(&self.invocations)?;
            for invocation in &self.invocations {
                invocation.retain(retained)?;
            }
            Some(())
        })?;
        retained.category(RetainedCategory::Definition, |retained| {
            retained.slice(&self.definitions)?;
            for definition in &self.definitions {
                definition.retain(retained)?;
            }
            Some(())
        })?;
        retained.category(RetainedCategory::Export, |retained| {
            retained.slice(&self.exports)?;
            for export in &self.exports {
                export.retain(retained)?;
            }
            retained.slice(&self.index)?;
            for index in &self.index {
                index.retain(retained)?;
            }
            Some(())
        })?;
        retained.category(RetainedCategory::Implementation, |retained| {
            retained.slice(&self.implementations)?;
            for implementation in &self.implementations {
                implementation.retain(retained)?;
            }
            Some(())
        })?;
        retained.category(RetainedCategory::Predicate, |retained| {
            retained.slice(&self.scalar_predicate_proofs)?;
            for proof in &self.scalar_predicate_proofs {
                proof.retain(retained)?;
            }
            Some(())
        })?;
        retained.category(RetainedCategory::Support, |retained| {
            retained.slice(&self.support)?;
            for support in &self.support {
                support.retain(retained)?;
            }
            Some(())
        })?;
        retained.category(RetainedCategory::Anonymous, |retained| {
            retained.slice(&self.anonymous)?;
            for anonymous in &self.anonymous {
                anonymous.retain(retained)?;
            }
            Some(())
        })?;
        retained.category(RetainedCategory::Canonical, |retained| {
            retained.node()?;
            retained.bytes(self.canonical.len())?;
            if let Some(stored) = &self.stored {
                retained.node()?;
                retained.bytes(stored.canonical.len())?;
                retained.node()?;
                retained.bytes(stored.selected.len())?;
            }
            Some(())
        })?;
        if retained.categories_sum()? != retained.usage() {
            return None;
        }
        Some(retained)
    }

    fn selected_usage(&self) -> Result<Usage> {
        let retained = self.recount_retained().ok_or_else(|| Error::Extraction {
            code: "api-ownership-overflow",
            reason: "recursive API ownership overflowed".into(),
        })?;
        let invocations = retained.categories[RetainedCategory::Invocation.index()];
        let canonical = retained.categories[RetainedCategory::Canonical.index()];
        Ok(Usage {
            items: retained.items - invocations.items - canonical.items,
            bytes: retained.bytes - invocations.bytes - canonical.bytes,
        })
    }

    #[cfg(test)]
    pub(crate) fn with_transaction_activity_for_test(mut self, items: usize, bytes: usize) -> Self {
        self.transaction = Usage { items, bytes };
        self
    }
}

impl Fingerprint {
    pub(crate) const fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    /// Borrow the exact SHA-256 bytes.
    pub const fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

impl Graph<Captured> {
    #[cfg(test)]
    pub(crate) fn raw(canonical: Vec<u8>, universe: Option<[u8; 32]>) -> Raw {
        Raw {
            semantic: canonical.clone(),
            canonical,
            universe,
        }
    }

    pub(crate) const fn raw_pair(
        canonical: Vec<u8>,
        semantic: Vec<u8>,
        universe: Option<[u8; 32]>,
    ) -> Raw {
        Raw {
            canonical,
            semantic,
            universe,
        }
    }

    pub(crate) fn take_stored(&mut self) -> Result<StoredGraph> {
        let stored = self.stored.take().ok_or_else(|| Error::Extraction {
            code: "semantic-api-storage",
            reason: "captured semantic API storage was already consumed".into(),
        })?;
        self.retained = self
            .recount_retained()
            .ok_or_else(|| Error::Extraction {
                code: "semantic-api-retained",
                reason: "retained API accounting overflowed".into(),
            })?
            .usage();
        Ok(stored)
    }

    #[cfg(all(test, feature = "rustc-driver"))]
    pub(crate) fn into_stored(mut self, limit: usize) -> Result<StoredGraph> {
        let stored = self.take_stored()?;
        limited_graph_usage(
            "stored API graph bytes",
            limit,
            stored.canonical.len().saturating_add(stored.selected.len()),
        )?;
        Ok(stored)
    }

    #[cfg(feature = "rustc-driver")]
    pub(crate) fn from_stored(
        stored: &StoredGraph,
        context: &Context,
        invocations: Vec<Invocation>,
        limits: &cargo::Limits,
        allocation: &mut crate::extract::Allocation,
    ) -> Result<Self> {
        (|| {
            limited_graph_usage(
                "stored API graph bytes",
                limits.node_canonical_bytes(),
                stored.canonical.len().saturating_add(stored.selected.len()),
            )?;
            // The record's retained-ownership audit is untrusted until recount.
            // Independently admit the decoder's peak, then debit conservative
            // decoder work before serde can allocate (also on malformed input).
            // This replaces, rather than adds to, the old header-based debit.
            allocation.exceeded("sidecar bytes", limits.json_bytes, stored.selected.len());
            // The scan folds the targets it crossed into this canonical
            // transaction on every exit, including a scan that then fails.
            let decode = crate::extract::protocol::selected_preflight_accounted(
                &stored.selected,
                limits,
                allocation,
            )?;
            allocation.admit_decode(decode.items, decode.admission_bytes)?;
            allocation.items(decode.items)?;
            allocation.bytes(decode.bytes)?;
            let selected: StoredSelected =
                serde_json::from_slice(&stored.selected).map_err(|error| Error::Extraction {
                    code: "stored-selected-schema",
                    reason: error.to_string().into_boxed_str(),
                })?;
            let mut exact = ExactCanonical {
                expected: &stored.selected,
                offset: 0,
                matches: true,
            };
            serde_json::to_writer(&mut exact, &selected).map_err(|error| Error::Extraction {
                code: "stored-selected-encoding",
                reason: error.to_string().into_boxed_str(),
            })?;
            if !exact.matches || exact.offset != stored.selected.len() {
                return Err(Error::Extraction {
                    code: "stored-selected-noncanonical",
                    reason: "stored selected graph encoding differs".into(),
                });
            }
            if selected.universe != context.resolution_universe() {
                return Err(Error::Extraction {
                    code: "stored-api-universe",
                    reason: "current API universe differs".into(),
                });
            }
            let canonical = semantic::current(
                &stored.canonical,
                context,
                &invocations,
                limits.node_canonical_bytes(),
                allocation,
            )?;
            let graph = Self::seal_canonical(
                invocations,
                selected.definitions.into_vec(),
                selected.exports.into_vec(),
                selected.implementations.into_vec(),
                selected.scalar_predicate_proofs.into_vec(),
                selected.support.into_vec(),
                selected.anonymous.into_vec(),
                selected.index.into_vec(),
                canonical,
                Semantic::Replayed(Fingerprint(Sha256::digest(&stored.canonical).into())),
                selected.universe,
                allocation,
            )?;
            if graph.selected_usage()? != stored.selected_usage {
                return Err(Error::Extraction {
                    code: "stored-api-ownership",
                    reason: "semantic API ownership audit differs".into(),
                });
            }
            #[cfg(feature = "import-baseline")]
            crate::metrics::record_scalar(
                crate::metrics::accounting::Scalar::ApiSelectedBytes,
                stored.selected.len() as u64,
            );
            Ok(graph)
        })()
        .map_err(|error| allocation.retained_failure(error))
    }

    #[cfg(any(test, feature = "test-fixtures"))]
    #[allow(
        clippy::too_many_arguments,
        reason = "sealing transforms all canonical ledgers and their allocation proof together"
    )]
    pub(crate) fn seal(
        context: &Context,
        invocations: Vec<Invocation>,
        definitions: Vec<model::Definition>,
        exports: Vec<ledger::Path>,
        implementations: Vec<model::Implementation>,
        scalar_predicate_proofs: Vec<model::ScalarPredicateProof>,
        support: Vec<ledger::Support>,
        anonymous: Vec<ledger::AnonymousImport>,
        byte_limit: usize,
        allocation: &mut crate::extract::Allocation,
    ) -> Result<Self> {
        let mut measured = Measured {
            bytes: 0,
            limit: byte_limit,
            overflow: None,
        };
        let result = serde_json::to_writer(
            &mut measured,
            &Canonical {
                context,
                invocations: &invocations,
                definitions: &definitions,
                exports: &exports,
                implementations: &implementations,
                scalar_predicate_proofs: &scalar_predicate_proofs,
                support: &support,
                anonymous: &anonymous,
            },
        );
        if let Some(actual) = measured.overflow {
            return Err(Error::Limit {
                resource: "canonical graph bytes",
                limit: byte_limit,
                actual,
            });
        }
        result.map_err(|error| Error::Extraction {
            code: "canonical-encoding",
            reason: error.to_string().into_boxed_str(),
        })?;
        allocation.items(1)?;
        allocation.bytes(measured.bytes)?;
        let mut canonical = Vec::new();
        canonical
            .try_reserve_exact(measured.bytes)
            .map_err(|source| {
                allocation.failure(Error::Allocation {
                    operation: "canonical graph allocation",
                    source,
                })
            })?;
        serde_json::to_writer(
            &mut canonical,
            &Canonical {
                context,
                invocations: &invocations,
                definitions: &definitions,
                exports: &exports,
                implementations: &implementations,
                scalar_predicate_proofs: &scalar_predicate_proofs,
                support: &support,
                anonymous: &anonymous,
            },
        )
        .map_err(|error| Error::Extraction {
            code: "canonical-encoding",
            reason: error.to_string().into_boxed_str(),
        })?;
        if canonical.len() != measured.bytes {
            return Err(Error::Extraction {
                code: "canonical-measurement",
                reason: "canonical graph changed between measurement and encoding".into(),
            });
        }
        let semantic = semantic::encode(
            &Canonical {
                context: semantic::Context(context),
                invocations: semantic::Invocations(&invocations),
                definitions: &definitions,
                exports: &exports,
                implementations: &implementations,
                scalar_predicate_proofs: &scalar_predicate_proofs,
                support: &support,
                anonymous: &anonymous,
            },
            byte_limit,
            allocation,
        )?;
        Self::seal_canonical(
            invocations,
            definitions,
            exports,
            implementations,
            scalar_predicate_proofs,
            support,
            anonymous,
            Vec::new(),
            canonical,
            Semantic::Captured(semantic),
            context.resolution_universe(),
            allocation,
        )
    }

    #[allow(
        clippy::too_many_arguments,
        reason = "sealing transforms the selected ledgers with their independent raw proof"
    )]
    pub(crate) fn seal_with_raw(
        invocations: Vec<Invocation>,
        definitions: Vec<model::Definition>,
        exports: Vec<ledger::Path>,
        implementations: Vec<model::Implementation>,
        scalar_predicate_proofs: Vec<model::ScalarPredicateProof>,
        support: Vec<ledger::Support>,
        anonymous: Vec<ledger::AnonymousImport>,
        index: Vec<ledger::Index>,
        raw: Raw,
        allocation: &mut crate::extract::Allocation,
    ) -> Result<Self> {
        let Raw {
            canonical,
            semantic,
            universe,
        } = raw;
        Self::seal_canonical(
            invocations,
            definitions,
            exports,
            implementations,
            scalar_predicate_proofs,
            support,
            anonymous,
            index,
            canonical,
            Semantic::Captured(semantic),
            universe,
            allocation,
        )
    }

    fn seal_canonical(
        invocations: Vec<Invocation>,
        definitions: Vec<model::Definition>,
        exports: Vec<ledger::Path>,
        implementations: Vec<model::Implementation>,
        scalar_predicate_proofs: Vec<model::ScalarPredicateProof>,
        support: Vec<ledger::Support>,
        anonymous: Vec<ledger::AnonymousImport>,
        index: Vec<ledger::Index>,
        canonical: Vec<u8>,
        semantic: Semantic,
        universe: Option<[u8; 32]>,
        allocation: &mut crate::extract::Allocation,
    ) -> Result<Self> {
        let semantic_fingerprint = match &semantic {
            Semantic::Captured(bytes) => Fingerprint(Sha256::digest(bytes).into()),
            Semantic::Replayed(fingerprint) => *fingerprint,
        };
        let fingerprint = Fingerprint(Sha256::digest(&canonical).into());
        let mut graph = Self {
            invocations: invocations.into_boxed_slice(),
            definitions: definitions.into_boxed_slice(),
            exports: exports.into_boxed_slice(),
            implementations: implementations.into_boxed_slice(),
            scalar_predicate_proofs: scalar_predicate_proofs.into_boxed_slice(),
            support: support.into_boxed_slice(),
            anonymous: anonymous.into_boxed_slice(),
            index: index.into_boxed_slice(),
            canonical: canonical.into_boxed_slice(),
            fingerprint,
            semantic_fingerprint,
            stored: None,
            universe,
            transaction: Usage { items: 0, bytes: 0 },
            retained: Usage { items: 0, bytes: 0 },
            evidence: (),
        };
        #[cfg(feature = "import-baseline")]
        crate::metrics::record_scalar(
            crate::metrics::accounting::Scalar::ApiCanonicalBytes,
            graph.canonical.len() as u64,
        );
        graph
            .validate_cardinality(allocation)
            .map_err(|error| allocation.failure(error))?;
        if let Semantic::Captured(canonical) = semantic {
            let selected = Selected {
                universe: graph.universe,
                definitions: &graph.definitions,
                exports: &graph.exports,
                implementations: &graph.implementations,
                scalar_predicate_proofs: &graph.scalar_predicate_proofs,
                support: &graph.support,
                anonymous: &graph.anonymous,
                index: &graph.index,
            };
            // Physical: the selected ledger is stored as node content. Its
            // size is measured against the canonical byte target by
            // `allocation.bytes` inside `measure`, which only records.
            let selected =
                semantic::encode_once(&selected, crate::node::MAX_CONTENT_BYTES, allocation)?;
            semantic::admit(selected.len(), crate::node::MAX_CONTENT_BYTES, allocation)?;
            let selected_usage = graph.selected_usage()?;
            let stored = stored(canonical, selected.len(), selected_usage, |output| {
                output.extend_from_slice(&selected);
                Ok(())
            })?;
            graph.stored = Some(stored);
        }
        let (items, bytes) = allocation.usage();
        graph.transaction = Usage { items, bytes };
        graph.retained = graph
            .recount_retained()
            .ok_or_else(|| {
                allocation.retained_failure(Error::Limit {
                    resource: "retained graph usage",
                    limit: usize::MAX,
                    actual: usize::MAX,
                })
            })?
            .usage();
        allocation.validate_retained(graph.retained.items, graph.retained.bytes)?;
        #[cfg(feature = "import-baseline")]
        if let Some(stored) = &graph.stored {
            crate::metrics::record_scalar(
                crate::metrics::accounting::Scalar::ApiSelectedBytes,
                stored.selected.len() as u64,
            );
        }
        Ok(graph)
    }

    pub(crate) fn publish(self, input: crate::input::proof::Proof) -> Result<Graph<Extracted>> {
        self.publish_with_chain(input, None)
    }

    pub(crate) fn publish_with_chain(
        self,
        input: crate::input::proof::Proof,
        chain: Option<crate::node::Chain>,
    ) -> Result<Graph<Extracted>> {
        Ok(Graph {
            invocations: self.invocations,
            definitions: self.definitions,
            exports: self.exports,
            implementations: self.implementations,
            scalar_predicate_proofs: self.scalar_predicate_proofs,
            support: self.support,
            anonymous: self.anonymous,
            index: self.index,
            canonical: self.canonical,
            fingerprint: self.fingerprint,
            semantic_fingerprint: self.semantic_fingerprint,
            stored: self.stored,
            universe: self.universe,
            transaction: self.transaction,
            retained: self.retained,
            evidence: FreshEvidence { input, chain },
        })
    }

    fn validate_cardinality(&self, allocation: &mut crate::extract::Allocation) -> Result<()> {
        let mut invocations = Vec::new();
        allocation.temporary_conversions(self.invocations.len());
        allocation.reserve(&mut invocations, self.invocations.len())?;
        for invocation in &self.invocations {
            invocations.push(invocation.unit_key.as_ref());
        }
        allocation.sort_keys(invocations.len());
        invocations.sort_unstable();
        if invocations.windows(2).any(|pair| pair[0] == pair[1]) {
            return Err(Error::Extraction {
                code: "invocation-ledger-cardinality",
                reason: "a canonical compiler unit occurs more than once".into(),
            });
        }
        if self
            .definitions
            .windows(2)
            .any(|pair| pair[0].id() >= pair[1].id())
        {
            return Err(Error::Extraction {
                code: "definition-ledger-cardinality",
                reason: "canonical definitions are duplicated or out of identity order".into(),
            });
        }
        let mut paths = Vec::new();
        allocation.temporary_conversions(self.exports.len());
        allocation.reserve(&mut paths, self.exports.len())?;
        for path in &self.exports {
            paths.push(path.key());
        }
        allocation.sort_keys(paths.len());
        paths.sort_unstable();
        if paths.windows(2).any(|pair| pair[0] == pair[1]) {
            return Err(Error::Extraction {
                code: "export-ledger-cardinality",
                reason: "a selected public path has more than one outcome".into(),
            });
        }
        if self.anonymous.windows(2).any(|pair| pair[0] >= pair[1]) {
            return Err(Error::Extraction {
                code: "anonymous-import-ledger-cardinality",
                reason: "anonymous imports are duplicated or out of canonical order".into(),
            });
        }
        for import in &self.anonymous {
            if import.binding_path().len() == 0
                || import.aliases().len() == 0
                || !matches!(
                    import.reexports().last(),
                    Some(ledger::Reexport::Identified {
                        kind: ledger::ReexportKind::Single,
                        ..
                    })
                )
            {
                return Err(Error::Extraction {
                    code: "anonymous-import-ledger-evidence",
                    reason: import.target().def_path_hash().into(),
                });
            }
        }
        for path in &self.exports {
            if let ledger::Path::Imported(export) = path
                && self
                    .definitions
                    .binary_search_by(|definition| definition.id().cmp(export.target()))
                    .is_err()
            {
                return Err(Error::Extraction {
                    code: "imported-definition-cardinality",
                    reason: export.target().def_path_hash().into(),
                });
            }
        }
        let mut support = Vec::new();
        allocation.temporary_conversions(self.support.len());
        allocation.reserve(&mut support, self.support.len())?;
        for outcome in &self.support {
            support.push((outcome.id(), outcome.provenance(), outcome.role()));
        }
        allocation.sort_keys(support.len());
        support.sort_unstable();
        if support.windows(2).any(|pair| pair[0] == pair[1]) {
            return Err(Error::Extraction {
                code: "support-ledger-cardinality",
                reason: "a support edge/role has more than one outcome".into(),
            });
        }
        for outcome in &self.support {
            if matches!(outcome, ledger::Support::Complete { .. })
                && self
                    .definitions
                    .binary_search_by(|definition| definition.id().cmp(outcome.id()))
                    .is_err()
            {
                return Err(Error::Extraction {
                    code: "complete-support-cardinality",
                    reason: outcome.id().def_path_hash().into(),
                });
            }
            if !retains_support_provenance(outcome, &self.definitions, &self.implementations) {
                return Err(Error::Extraction {
                    code: "support-provenance-cardinality",
                    reason: outcome.provenance().def_path_hash().into(),
                });
            }
        }
        if self
            .implementations
            .windows(2)
            .any(|pair| pair[0].id() >= pair[1].id())
        {
            return Err(Error::Extraction {
                code: "implementation-ledger-cardinality",
                reason: "canonical implementations are duplicated or out of identity order".into(),
            });
        }
        if self
            .scalar_predicate_proofs
            .windows(2)
            .any(|pair| pair[0] >= pair[1])
        {
            return Err(Error::Extraction {
                code: "scalar-predicate-proof-cardinality",
                reason: "scalar predicate proofs are duplicated or out of canonical order".into(),
            });
        }
        for proof in &self.scalar_predicate_proofs {
            if self
                .definitions
                .binary_search_by(|definition| definition.id().cmp(proof.owner()))
                .is_err()
                || self
                    .definitions
                    .binary_search_by(|definition| definition.id().cmp(proof.trait_id()))
                    .is_err()
            {
                return Err(Error::Extraction {
                    code: "scalar-predicate-proof-owner",
                    reason: proof.owner().def_path_hash().into(),
                });
            }
            if let model::ScalarPredicateOutcome::Positive { implementation } = proof.outcome()
                && self
                    .implementations
                    .binary_search_by(|candidate| candidate.id().cmp(implementation))
                    .is_err()
            {
                return Err(Error::Extraction {
                    code: "scalar-predicate-proof-implementation",
                    reason: implementation.def_path_hash().into(),
                });
            }
        }
        for definition in &self.definitions {
            if let model::Owner::Implementation(owner) = definition.owner() {
                let Ok(index) = self
                    .implementations
                    .binary_search_by(|implementation| implementation.id().cmp(owner))
                else {
                    return Err(Error::Extraction {
                        code: "implementation-owner-cardinality",
                        reason: owner.def_path_hash().into(),
                    });
                };
                let implementation = &self.implementations[index];
                if implementation
                    .associated_slice()
                    .binary_search(definition.id())
                    .is_err()
                {
                    return Err(Error::Extraction {
                        code: "implementation-owner-associated",
                        reason: definition.id().def_path_hash().into(),
                    });
                }
            }
        }
        for implementation in &self.implementations {
            for associated in implementation.associated() {
                let Ok(index) = self
                    .definitions
                    .binary_search_by(|definition| definition.id().cmp(associated))
                else {
                    return Err(Error::Extraction {
                        code: "implementation-associated-cardinality",
                        reason: associated.def_path_hash().into(),
                    });
                };
                let definition = &self.definitions[index];
                if !implementation.retains_associated_owner(definition) {
                    return Err(Error::Extraction {
                        code: "implementation-associated-owner",
                        reason: associated.def_path_hash().into(),
                    });
                }
            }
        }
        Ok(())
    }
}

fn limited_graph_usage(resource: &'static str, limit: usize, actual: usize) -> Result<()> {
    if actual <= limit {
        Ok(())
    } else {
        Err(Error::Limit {
            resource,
            limit,
            actual,
        })
    }
}

fn retains_support_provenance(
    outcome: &ledger::Support,
    definitions: &[model::Definition],
    implementations: &[model::Implementation],
) -> bool {
    match outcome.role() {
        ledger::SupportRole::Definition => definitions
            .binary_search_by(|definition| definition.id().cmp(outcome.provenance()))
            .is_ok(),
        ledger::SupportRole::Implementation => implementations
            .binary_search_by(|implementation| implementation.id().cmp(outcome.provenance()))
            .is_ok(),
    }
}

impl Graph<Extracted> {
    /// The content identity of the exact normal-build inputs sealed at publication.
    pub const fn freshness_fingerprint(&self) -> crate::input::fingerprint::Fingerprint {
        self.evidence.input.fingerprint()
    }

    pub(crate) fn into_freshness_proof(self) -> crate::input::proof::Proof {
        self.evidence.input
    }

    /// Borrow the current content-addressed API parent, when this graph came
    /// through the shared DAG rather than a legacy or evidence-only path.
    #[doc(hidden)]
    #[must_use]
    pub const fn node_chain(&self) -> Option<&crate::node::Chain> {
        self.evidence.chain.as_ref()
    }

    #[cfg(any(test, feature = "test-fixtures"))]
    #[doc(hidden)]
    pub fn with_node_chain_for_test(mut self, chain: crate::node::Chain) -> Self {
        self.evidence.chain = Some(chain);
        self
    }
}

impl<P: Phase> Graph<P> {
    pub(crate) fn definition_index(&self, id: &identity::Definition) -> Option<usize> {
        self.definitions
            .binary_search_by(|definition| definition.id().cmp(id))
            .ok()
    }

    pub(crate) fn implementation_index(&self, id: &identity::Definition) -> Option<usize> {
        self.implementations
            .binary_search_by(|implementation| implementation.id().cmp(id))
            .ok()
    }
}

impl Write for Measured {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        let actual = self.bytes.saturating_add(buffer.len());
        if actual > self.limit {
            self.overflow = Some(actual);
            return Err(io::Error::other("canonical graph byte limit exceeded"));
        }
        self.bytes = actual;
        Ok(buffer.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl Invocation {
    #[allow(
        clippy::too_many_arguments,
        reason = "the proof-bundle constructor requires every correlated invocation authority"
    )]
    pub(crate) fn new(
        package_id: Box<str>,
        root_aliases: Box<[Box<str>]>,
        target: Box<str>,
        target_kind: Box<str>,
        crate_types: Box<[Box<str>]>,
        crate_name: Box<str>,
        edition: Box<str>,
        compilation: Box<str>,
        host: Box<str>,
        selected_target: Box<str>,
        features: Box<[Box<str>]>,
        artifact: Artifact,
        compiler_commit: Box<str>,
        unit_correlation: Box<str>,
        unit_key: Box<str>,
        dependencies: Box<[Box<str>]>,
        sysroot: Box<[identity::Sysroot]>,
    ) -> Self {
        Self {
            package_id,
            root_aliases,
            target,
            target_kind,
            crate_types,
            crate_name,
            edition,
            compilation,
            host,
            selected_target,
            features,
            artifact,
            compiler_commit,
            unit_correlation,
            unit_key,
            dependencies,
            sysroot,
        }
    }

    fn retain(&self, retained: &mut Retained) -> Option<()> {
        retained.node()?;
        retained.string(&self.package_id)?;
        retained.slice(&self.root_aliases)?;
        for value in &self.root_aliases {
            retained.string(value)?;
        }
        retained.string(&self.target)?;
        retained.string(&self.target_kind)?;
        retained.slice(&self.crate_types)?;
        for value in &self.crate_types {
            retained.string(value)?;
        }
        retained.string(&self.crate_name)?;
        retained.string(&self.edition)?;
        retained.string(&self.compilation)?;
        retained.string(&self.host)?;
        retained.string(&self.selected_target)?;
        retained.slice(&self.features)?;
        for value in &self.features {
            retained.string(value)?;
        }
        self.artifact.retain(retained)?;
        retained.string(&self.compiler_commit)?;
        retained.string(&self.unit_correlation)?;
        retained.string(&self.unit_key)?;
        retained.slice(&self.dependencies)?;
        for value in &self.dependencies {
            retained.string(value)?;
        }
        retained.slice(&self.sysroot)?;
        for sysroot in &self.sysroot {
            sysroot.retain(retained)?;
        }
        Some(())
    }
}

impl Artifact {
    fn retain(&self, retained: &mut Retained) -> Option<()> {
        retained.node()?;
        retained.string(&self.digest)?;
        retained.string(&self.profile)?;
        retained.string(&self.crate_hash)?;
        retained.string(&self.stable_crate_id)
    }
}

impl Context {
    #[cfg(feature = "rustc-driver")]
    pub(crate) fn from_resolution_parts(
        metadata: &unit::Metadata,
        build_unit: &unit::BuildGraph,
        selected: &[cargo::Selection],
        toolchain: &cargo::toolchain::Proof,
        target: &str,
        profile: &str,
        universe: Option<cargo::universe::Proof>,
        allocation: &mut crate::extract::Allocation,
    ) -> Result<Self> {
        let build_unit = build_unit.digest();
        allocation.items(1)?;
        let mut selections = Vec::new();
        allocation.reserve(&mut selections, selected.len())?;
        for selection in selected {
            allocation.items(1)?;
            let mut roles = Vec::new();
            let selected_roles = selection.roles();
            for role in selected_roles.iter() {
                allocation.reserve(&mut roles, 1)?;
                roles.push(allocation.owned(role.as_str())?);
            }
            selections.push(Selection {
                alias: allocation.owned(selection.alias())?,
                package_id: allocation.owned(selection.package_id())?,
                roles: roles.into_boxed_slice(),
            });
        }
        selections.sort_unstable_by(|left, right| left.alias.cmp(&right.alias));
        Ok(Self {
            graph_schema: toolchain.schemas().graph(),
            probe_schema: toolchain.schemas().probe(),
            canonicalizer: CANONICALIZER_VERSION,
            metadata: *metadata.digest().as_bytes(),
            build_unit: *build_unit.as_bytes(),
            toolchain_channel: allocation.owned(toolchain.channel())?,
            compiler_commit: allocation.owned(toolchain.rustc_commit())?,
            cargo_commit: allocation.owned(toolchain.cargo_commit())?,
            host: allocation.owned(toolchain.host())?,
            target: allocation.owned(target)?,
            profile: allocation.owned(profile)?,
            resolution_universe: universe.map_or([0; 32], |proof| *proof.base().as_bytes()),
            resolution_proof: universe.map_or([0; 32], |proof| *proof.complete().as_bytes()),
            final_lock: universe.map(|proof| *proof.final_lock().as_bytes()),
            selections: selections.into_boxed_slice(),
        })
    }

    #[cfg(any(test, feature = "test-fixtures"))]
    pub(crate) fn fixture() -> Self {
        Self::fixture_with_universe(None)
    }

    #[cfg(any(test, feature = "test-fixtures"))]
    pub(crate) fn fixture_with_universe(universe: Option<[u8; 32]>) -> Self {
        Self::fixture_with_universe_proof(universe, universe)
    }

    #[cfg(any(test, feature = "test-fixtures"))]
    fn fixture_with_universe_proof(universe: Option<[u8; 32]>, proof: Option<[u8; 32]>) -> Self {
        Self {
            graph_schema: 1,
            probe_schema: crate::extract::protocol::SCHEMA,
            canonicalizer: CANONICALIZER_VERSION,
            metadata: [6; 32],
            build_unit: [9; 32],
            toolchain_channel: "nightly-2026-07-11".into(),
            compiler_commit: "375b1431b7d89d1c2e2bc168c011848ae12b7d14".into(),
            cargo_commit: "59800466c5c41c444d264b1010b4d57e85a7117f".into(),
            host: "aarch64-apple-darwin".into(),
            target: "aarch64-apple-darwin".into(),
            profile: "dev".into(),
            resolution_universe: universe.unwrap_or([0; 32]),
            resolution_proof: proof.unwrap_or([0; 32]),
            final_lock: universe,
            selections: Box::default(),
        }
    }

    pub(crate) fn compiler_commit(&self) -> &str {
        &self.compiler_commit
    }

    pub(crate) fn host(&self) -> &str {
        &self.host
    }

    pub(crate) fn target(&self) -> &str {
        &self.target
    }

    pub(crate) const fn probe_schema(&self) -> u32 {
        self.probe_schema
    }

    pub(crate) fn selected_package(&self, alias: &str) -> Option<&str> {
        self.selections
            .iter()
            .find(|selection| selection.alias.as_ref() == alias)
            .map(|selection| selection.package_id.as_ref())
    }

    pub(crate) fn has_selections(&self) -> bool {
        !self.selections.is_empty()
    }

    pub(crate) fn resolution_universe(&self) -> Option<[u8; 32]> {
        (self.resolution_universe != [0; 32]).then_some(self.resolution_universe)
    }
}

#[cfg(all(test, feature = "rustc-driver"))]
mod selected_preflight_crossings {
    use super::{Captured, Context, Graph, StoredGraph, Usage};
    use crate::extract::Allocation;
    use crate::Error;

    /// A stored selected ledger that crosses a scanner target and then fails
    /// to scan keeps the crossing on the original canonical account.
    #[test]
    fn scan_failure_keeps_the_crossing_already_recorded() {
        let mut limits = crate::cargo::Limits::standard();
        limits.containers = 1;
        limits.string_bytes = 1;
        for (selected, axis) in [
            (&br#"[[],["ab","\"#[..], "sidecar containers"),
            (&br#"["abc","\"#[..], "sidecar string"),
            (&br#"[[],[]"#[..], "sidecar containers"),
        ] {
            let stored = StoredGraph {
                canonical: Vec::new().into(),
                selected: selected.to_vec().into(),
                selected_usage: Usage { items: 0, bytes: 0 },
            };
            let mut allocation = Allocation::new(&limits);
            let error = Graph::<Captured>::from_stored(
                &stored,
                &Context::fixture(),
                Vec::new(),
                &limits,
                &mut allocation,
            )
            .err()
            .expect("malformed input fails after the crossing");
            assert!(
                matches!(
                    error,
                    Error::Extraction {
                        code: "sidecar-schema",
                        ..
                    }
                ),
                "{error:?}"
            );
            assert!(
                allocation.exceedances().axis(axis).is_some(),
                "`{axis}` survives the failed scan"
            );
            assert_eq!(allocation.usage(), (0, 0));
            assert!(!allocation.is_refused());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{retains_support_provenance, Artifact, Context, Graph, Invocation};
    use crate::api::identity::{Authority, Compilation, Definition, Namespace, Package, Unit};
    use crate::api::ledger::{Support, SupportRole};
    use crate::extract::Allocation;
    use crate::Error;

    fn invocation(unit_key: &str) -> Invocation {
        Invocation::new(
            "root 0.1.0 (path+root)".into(),
            Box::from([Box::<str>::from("root")]),
            "/root/Cargo.toml:lib".into(),
            "lib".into(),
            Box::from([Box::<str>::from("rlib")]),
            "root".into(),
            "2024".into(),
            "target".into(),
            "aarch64-apple-darwin".into(),
            "aarch64-apple-darwin".into(),
            Box::default(),
            Artifact {
                length: 1,
                digest: "11".repeat(32).into_boxed_str(),
                profile: "dev".into(),
                fresh: false,
                crate_hash: "crate-hash".into(),
                stable_crate_id: "1111111111111111".into(),
            },
            "375b1431b7d89d1c2e2bc168c011848ae12b7d14".into(),
            "22".repeat(32).into_boxed_str(),
            unit_key.into(),
            Box::default(),
            Box::default(),
        )
    }

    fn identity(hash: &str) -> Definition {
        Definition::new(
            Unit::new(
                Authority::Cargo(Package::new(
                    "path+file:///fixture#fixture@0.1.0".into(),
                    "fixture".into(),
                    "0.1.0".into(),
                    "path+file:///fixture".into(),
                )),
                "fixture".into(),
                "lib".into(),
                "fixture".into(),
                Compilation::Target,
                Box::default(),
                "0000000000000001".into(),
            ),
            hash.into(),
            Namespace::Type,
        )
    }

    fn seal_with_context(
        context: &Context,
        allocation: &mut Allocation,
    ) -> crate::Result<Graph<super::Captured>> {
        Graph::seal(
            context,
            vec![invocation("unit-a"), invocation("unit-b")],
            Vec::new(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
            usize::MAX,
            allocation,
        )
    }

    fn seal(allocation: &mut Allocation) -> crate::Result<Graph<super::Captured>> {
        seal_with_context(&Context::fixture(), allocation)
    }

    #[test]
    fn cross_universe_context_changes_api_fingerprint() {
        let mut first_allocation = Allocation::with_limits(usize::MAX, usize::MAX);
        let first = seal_with_context(
            &Context::fixture_with_universe_proof(Some([1; 32]), Some([3; 32])),
            &mut first_allocation,
        )
        .expect("first universe graph seals");
        let mut second_allocation = Allocation::with_limits(usize::MAX, usize::MAX);
        let second = seal_with_context(
            &Context::fixture_with_universe_proof(Some([1; 32]), Some([4; 32])),
            &mut second_allocation,
        )
        .expect("changed completed proof graph seals");
        let mut third_allocation = Allocation::with_limits(usize::MAX, usize::MAX);
        let third = seal_with_context(
            &Context::fixture_with_universe_proof(Some([2; 32]), Some([4; 32])),
            &mut third_allocation,
        )
        .expect("changed base universe graph seals");

        assert_ne!(first.fingerprint(), second.fingerprint());
        assert_ne!(first.canonical_bytes(), second.canonical_bytes());
        assert_ne!(second.fingerprint(), third.fingerprint());
        assert_ne!(second.canonical_bytes(), third.canonical_bytes());
    }

    #[test]
    fn raw_canonical_sealing_preserves_the_context_universe_through_publication() {
        let expected = [7; 32];
        let raw = Graph::<super::Captured>::raw(
            b"{\"context\":{\"resolution_universe\":\"fixture\"}}".to_vec(),
            Some(expected),
        );
        let mut allocation = Allocation::with_limits(usize::MAX, usize::MAX);
        let captured = Graph::seal_with_raw(
            vec![invocation("unit-a"), invocation("unit-b")],
            Vec::new(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
            raw,
            &mut allocation,
        )
        .expect("raw canonical graph seals with its context universe");

        assert_eq!(captured.resolution_universe(), Some(&expected));
        let extracted = captured
            .publish(crate::input::proof::Proof::fixture())
            .expect("captured graph publishes");
        assert_eq!(extracted.resolution_universe(), Some(&expected));
    }

    #[test]
    fn seal_and_cardinality_temporaries_record_exact_cumulative_exceedances() {
        let mut measured = Allocation::with_limits(usize::MAX, usize::MAX);
        seal(&mut measured).expect("the measured graph seals");
        let (items, bytes) = measured.usage();

        let mut exact = Allocation::with_limits(items, bytes);
        seal(&mut exact).expect("the exact graph allocation boundary succeeds");
        assert!(!exact.exceedances().is_exceeded());

        // One over each cumulative target records a truthful high water
        // on the owning account and the seal continues.
        for (mut short, resource, target, actual) in [
            (
                Allocation::with_limits(items - 1, usize::MAX),
                "canonical allocation items",
                items - 1,
                items,
            ),
            (
                Allocation::with_limits(usize::MAX, bytes - 1),
                "canonical allocation bytes",
                bytes - 1,
                bytes,
            ),
        ] {
            seal(&mut short).expect("an accounting target never refuses the seal");
            assert_eq!(short.usage(), (items, bytes));
            assert!(!short.is_refused());
            let record = short.exceedances();
            let axis = record
                .axis(resource)
                .expect("the crossed target is recorded");
            assert_eq!(axis.target, target as u64);
            assert_eq!(axis.high_water, actual as u64);
        }
    }

    #[test]
    fn retained_graph_usage_is_history_independent_and_recounted() {
        let mut first = Allocation::with_limits(usize::MAX, usize::MAX);
        let first = seal(&mut first).expect("the first graph seals");

        let mut second = Allocation::with_limits(usize::MAX, usize::MAX);
        second.items(17).expect("history item charge fits");
        second.bytes(19).expect("history byte charge fits");
        let second = seal(&mut second).expect("the second graph seals");

        assert_eq!(first.fingerprint(), second.fingerprint());
        assert_eq!(first.retained.items, second.retained.items);
        assert_eq!(first.retained.bytes, second.retained.bytes);
        assert_eq!(
            first.recount_retained().map(|retained| retained.usage()),
            Some(first.retained)
        );
        assert_eq!(
            second.recount_retained().map(|retained| retained.usage()),
            Some(second.retained)
        );
        assert_ne!(first.transaction.items, second.transaction.items);
        assert_ne!(first.transaction.bytes, second.transaction.bytes);
    }

    #[cfg(feature = "rustc-driver")]
    #[test]
    fn retained_graph_categories_are_scalar_exact_and_allocation_free() {
        let mut allocation = Allocation::with_limits(usize::MAX, usize::MAX);
        let graph = seal(&mut allocation).expect("the retained graph is measured");
        let evidence = graph
            .retained_evidence()
            .expect("the scalar categories recount exactly");

        assert_eq!(evidence.category_sum(), Some(evidence.total));
        assert_eq!(evidence.total, graph.retained);
        assert!(evidence.context.items > 0);
        assert!(evidence.invocation.items > 0);
        assert!(evidence.canonical.items > 0);
        assert!(!std::mem::needs_drop::<super::RetainedEvidence>());
    }

    #[test]
    fn retained_graph_usage_records_exact_independent_exceedances() {
        let mut measured = Allocation::with_limits(usize::MAX, usize::MAX);
        let measured = seal(&mut measured).expect("the retained graph is measured");
        let retained = measured.retained;

        let mut exact = Allocation::with_limits(usize::MAX, usize::MAX)
            .with_retained_limits(retained.items, retained.bytes);
        seal(&mut exact).expect("the exact retained boundary succeeds");
        assert!(!exact.exceedances().is_exceeded());

        // Each retained target is independent; one over records only
        // its own axis and the measured graph is retained either way.
        for (mut short, resource, other, target, actual) in [
            (
                Allocation::with_limits(usize::MAX, usize::MAX)
                    .with_retained_limits(retained.items - 1, usize::MAX),
                "retained graph items",
                "retained graph bytes",
                retained.items - 1,
                retained.items,
            ),
            (
                Allocation::with_limits(usize::MAX, usize::MAX)
                    .with_retained_limits(usize::MAX, retained.bytes - 1),
                "retained graph bytes",
                "retained graph items",
                retained.bytes - 1,
                retained.bytes,
            ),
        ] {
            let graph = seal(&mut short).expect("a retained target never refuses the seal");
            assert_eq!(graph.retained, retained);
            let record = short.exceedances();
            let axis = record
                .axis(resource)
                .expect("the crossed target is recorded");
            assert_eq!(axis.target, target as u64);
            assert_eq!(axis.first_requested, actual as u64);
            assert_eq!(axis.high_water, actual as u64);
            assert!(record.axis(other).is_none());
        }
    }

    #[cfg(feature = "import-baseline")]
    #[test]
    fn preseal_retained_exceedance_keeps_original_transaction_and_retained_usage() {
        use crate::metrics::accounting::{Scalar, SemanticStage};

        let mut measured = Allocation::with_limits(usize::MAX, usize::MAX);
        let retained = seal(&mut measured).unwrap().retained;
        let recorder = crate::metrics::recorder::Recorder::transform_at(10, 1).unwrap();
        let mut allocation = Allocation::with_limits(usize::MAX, usize::MAX)
            .with_retained_limits(usize::MAX, retained.bytes - 1);
        let reached;
        {
            let _scope = recorder.scope().unwrap();
            // The retained byte target records and the seal continues.
            let graph = seal(&mut allocation).expect("a retained target never refuses");
            reached = allocation.usage();
            assert_eq!(graph.retained, retained);
            assert_eq!((graph.transaction.items, graph.transaction.bytes), reached);
            let record = allocation.exceedances();
            let axis = record
                .axis("retained graph bytes")
                .expect("the retained byte target is recorded");
            assert_eq!(axis.target, (retained.bytes - 1) as u64);
            assert_eq!(axis.high_water, retained.bytes as u64);
            assert!(record.axis("retained graph items").is_none());
            // Dropping the account reports its cumulative transaction usage.
            drop(allocation);
        }
        let evidence = recorder.snapshot().unwrap().accounting;
        for (scalar, expected) in [
            (Scalar::ApiTransactionItems, reached.0),
            (Scalar::ApiTransactionBytes, reached.1),
            (Scalar::ApiRetainedItems, retained.items),
            (Scalar::ApiRetainedBytes, retained.bytes),
        ] {
            assert_eq!(evidence.scalar(scalar).unwrap().max, expected as u64);
        }
        for stage in [SemanticStage::ApiRetained, SemanticStage::ApiTransaction] {
            assert_eq!(evidence.scalar(Scalar::SemanticRefusals(stage)), None);
            assert_eq!(
                evidence.scalar(Scalar::SemanticRefusedRequired(stage)),
                None
            );
            assert_eq!(evidence.scalar(Scalar::SemanticRefusedLimit(stage)), None);
        }
        assert!(evidence.scalar(Scalar::ApiSelectedBytes).is_some());
        assert!(evidence.scalar(Scalar::ApiCanonicalBytes).is_some());
    }

    #[test]
    fn duplicate_invocation_cardinality_rejects_without_a_tree_allocation() {
        let mut allocation = Allocation::with_limits(usize::MAX, usize::MAX);
        let result = Graph::seal(
            &Context::fixture(),
            vec![invocation("duplicate"), invocation("duplicate")],
            Vec::new(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
            usize::MAX,
            &mut allocation,
        );
        assert!(matches!(
            result,
            Err(Error::Extraction {
                code: "invocation-ledger-cardinality",
                ..
            })
        ));
    }

    #[test]
    fn support_role_rejects_orphan_definition_and_implementation_provenance() {
        let target = identity("target");
        let provenance = identity("orphan");
        for role in [SupportRole::Definition, SupportRole::Implementation] {
            let outcome = Support::Complete {
                id: target.clone(),
                provenance: provenance.clone(),
                role,
            };
            assert!(!retains_support_provenance(&outcome, &[], &[]));
            let rejected = Support::Rejected {
                id: target.clone(),
                provenance: provenance.clone(),
                role,
                reason: crate::api::ledger::Reason::UnresolvedDefinition(target.clone()),
                span: crate::api::model::Span::new("fixture.rs".into(), 0, 1, 1, 0, 1, 1),
            };
            assert!(!retains_support_provenance(&rejected, &[], &[]));
        }
    }

    #[test]
    fn support_ledger_does_not_promote_body_only_dependencies() {
        let body_only = identity("body-only-method");
        let surface = identity("surface-owner");
        let body_local_constant = identity("body-local-constant");
        let unrelated = identity("unrelated-provenance");
        let support = [
            Support::Complete {
                id: surface.clone(),
                provenance: body_only.clone(),
                role: SupportRole::Definition,
            },
            Support::Complete {
                id: identity("unrelated-target"),
                provenance: unrelated,
                role: SupportRole::Definition,
            },
        ];

        let mut body_edges = support
            .iter()
            .filter(|outcome| outcome.provenance() == &body_only);
        let edge = body_edges
            .next()
            .expect("the public method retains its shape owner");
        assert_eq!(edge.id(), &surface);
        assert!(body_edges.next().is_none());
        assert!(support
            .iter()
            .all(|outcome| outcome.id() != &body_local_constant));
    }
}
