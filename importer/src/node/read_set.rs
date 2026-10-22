use super::content::Edge;
use std::sync::Arc;

use sha2::{Digest as _, Sha256};

#[cfg(test)]
use super::Parent;
use super::{ActionKey, ContentRef, Digest, Direct, Discoverable, Error, Kind, Value};

const MAX_ENTRIES: usize = 65_536;
#[cfg(test)]
const MAX_DECODE_BYTES: u64 = 32 * 1024 * 1024;
#[cfg(test)]
const MAX_HASH_BYTES: u64 = 8 * 1024 * 1024 * 1024;

pub(crate) type Validator<K> =
    Arc<dyn Fn(&K, &super::GraphBudget) -> Result<Box<[u8]>, Error> + Send + Sync + 'static>;

/// The physical obligation at this exact validation boundary. Both scopes
/// always validate current behavior-affecting inputs.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Scope {
    Consumer,
    Complete,
}

pub(crate) type ScopedValidator<K> =
    Arc<dyn Fn(&K, &super::GraphBudget, Scope) -> Result<Box<[u8]>, Error> + Send + Sync + 'static>;

/// Launch-local exact union of current node and transitive parent recipes.
#[derive(Clone)]
pub struct Fragment {
    entries: Box<[Entry]>,
    witnesses: Box<[Witness]>,
    digest: Digest,
    context: Option<Digest>,
    graph: super::GraphBudget,
}

/// Authenticated historical closure used to acquire retention, without freshness
/// validators, current authority, or ownership of an operating-system pin.
pub(crate) struct Retention {
    entries: Box<[Entry]>,
    digest: Digest,
    context: Digest,
    graph: super::GraphBudget,
    bundles: Box<[(Entry, crate::store::bundle::BundleCommitment)]>,
}

impl Retention {
    /// Join complete historical closures without granting current authority.
    pub(crate) fn union(&self, other: &Self) -> Result<Self, Error> {
        if self.context != other.context {
            return Err(Error::State("retention belongs to another context"));
        }
        if !self.graph.shares_with(&other.graph) {
            return Err(Error::State("retention belongs to another graph"));
        }
        self.require_bindings()?;
        other.require_bindings()?;
        let count = self
            .entries
            .len()
            .checked_add(other.entries.len())
            .ok_or(Error::Allocation)?;
        let unique = self
            .entries
            .len()
            .checked_add(
                other
                    .entries
                    .iter()
                    .filter(|entry| self.entries.binary_search(entry).is_err())
                    .count(),
            )
            .ok_or(Error::Allocation)?;
        if unique > MAX_ENTRIES {
            return Err(Error::Limit {
                resource: "node read-set entries",
                limit: MAX_ENTRIES,
                actual: unique,
            });
        }
        let binding_count = self
            .bundles
            .len()
            .checked_add(other.bundles.len())
            .ok_or(Error::Allocation)?;
        // Charge both original arrays before allocating, including duplicates
        // that will disappear from the exact union.
        let bytes = count
            .checked_mul(std::mem::size_of::<Entry>())
            .and_then(|bytes| {
                binding_count
                    .checked_mul(std::mem::size_of::<(
                        Entry,
                        crate::store::bundle::BundleCommitment,
                    )>())
                    .and_then(|bindings| bytes.checked_add(bindings))
            })
            .ok_or(Error::Allocation)?;
        self.graph.decode(bytes)?;
        let mut entries = Vec::new();
        entries
            .try_reserve_exact(count)
            .map_err(|_| Error::Allocation)?;
        entries.extend_from_slice(&self.entries);
        entries.extend_from_slice(&other.entries);
        entries.sort_unstable_by_key(|entry| (entry.action, *entry));
        if entries
            .windows(2)
            .any(|pair| pair[0].action == pair[1].action && pair[0] != pair[1])
        {
            return Err(Error::State(
                "retention action has contradictory identities",
            ));
        }
        entries.sort_unstable();
        entries.dedup();
        let mut bundles = Vec::new();
        bundles
            .try_reserve_exact(binding_count)
            .map_err(|_| Error::Allocation)?;
        bundles.extend_from_slice(&self.bundles);
        bundles.extend_from_slice(&other.bundles);
        bundles.sort_unstable_by_key(|(entry, _)| *entry);
        if bundles
            .windows(2)
            .any(|pair| pair[0].0 == pair[1].0 && pair[0].1 != pair[1].1)
        {
            return Err(Error::State(
                "retention action has contradictory bundle commitments",
            ));
        }
        bundles.dedup();
        let union = Self {
            digest: digest(&entries),
            entries: entries.into_boxed_slice(),
            context: self.context,
            graph: self.graph.clone(),
            bundles: bundles.into_boxed_slice(),
        };
        union.require_bindings()?;
        Ok(union)
    }

    pub(crate) fn require_proof(&self, proof: &ReadSetProof) -> Result<(), Error> {
        proof
            .fragment
            .require_lineage(Some(self.context), &self.graph)?;
        self.require_bindings()?;
        if proof.fragment.context != Some(self.context)
            || proof.fragment.digest != self.digest
            || proof.fragment.entries != self.entries
        {
            return Err(Error::State(
                "retention differs from the prepared proof closure",
            ));
        }
        Ok(())
    }

    pub(crate) fn try_clone(&self) -> Result<Self, Error> {
        self.graph.decode(
            self.entries
                .len()
                .checked_mul(std::mem::size_of::<Entry>())
                .ok_or(Error::Allocation)?,
        )?;
        let mut entries = Vec::new();
        entries
            .try_reserve_exact(self.entries.len())
            .map_err(|_| Error::Allocation)?;
        entries.extend_from_slice(&self.entries);
        Ok(Self {
            entries: entries.into_boxed_slice(),
            digest: self.digest,
            context: self.context,
            graph: self.graph.clone(),
            bundles: self.copy_bundles(None)?,
        })
    }

    pub(crate) fn from_validated<K: Value>(
        value: &super::Validated<K>,
        commitment: Option<crate::store::bundle::BundleCommitment>,
    ) -> Result<Self, Error> {
        let receipt = value.receipt();
        let fragment = receipt.read_set();
        fragment.require_lineage(fragment.context, &fragment.graph)?;
        let context = fragment
            .context
            .ok_or(Error::State("retention context is absent"))?;
        let root = fragment
            .entries
            .iter()
            .find(|entry| entry.action == receipt.action())
            .ok_or(Error::State("retention root is absent from its closure"))?;
        if root.kind != K::KIND
            || root.schema != K::SCHEMA
            || root.recipe != receipt.recipe_digest()
            || root.content != receipt.content_digest()
        {
            return Err(Error::State(
                "retention root identity differs from its receipt",
            ));
        }
        if fragment
            .entries
            .iter()
            .any(|entry| entry.bundled && entry.action != root.action)
        {
            return Err(Error::State("retention bundled ancestor has no commitment"));
        }
        let bundle = match (root.bundled, commitment) {
            (true, Some(commitment)) => Some((*root, commitment)),
            (false, None) => None,
            (true, None) => return Err(Error::State("retention bundled root has no commitment")),
            (false, Some(_)) => {
                return Err(Error::State("retention has an extra bundle commitment"))
            }
        };
        fragment.graph.decode(
            fragment
                .entries
                .len()
                .checked_mul(std::mem::size_of::<Entry>())
                .ok_or(Error::Allocation)?,
        )?;
        let mut entries = Vec::new();
        entries
            .try_reserve_exact(fragment.entries.len())
            .map_err(|_| Error::Allocation)?;
        entries.extend_from_slice(&fragment.entries);
        let bundles = Self::copy_bindings(&fragment.graph, &[], bundle)?;
        Ok(Self {
            entries: entries.into_boxed_slice(),
            digest: fragment.digest,
            context,
            graph: fragment.graph.clone(),
            bundles,
        })
    }

    /// Extend an already-owned closure by one nonbundled output. This only
    /// preserves commitments; callers must acquire its pin before releasing the
    /// predecessor. It does not establish current authority or pin ownership.
    pub(crate) fn extend_current<K: Value>(
        &self,
        child: &super::Validated<K>,
    ) -> Result<Self, Error> {
        self.extend_fragment(child.receipt().action(), child.receipt().read_set())
    }

    /// Preserve every retained bundle while binding one new bundled output.
    /// Storage authentication and pin acquisition remain separate operations.
    pub(crate) fn extend_bundled_current<K: Value>(
        &self,
        child: &super::Validated<K>,
        commitment: crate::store::bundle::BundleCommitment,
    ) -> Result<Self, Error> {
        let receipt = child.receipt();
        let fragment = receipt.read_set();
        let root = fragment
            .entries
            .iter()
            .find(|entry| entry.action == receipt.action())
            .ok_or(Error::State("retention successor root is absent"))?;
        if root.kind != K::KIND
            || root.schema != K::SCHEMA
            || root.recipe != receipt.recipe_digest()
            || root.content != receipt.content_digest()
        {
            return Err(Error::State(
                "retention root identity differs from its receipt",
            ));
        }
        self.extend_with_commitment(receipt.action(), fragment, Some(commitment))
    }

    pub(crate) fn extend_staged<K: Value>(
        &self,
        child: &super::replay::Staged<K>,
    ) -> Result<Self, Error> {
        self.extend_fragment(child.action(), child.fragment())
    }

    fn extend_fragment(&self, action: ActionKey, fragment: &Fragment) -> Result<Self, Error> {
        self.extend_with_commitment(action, fragment, None)
    }

    fn extend_with_commitment(
        &self,
        action: ActionKey,
        fragment: &Fragment,
        commitment: Option<crate::store::bundle::BundleCommitment>,
    ) -> Result<Self, Error> {
        fragment.require_lineage(Some(self.context), &self.graph)?;
        self.require_bindings()?;
        let root = fragment
            .entries
            .iter()
            .find(|entry| entry.action == action)
            .ok_or(Error::State("retention successor root is absent"))?;
        if self.entries.iter().any(|entry| entry.action == action) {
            return Err(Error::State("retention successor root is already retained"));
        }
        let bundle = match (root.bundled, commitment) {
            (true, Some(commitment)) => Some((*root, commitment)),
            (false, None) => None,
            (true, None) => return Err(Error::State("retention bundled root has no commitment")),
            (false, Some(_)) => {
                return Err(Error::State("retention has an extra bundle commitment"))
            }
        };
        // A pure sibling may depend on only part of the retained closure.
        // Every ancestor must already be owned with its exact identity; retain
        // unrelated siblings without inventing dependencies in the recipe.
        if fragment.entries.len() < 2
            || fragment
                .entries
                .iter()
                .filter(|entry| entry.action != action)
                .any(|entry| self.entries.binary_search(entry).is_err())
        {
            return Err(Error::State(
                "retention successor changed its ancestor closure",
            ));
        }
        let retained = self.entries.len().checked_add(1).ok_or(Error::Allocation)?;
        if retained > MAX_ENTRIES {
            return Err(Error::Limit {
                resource: "node read-set entries",
                limit: MAX_ENTRIES,
                actual: retained,
            });
        }
        self.graph.decode(
            retained
                .checked_mul(std::mem::size_of::<Entry>())
                .ok_or(Error::Allocation)?,
        )?;
        let mut entries = Vec::new();
        entries
            .try_reserve_exact(retained)
            .map_err(|_| Error::Allocation)?;
        entries.extend_from_slice(&self.entries);
        entries.push(*root);
        entries.sort_unstable();
        Ok(Self {
            digest: digest(&entries),
            entries: entries.into_boxed_slice(),
            context: self.context,
            graph: self.graph.clone(),
            bundles: self.copy_bundles(bundle)?,
        })
    }

    pub(crate) fn validate_storage(&self, store: &crate::store::Store) -> Result<(), Error> {
        if !self.graph.shares_with(store.graph_budget()) {
            return Err(Error::State("retention belongs to another graph"));
        }
        self.require_bindings()?;
        for entry in &self.entries {
            store.revalidate_read_set_entry(*entry, &self.graph)?;
        }
        for (entry, commitment) in &self.bundles {
            crate::store::bundle::validate_commitment(
                store.root(),
                commitment,
                entry.kind,
                entry.schema,
                entry.content,
                &self.graph,
            )?;
        }
        Ok(())
    }

    fn require_bindings(&self) -> Result<(), Error> {
        // Bindings follow exact entry order, so the complete coverage check is
        // bounded and allocates nothing, including for unrelated siblings.
        let mut bundles = self.bundles.iter();
        for entry in self.entries.iter().filter(|entry| entry.bundled) {
            match bundles.next() {
                Some((bound, _)) if bound == entry => {}
                _ => {
                    return Err(Error::State(
                        "retention bundle bindings differ from its closure",
                    ))
                }
            }
        }
        if bundles.next().is_some() {
            return Err(Error::State("retention has an extra bundle commitment"));
        }
        Ok(())
    }

    fn copy_bundles(
        &self,
        extra: Option<(Entry, crate::store::bundle::BundleCommitment)>,
    ) -> Result<Box<[(Entry, crate::store::bundle::BundleCommitment)]>, Error> {
        Self::copy_bindings(&self.graph, &self.bundles, extra)
    }

    fn copy_bindings(
        graph: &super::GraphBudget,
        existing: &[(Entry, crate::store::bundle::BundleCommitment)],
        extra: Option<(Entry, crate::store::bundle::BundleCommitment)>,
    ) -> Result<Box<[(Entry, crate::store::bundle::BundleCommitment)]>, Error> {
        let count = existing
            .len()
            .checked_add(usize::from(extra.is_some()))
            .ok_or(Error::Allocation)?;
        graph.decode(
            count
                .checked_mul(std::mem::size_of::<(
                    Entry,
                    crate::store::bundle::BundleCommitment,
                )>())
                .ok_or(Error::Allocation)?,
        )?;
        let mut bundles = Vec::new();
        bundles
            .try_reserve_exact(count)
            .map_err(|_| Error::Allocation)?;
        bundles.extend_from_slice(existing);
        if let Some(binding) = extra {
            bundles.push(binding);
            bundles.sort_unstable_by_key(|(entry, _)| *entry);
        }
        Ok(bundles.into_boxed_slice())
    }

    pub(crate) fn require_parents(&self, parents: &[impl Edge]) -> Result<(), Error> {
        let [parent] = parents else {
            return Err(Error::State("retention requires exactly one recipe parent"));
        };
        let fragment = parent.read_set();
        fragment.require_lineage(Some(self.context), &self.graph)?;
        if fragment.digest != self.digest || fragment.entries.as_ref() != self.entries.as_ref() {
            return Err(Error::State(
                "retention differs from the recipe parent closure",
            ));
        }
        Ok(())
    }

    /// Prove coverage for an actual branch recipe without changing its edges.
    pub(crate) fn covers_parents(&self, parents: &[impl Edge]) -> Result<(), Error> {
        if parents.is_empty() {
            return Err(Error::State("retention requires recipe parents"));
        }
        for parent in parents {
            let fragment = parent.read_set();
            fragment.require_lineage(Some(self.context), &self.graph)?;
            if fragment
                .entries
                .iter()
                .any(|entry| self.entries.binary_search(entry).is_err())
            {
                return Err(Error::State(
                    "retention does not cover the recipe parent closure",
                ));
            }
        }
        Ok(())
    }

    pub(crate) const fn entries(&self) -> &[Entry] {
        &self.entries
    }

    pub(crate) const fn digest(&self) -> Digest {
        self.digest
    }
}

#[cfg(test)]
mod retention_tests;

/// A twice-revalidatable union of every recipe read by one graph evaluation.
pub struct ReadSetProof {
    fragment: Fragment,
    snapshot: Option<crate::supervisor::snapshot::Freshness>,
}

/// Exact current transitive closure ready to be pinned across admission.
///
/// This value can only be born from a current validated chain. It carries no
/// process or supervisor authority. It retains closed store roles and
/// revalidates the closure immediately before the supplied admission operation.
pub(crate) struct Prepared {
    context: crate::context::Context,
    proof: ReadSetProof,
    retention: Option<Retention>,
    pins: std::sync::Mutex<Pins>,
}

#[derive(Default)]
struct Pins {
    prepared: Option<crate::store::PinGuard>,
    live: Option<crate::store::PinGuard>,
    last_good: Option<crate::store::PinGuard>,
    startup: Option<Startup>,
}

/// Born only after snapshot binding and retained Prepared acquisition. The
/// enclosing proof and owned guard supply the rest of this private handoff.
struct Startup {
    closure: Digest,
}

#[derive(Clone, Copy, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) struct Entry {
    pub(crate) kind: Kind,
    pub(crate) schema: u32,
    pub(crate) action: ActionKey,
    pub(crate) recipe: Digest,
    pub(crate) content: Digest,
    pub(crate) bundled: bool,
}

#[derive(Clone)]
pub(crate) struct Witness {
    expected: Entry,
    current: Arc<dyn Current>,
}

#[cfg(test)]
struct Budget {
    decoded: u64,
    hashed: u64,
}

trait Current: Send + Sync {
    fn revalidate(&self, budget: &super::GraphBudget, scope: Scope) -> Result<Entry, Error>;
}

struct Static {
    entry: Entry,
}

struct Discovered<K> {
    recipe: Discoverable<K>,
    value: Arc<K>,
    validate: ScopedValidator<K>,
    content: ContentRef<K>,
    bundled: bool,
}

impl Witness {
    pub(crate) fn direct<K: Value>(
        recipe: &Direct<K>,
        content: ContentRef<K>,
        bundled: bool,
    ) -> Self {
        let expected = entry::<K>(recipe, content, bundled);
        Self {
            expected,
            current: Arc::new(Static { entry: expected }),
        }
    }

    #[cfg(test)]
    pub(crate) fn discovered<K>(
        recipe: &Discoverable<K>,
        value: Arc<K>,
        validate: Validator<K>,
        current: &Direct<K>,
        content: ContentRef<K>,
        bundled: bool,
    ) -> Self
    where
        K: Value + Send + Sync + 'static,
    {
        Self::discovered_scoped(
            recipe,
            value,
            Arc::new(move |value, budget, _| validate(value, budget)),
            current,
            content,
            bundled,
        )
    }

    pub(crate) fn discovered_scoped<K>(
        recipe: &Discoverable<K>,
        value: Arc<K>,
        validate: ScopedValidator<K>,
        current: &Direct<K>,
        content: ContentRef<K>,
        bundled: bool,
    ) -> Self
    where
        K: Value + Send + Sync + 'static,
    {
        Self {
            expected: entry::<K>(current, content, bundled),
            current: Arc::new(Discovered {
                recipe: recipe.clone(),
                value,
                validate,
                content,
                bundled,
            }),
        }
    }

    fn revalidate(&self, budget: &super::GraphBudget, scope: Scope) -> Result<Entry, Error> {
        let current = self.current.revalidate(budget, scope)?;
        if current != self.expected {
            return Err(Error::StaleReadSet {
                action: self.expected.action,
            });
        }
        Ok(current)
    }
}

#[cfg(test)]
impl Budget {
    fn new() -> Self {
        Self {
            decoded: 0,
            hashed: 0,
        }
    }

    pub(crate) fn decode(&mut self, bytes: usize) -> Result<(), Error> {
        charge(
            &mut self.decoded,
            bytes,
            MAX_DECODE_BYTES,
            "node read-set decoded bytes",
        )
    }

    pub(crate) fn hash(&mut self, bytes: usize) -> Result<(), Error> {
        charge(
            &mut self.hashed,
            bytes,
            MAX_HASH_BYTES,
            "node read-set hashed bytes",
        )
    }
}

impl Current for Static {
    fn revalidate(&self, _budget: &super::GraphBudget, _scope: Scope) -> Result<Entry, Error> {
        Ok(self.entry)
    }
}

impl<K> Current for Discovered<K>
where
    K: Value + Send + Sync + 'static,
{
    fn revalidate(&self, budget: &super::GraphBudget, scope: Scope) -> Result<Entry, Error> {
        let discovered = (self.validate)(&self.value, budget, scope)?;
        let current = self.recipe.complete(&discovered)?;
        Ok(entry::<K>(&current, self.content, self.bundled))
    }
}

impl Fragment {
    /// Compare an exact authenticated entry without granting current authority.
    pub(crate) fn contains_entry(&self, expected: &Entry) -> bool {
        self.entries.binary_search(expected).is_ok()
    }

    pub(crate) fn charge_copy(&self) -> Result<(), Error> {
        let bytes = self
            .entries
            .len()
            .checked_mul(std::mem::size_of::<Entry>())
            .and_then(|bytes| {
                self.witnesses
                    .len()
                    .checked_mul(std::mem::size_of::<Witness>())
                    .and_then(|witnesses| bytes.checked_add(witnesses))
            })
            .ok_or(Error::Allocation)?;
        self.graph.decode(bytes)
    }

    /// Check exact ancestry and original account without refreshing any facts.
    #[cfg(feature = "rustc-driver")]
    pub(crate) fn require_ancestor(&self, ancestor: &Self) -> Result<(), Error> {
        self.require_lineage(ancestor.context, &ancestor.graph)?;
        if ancestor
            .entries
            .iter()
            .any(|entry| self.entries.binary_search(entry).is_err())
        {
            return Err(Error::State("current API lost its exact owner ancestry"));
        }
        Ok(())
    }

    pub(super) fn require_successor(&self, parent: &Self, action: ActionKey) -> Result<(), Error> {
        self.require_lineage(parent.context, &parent.graph)?;
        if self.entries.len() != parent.entries.len().saturating_add(1)
            || !self.entries.iter().any(|entry| entry.action == action)
            || parent.entries.iter().any(|entry| entry.action == action)
            || !self
                .entries
                .iter()
                .filter(|entry| entry.action != action)
                .eq(parent.entries.iter())
        {
            return Err(Error::State(
                "staged promotion is not an exact successor closure",
            ));
        }
        Ok(())
    }

    pub(crate) fn compose<K: Value>(
        parents: &[impl Edge],
        action: ActionKey,
        recipe: Digest,
        content: ContentRef<K>,
        bundled: bool,
        current: Witness,
        context: Digest,
        graph: &super::GraphBudget,
    ) -> Result<Self, Error> {
        for parent in parents {
            parent.read_set().require_lineage(Some(context), graph)?;
        }
        let retained = parents.iter().try_fold(1_usize, |total, parent| {
            total.checked_add(parent.read_set().entries.len())
        });
        let retained = retained.ok_or(Error::Limit {
            resource: "node read-set entries",
            limit: MAX_ENTRIES,
            actual: usize::MAX,
        })?;
        if retained > MAX_ENTRIES {
            return Err(Error::Limit {
                resource: "node read-set entries",
                limit: MAX_ENTRIES,
                actual: retained,
            });
        }
        graph.decode(
            retained
                .checked_mul(std::mem::size_of::<(Entry, Witness)>())
                .ok_or(Error::Allocation)?,
        )?;
        let mut pairs = Vec::new();
        pairs
            .try_reserve_exact(retained)
            .map_err(|_| Error::Allocation)?;
        for parent in parents {
            pairs.extend(
                parent
                    .read_set()
                    .entries
                    .iter()
                    .copied()
                    .zip(parent.read_set().witnesses.iter().cloned()),
            );
        }
        let expected = Entry {
            kind: K::KIND,
            schema: K::SCHEMA,
            action,
            recipe,
            content: content.digest(),
            bundled,
        };
        if current.expected != expected {
            return Err(Error::State(
                "node read-set witness does not match its receipt",
            ));
        }
        pairs.push((expected, current));
        Self::from_pairs(pairs, Some(context), graph)
    }

    fn from_pairs(
        mut pairs: Vec<(Entry, Witness)>,
        context: Option<Digest>,
        graph: &super::GraphBudget,
    ) -> Result<Self, Error> {
        if pairs.len() > MAX_ENTRIES {
            return Err(Error::Limit {
                resource: "node read-set entries",
                limit: MAX_ENTRIES,
                actual: pairs.len(),
            });
        }
        pairs.sort_unstable_by_key(|(entry, _)| (entry.action, *entry));
        if pairs
            .windows(2)
            .any(|pair| pair[0].0.action == pair[1].0.action && pair[0].0 != pair[1].0)
        {
            return Err(Error::State(
                "node read-set action has contradictory identities",
            ));
        }
        pairs.sort_unstable_by_key(|(entry, _)| *entry);
        pairs.dedup_by(|left, right| left.0 == right.0);
        graph.decode(
            pairs
                .len()
                .checked_mul(std::mem::size_of::<Entry>() + std::mem::size_of::<Witness>())
                .ok_or(Error::Allocation)?,
        )?;
        let mut entries = Vec::new();
        let mut witnesses = Vec::new();
        entries
            .try_reserve_exact(pairs.len())
            .map_err(|_| Error::Allocation)?;
        witnesses
            .try_reserve_exact(pairs.len())
            .map_err(|_| Error::Allocation)?;
        for (entry, witness) in pairs {
            entries.push(entry);
            witnesses.push(witness);
        }
        let digest = digest(&entries);
        Ok(Self {
            entries: entries.into_boxed_slice(),
            witnesses: witnesses.into_boxed_slice(),
            digest,
            context,
            graph: graph.clone(),
        })
    }

    pub(crate) fn seal(
        store: &crate::store::Store,
        fragments: impl IntoIterator<Item = Fragment>,
    ) -> Result<ReadSetProof, Error> {
        Self::seal_scoped(store, fragments, Scope::Consumer)
    }

    fn seal_scoped(
        store: &crate::store::Store,
        fragments: impl IntoIterator<Item = Fragment>,
        scope: Scope,
    ) -> Result<ReadSetProof, Error> {
        let mut pairs = Vec::new();
        let mut context = None;
        let mut admitted = 0;
        for fragment in fragments {
            fragment.require_lineage(context.or(fragment.context), store.graph_budget())?;
            if context.is_none() {
                context = fragment.context;
            }
            let retained = pairs.len().saturating_add(fragment.entries.len());
            if retained > MAX_ENTRIES {
                return Err(Error::Limit {
                    resource: "node read-set entries",
                    limit: MAX_ENTRIES,
                    actual: retained,
                });
            }
            reserve_pairs(&mut pairs, &mut admitted, retained, store.graph_budget())?;
            pairs.extend(
                fragment
                    .entries
                    .into_vec()
                    .into_iter()
                    .zip(fragment.witnesses.into_vec()),
            );
        }
        let fragment = Self::from_pairs(pairs, context, store.graph_budget())?;
        fragment.revalidate_scoped(store, scope)?;
        Ok(ReadSetProof {
            fragment,
            snapshot: None,
        })
    }

    fn require_lineage(
        &self,
        context: Option<Digest>,
        graph: &super::GraphBudget,
    ) -> Result<(), Error> {
        if !self.graph.shares_with(graph) {
            return Err(Error::State("node read-set belongs to another graph"));
        }
        if self.context != context && !self.entries.is_empty() {
            return Err(Error::State("node read-set belongs to another context"));
        }
        if self.context.is_none() && !self.entries.is_empty() {
            return Err(Error::State("node read-set nonempty lineage is absent"));
        }
        Ok(())
    }

    pub(crate) fn revalidate(&self, store: &crate::store::Store) -> Result<(), Error> {
        self.revalidate_scoped(store, Scope::Consumer)
    }

    fn revalidate_scoped(&self, store: &crate::store::Store, scope: Scope) -> Result<(), Error> {
        self.require_lineage(self.context, store.graph_budget())?;
        if self.entries.len() != self.witnesses.len() || digest(&self.entries) != self.digest {
            return Err(Error::State("node read-set proof shape changed"));
        }
        let budget = store.graph_budget();
        for (entry, witness) in self.entries.iter().zip(self.witnesses.iter()) {
            if witness.revalidate(budget, scope)? != *entry {
                return Err(Error::StaleReadSet {
                    action: entry.action,
                });
            }
            store.revalidate_read_set_entry(*entry, budget)?;
        }
        Ok(())
    }

    /// Number of unique current recipe entries in this transitive fragment.
    #[must_use]
    pub const fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether this fragment contains no current recipes.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Comparison-only identity for parent-envelope provenance.
    #[must_use]
    pub const fn digest(&self) -> Digest {
        self.digest
    }

    /// Test whether this fragment contains one current action.
    #[must_use]
    pub fn contains(&self, action: ActionKey) -> bool {
        self.entries.iter().any(|entry| entry.action == action)
    }
}

fn reserve_pairs(
    pairs: &mut Vec<(Entry, Witness)>,
    admitted: &mut usize,
    requested: usize,
    graph: &super::GraphBudget,
) -> Result<(), Error> {
    if requested > *admitted {
        graph.decode(
            requested
                .checked_mul(std::mem::size_of::<(Entry, Witness)>())
                .ok_or(Error::Allocation)?,
        )?;
        pairs
            .try_reserve_exact(requested.saturating_sub(pairs.len()))
            .map_err(|_| Error::Allocation)?;
        *admitted = requested;
    }
    Ok(())
}

impl ReadSetProof {
    pub(super) fn into_fragment(self) -> Fragment {
        self.fragment
    }

    pub(super) fn from_parents(
        store: &crate::store::Store,
        parents: &[impl Edge],
    ) -> Result<Option<Self>, Error> {
        if parents.is_empty() {
            return Ok(None);
        }
        Fragment::seal(
            store,
            parents.iter().map(|parent| parent.read_set().clone()),
        )
        .map(Some)
    }

    pub(crate) fn revalidate(&self, store: &crate::store::Store) -> Result<(), Error> {
        self.revalidate_scoped(store, Scope::Consumer)
    }

    pub(crate) fn revalidate_complete(&self, store: &crate::store::Store) -> Result<(), Error> {
        self.revalidate_scoped(store, Scope::Complete)
    }

    pub(crate) fn revalidate_scoped(
        &self,
        store: &crate::store::Store,
        scope: Scope,
    ) -> Result<(), Error> {
        self.fragment.revalidate_scoped(store, scope)?;
        if let Some(snapshot) = &self.snapshot {
            snapshot.revalidate(store.graph_budget())?;
        }
        Ok(())
    }

    pub(crate) const fn entries(&self) -> &[Entry] {
        &self.fragment.entries
    }

    /// Number of unique current actions revalidated by this proof.
    #[must_use]
    pub const fn len(&self) -> usize {
        self.fragment.len()
    }

    /// Whether this proof contains no current actions.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.fragment.is_empty()
    }

    /// Comparison-only identity of the current deduplicated union.
    #[must_use]
    pub const fn digest(&self) -> Digest {
        self.fragment.digest()
    }

    /// Test whether this proof covers one current action.
    #[must_use]
    pub fn contains(&self, action: ActionKey) -> bool {
        self.fragment.contains(action)
    }
}

impl Prepared {
    #[cfg(test)]
    pub(super) fn seal(
        context: crate::context::Context,
        fragment: Fragment,
    ) -> Result<Self, Error> {
        Self::seal_union(context, [fragment])
    }

    pub(crate) fn seal_union(
        context: crate::context::Context,
        fragments: impl IntoIterator<Item = Fragment>,
    ) -> Result<Self, Error> {
        let store = context.open_store()?;
        let proof = Fragment::seal_scoped(&store, fragments, Scope::Complete)?;
        Ok(Self {
            context,
            proof,
            retention: None,
            pins: std::sync::Mutex::new(Pins::default()),
        })
    }

    pub(crate) fn with_retention(mut self, retention: Retention) -> Result<Self, Error> {
        if self.retention.is_some() {
            return Err(Error::State("prepared retention cannot be replaced"));
        }
        let pins = self
            .pins
            .get_mut()
            .map_err(|_| Error::State("prepared read-set pin ownership lock was poisoned"))?;
        if pins.prepared.is_some() || pins.live.is_some() || pins.last_good.is_some() {
            return Err(Error::State("retention must bind before pin ownership"));
        }
        retention.require_proof(&self.proof)?;
        self.retention = Some(retention);
        Ok(self)
    }

    pub(crate) fn bind_snapshot(
        &mut self,
        snapshot: &crate::supervisor::snapshot::Bytes,
    ) -> Result<(), Error> {
        if let Some(current) = &self.proof.snapshot {
            if !current.matches(snapshot) {
                return Err(Error::State("prepared snapshot witness cannot be replaced"));
            }
            return self.revalidate();
        }
        let pins = self
            .pins
            .get_mut()
            .map_err(|_| Error::State("prepared read-set pin ownership lock was poisoned"))?;
        if pins.prepared.is_some() || pins.live.is_some() || pins.last_good.is_some() {
            return Err(Error::State(
                "snapshot witness must bind before pin ownership",
            ));
        }
        let store = self.context.open_store()?;
        let freshness = snapshot.freshness(store.graph_budget(), &self.context.limits())?;
        freshness.revalidate(store.graph_budget())?;
        self.proof.snapshot = Some(freshness);
        Ok(())
    }

    pub(crate) fn with_pin<T, E>(
        &self,
        operation: impl FnOnce() -> Result<T, E>,
    ) -> Result<Result<T, E>, Error> {
        self.pin_prepared()?;
        self.revalidate()?;
        Ok(operation())
    }

    pub(crate) fn retain_startup(
        &mut self,
        snapshot: &crate::supervisor::snapshot::Bytes,
    ) -> Result<(), Error> {
        self.bind_snapshot(snapshot)?;
        let Some(retention) = &self.retention else {
            return self.pin_prepared();
        };
        let pins = self
            .pins
            .get_mut()
            .map_err(|_| Error::State("prepared read-set pin ownership lock was poisoned"))?;
        if pins.prepared.is_some()
            || pins.live.is_some()
            || pins.last_good.is_some()
            || pins.startup.is_some()
        {
            return Err(Error::State(
                "startup retention must precede admission ownership",
            ));
        }
        let store = self.context.open_store()?;
        let guard = store.acquire_startup_retained(&self.proof, retention)?;
        pins.prepared = Some(guard);
        pins.startup = Some(Startup {
            closure: self.proof.digest(),
        });
        Ok(())
    }

    pub(crate) fn bind_startup_snapshot(
        &mut self,
        snapshot: &crate::supervisor::snapshot::Bytes,
    ) -> Result<(), Error> {
        let pins = self
            .pins
            .get_mut()
            .map_err(|_| Error::State("prepared read-set pin ownership lock was poisoned"))?;
        if let Some(startup) = &pins.startup {
            if startup.closure != self.proof.digest()
                || pins.prepared.is_none()
                || pins.live.is_some()
                || pins.last_good.is_some()
                || !self
                    .proof
                    .snapshot
                    .as_ref()
                    .is_some_and(|current| current.matches(snapshot))
            {
                return Err(Error::State(
                    "startup snapshot or Prepared ownership changed",
                ));
            }
            self.retention
                .as_ref()
                .ok_or(Error::State("startup retention absent"))?
                .require_proof(&self.proof)?;
            return Ok(());
        }
        self.bind_snapshot(snapshot)
    }

    pub(crate) fn with_startup_pin<T, E>(
        &self,
        operation: impl FnOnce() -> Result<T, E>,
    ) -> Result<Result<T, E>, Error> {
        let mut pins = self
            .pins
            .lock()
            .map_err(|_| Error::State("prepared read-set pin ownership lock was poisoned"))?;
        let Some(startup) = pins.startup.take() else {
            drop(pins);
            return self.with_pin(operation);
        };
        if startup.closure != self.proof.digest()
            || pins.prepared.is_none()
            || pins.live.is_some()
            || pins.last_good.is_some()
            || self.proof.snapshot.is_none()
        {
            return Err(Error::State("startup handoff lost its Prepared ownership"));
        }
        self.retention
            .as_ref()
            .ok_or(Error::State("startup retention absent"))?
            .require_proof(&self.proof)?;
        drop(pins);
        self.revalidate()?;
        Ok(operation())
    }

    pub(crate) fn pin_prepared(&self) -> Result<(), Error> {
        let mut pins = self
            .pins
            .lock()
            .map_err(|_| Error::State("prepared read-set pin ownership lock was poisoned"))?;
        if pins.prepared.is_some() {
            drop(pins);
            return self.revalidate();
        }
        let store = self.context.open_store()?;
        pins.prepared = Some(match &self.retention {
            Some(retention) => store.acquire_prepared_retained(&self.proof, retention)?,
            None => store.acquire_pin(crate::store::PinRole::Prepared, &self.proof)?,
        });
        Ok(())
    }

    pub(crate) fn revalidate(&self) -> Result<(), Error> {
        let store = self.context.open_store()?;
        self.proof.revalidate(&store)
    }

    pub(crate) fn revalidate_complete(&self) -> Result<(), Error> {
        let store = self.context.open_store()?;
        self.proof.revalidate_complete(&store)
    }

    pub(crate) fn pin_live_last_good(&self) -> Result<(), Error> {
        match self.with_live_last_good(|| Ok::<(), std::convert::Infallible>(()))? {
            Ok(()) => Ok(()),
            Err(
                crate::store::transaction::Rejected::Operation(never)
                | crate::store::transaction::Rejected::Cleanup { failure: never, .. },
            ) => match never {},
        }
    }

    /// Keep durable rollback authority until the exact admission operation succeeds.
    /// The operation runs under publication and may not reopen or mutate the store.
    pub(crate) fn with_live_last_good<T, E>(
        &self,
        operation: impl FnOnce() -> Result<T, E>,
    ) -> Result<Result<T, crate::store::transaction::Rejected<E>>, Error> {
        let mut pins = self
            .pins
            .lock()
            .map_err(|_| Error::State("prepared read-set pin ownership lock was poisoned"))?;
        match (pins.live.is_some(), pins.last_good.is_some()) {
            (true, true) | (false, false) => {}
            _ => {
                return Err(Error::State(
                    "prepared read-set retained a partial live/last-good pin pair",
                ));
            }
        }
        let store = self.context.open_store()?;
        let result = match &self.retention {
            Some(retention) => {
                store.with_live_last_good_retained(&self.proof, retention, operation)?
            }
            None => store.with_live_last_good(&self.proof, operation)?,
        };
        match result {
            Ok((value, [live, last_good])) => {
                pins.live = Some(live);
                pins.last_good = Some(last_good);
                Ok(Ok(value))
            }
            Err(rejected) => Ok(Err(rejected)),
        }
    }

    #[cfg(test)]
    pub(crate) const fn proof(&self) -> &ReadSetProof {
        &self.proof
    }
}

fn entry<K: Value>(recipe: &Direct<K>, content: ContentRef<K>, bundled: bool) -> Entry {
    Entry {
        kind: K::KIND,
        schema: K::SCHEMA,
        action: recipe.action,
        recipe: recipe.recipe,
        content: content.digest(),
        bundled,
    }
}

fn digest(entries: &[Entry]) -> Digest {
    let mut digest = Sha256::new();
    digest.update(b"rawr.node.read-set/v2");
    digest.update((entries.len() as u64).to_le_bytes());
    for entry in entries {
        digest.update([entry.kind as u8]);
        digest.update(entry.schema.to_le_bytes());
        digest.update(entry.action.as_bytes());
        digest.update(entry.recipe.as_bytes());
        digest.update(entry.content.as_bytes());
        digest.update([u8::from(entry.bundled)]);
    }
    Digest::from_bytes(digest.finalize().into())
}

#[cfg(test)]
fn charge(
    retained: &mut u64,
    bytes: usize,
    limit: u64,
    resource: &'static str,
) -> Result<(), Error> {
    let bytes = u64::try_from(bytes).unwrap_or(u64::MAX);
    let actual = retained.checked_add(bytes).unwrap_or(u64::MAX);
    if actual > limit {
        return Err(Error::Limit {
            resource,
            limit: usize::try_from(limit).unwrap_or(usize::MAX),
            actual: usize::try_from(actual).unwrap_or(usize::MAX),
        });
    }
    *retained = actual;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lineage_allocator_capacity_does_not_authorize_unadmitted_slots() {
        let graph = super::super::GraphBudget::new();
        let mut pairs = Vec::with_capacity(8);
        let mut admitted = 0;
        let bytes = std::mem::size_of::<(Entry, Witness)>();
        graph
            .decode(super::super::MAX_DECODED_BYTES_PER_GRAPH as usize - bytes)
            .unwrap();
        reserve_pairs(&mut pairs, &mut admitted, 1, &graph).unwrap();
        assert_eq!(admitted, 1);
        assert!(pairs.capacity() >= 8);
        assert!(
            !graph.exceedances().is_exceeded(),
            "the exact decoded-byte target is not an exceedance"
        );
        // Allocator surplus never substitutes for the account. The two
        // requested slots are charged in full to the original account, which
        // records the exceedance instead of refusing them.
        reserve_pairs(&mut pairs, &mut admitted, 2, &graph).unwrap();
        assert_eq!(admitted, 2);
        let target = super::super::MAX_DECODED_BYTES_PER_GRAPH;
        let decoded = graph
            .exceedances()
            .axis(super::super::DECODED_RESOURCE)
            .copied()
            .expect("unadmitted slots charge the decoded-byte account");
        assert_eq!(decoded.target, target);
        assert_eq!(decoded.first_requested, target + 2 * bytes as u64);
        assert_eq!(decoded.high_water, target + 2 * bytes as u64);
        assert_eq!(graph.usage.lock().unwrap().decoded, decoded.high_water);
    }

    struct LineageValue;
    impl Value for LineageValue {
        const KIND: Kind = Kind::Api;
        const SCHEMA: u32 = 1;
        fn encode(&self, _: usize) -> Result<Box<[u8]>, Error> {
            Ok(Box::default())
        }
        fn decode(_: &[u8], _: usize) -> Result<Self, Error> {
            Ok(Self)
        }
    }
    struct NeverCurrent;
    impl Current for NeverCurrent {
        fn revalidate(&self, _: &super::super::GraphBudget, _: Scope) -> Result<Entry, Error> {
            panic!("lineage composition must not invoke semantic validation")
        }
    }
    fn lineage_fragment(
        parents: &[Parent],
        context: Digest,
        graph: &super::super::GraphBudget,
    ) -> Result<Fragment, Error> {
        let entry = Entry {
            kind: Kind::Api,
            schema: 1,
            action: ActionKey::from_bytes([1; 32]),
            recipe: Digest::from_bytes([2; 32]),
            content: Digest::from_bytes([3; 32]),
            bundled: false,
        };
        Fragment::compose::<LineageValue>(
            parents,
            entry.action,
            entry.recipe,
            ContentRef::new(entry.content),
            false,
            Witness {
                expected: entry,
                current: Arc::new(NeverCurrent),
            },
            context,
            graph,
        )
    }
    fn lineage_parent(context: Digest, graph: &super::super::GraphBudget) -> Parent {
        let fragment = lineage_fragment(&[], context, graph).unwrap();
        let entry = fragment.entries[0];
        let content = ContentRef::<LineageValue>::new(entry.content);
        let receipt = super::super::Receipt::new(entry.action, entry.recipe, content, fragment);
        Parent::validated(&super::super::Validated::new(
            Arc::new(LineageValue),
            content,
            receipt,
        ))
    }
    #[test]
    fn lineage_same_context_and_original_graph_composes_without_semantic_validation() {
        let graph = super::super::GraphBudget::new();
        let context = Digest::from_bytes([5; 32]);
        let parent = lineage_parent(context, &graph);
        let fragment = lineage_fragment(&[parent], context, &graph.clone()).unwrap();
        assert!(fragment.graph.shares_with(&graph));
        assert!(fragment.context == Some(context));
        assert_eq!(fragment.len(), 1);
    }
    #[test]
    fn lineage_foreign_graph_with_same_context_rejects() {
        let graph = super::super::GraphBudget::new();
        let context = Digest::from_bytes([5; 32]);
        let parent = lineage_parent(context, &graph);
        assert!(matches!(
            lineage_fragment(&[parent], context, &super::super::GraphBudget::new()),
            Err(Error::State("node read-set belongs to another graph"))
        ));
    }
    #[test]
    fn lineage_foreign_context_with_same_graph_rejects() {
        let graph = super::super::GraphBudget::new();
        let parent = lineage_parent(Digest::from_bytes([5; 32]), &graph);
        assert!(matches!(
            lineage_fragment(&[parent], Digest::from_bytes([6; 32]), &graph),
            Err(Error::State("node read-set belongs to another context"))
        ));
    }

    #[test]
    fn lineage_contradictory_action_identity_rejects() {
        let graph = super::super::GraphBudget::new();
        let context = Digest::from_bytes([5; 32]);
        let fragment = lineage_fragment(&[], context, &graph).unwrap();
        let first = fragment.entries[0];
        let mut conflicting = first;
        conflicting.recipe = Digest::from_bytes([9; 32]);
        let pairs = [first, conflicting]
            .into_iter()
            .map(|entry| {
                (
                    entry,
                    Witness {
                        expected: entry,
                        current: Arc::new(NeverCurrent),
                    },
                )
            })
            .collect();
        assert!(matches!(
            Fragment::from_pairs(pairs, Some(context), &graph),
            Err(Error::State(
                "node read-set action has contradictory identities"
            ))
        ));
    }
    // Composition past the decoded-byte target still takes ownership on
    // the original account; the account records the pair array as the first
    // exceeding charge and the retained arrays as its truthful high water.
    #[test]
    fn lineage_composition_charges_original_account_and_records_exceedance() {
        let graph = super::super::GraphBudget::new();
        let target = super::super::MAX_DECODED_BYTES_PER_GRAPH;
        graph.decode(usize::try_from(target).unwrap()).unwrap();
        assert!(!graph.exceedances().is_exceeded());
        let fragment = lineage_fragment(&[], Digest::from_bytes([5; 32]), &graph)
            .expect("composition past the accounting target still composes");
        assert_eq!(fragment.len(), 1);
        assert!(fragment.graph.shares_with(&graph));
        let pairs = std::mem::size_of::<(Entry, Witness)>() as u64;
        let retained = (std::mem::size_of::<Entry>() + std::mem::size_of::<Witness>()) as u64;
        let decoded = graph
            .exceedances()
            .axis(super::super::DECODED_RESOURCE)
            .copied()
            .expect("composition charges the decoded-byte account");
        assert_eq!(decoded.target, target);
        assert_eq!(decoded.first_requested, target + pairs);
        assert_eq!(decoded.high_water, target + pairs + retained);
        assert_eq!(graph.usage.lock().unwrap().decoded, decoded.high_water);
    }

    #[test]
    fn read_set_work_budgets_accept_exact_caps_and_reject_one_more_byte() {
        let mut decode = Budget::new();
        decode
            .decode(usize::try_from(MAX_DECODE_BYTES).expect("the decode cap is addressable"))
            .expect("the exact decode cap is accepted");
        assert!(matches!(
            decode.decode(1),
            Err(Error::Limit {
                resource: "node read-set decoded bytes",
                limit,
                actual,
            }) if limit == usize::try_from(MAX_DECODE_BYTES).expect("the cap is addressable")
                && actual == limit + 1
        ));

        let mut hash = Budget::new();
        hash.hash(usize::try_from(MAX_HASH_BYTES).expect("the hash cap is addressable"))
            .expect("the exact hash cap is accepted");
        assert!(matches!(
            hash.hash(1),
            Err(Error::Limit {
                resource: "node read-set hashed bytes",
                limit,
                actual,
            }) if limit == usize::try_from(MAX_HASH_BYTES).expect("the cap is addressable")
                && actual == limit + 1
        ));
    }
}
