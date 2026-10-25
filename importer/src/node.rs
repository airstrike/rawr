//! Typed content-addressed importer transformations.

use std::marker::PhantomData;
use std::sync::{Arc, Mutex};

use sha2::{Digest as _, Sha256};

use crate::metrics;

pub(crate) mod account;
pub(crate) mod codec;
mod content;
mod key;
pub(crate) mod read_set;
pub(crate) mod replay;
pub(crate) mod staging;

pub use crate::store::bundle::Corruption;
pub use content::{Chain, ContentRef, Parent, Receipt, Validated};
pub use key::{ActionKey, Digest, ExecutionKey, LookupKey};
pub use read_set::{Fragment, ReadSetProof};

pub(crate) const STORAGE_SCHEMA: u16 = 2;
pub(crate) const STORAGE_DIRECTORY: &str = "0002";
const MAX_RECIPE_BYTES: usize = 16 * 1024 * 1024;
pub(crate) const MAX_CONTENT_BYTES: usize = 1_207_959_552;

/// One versioned importer transform family.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
#[repr(u8)]
pub enum Kind {
    Toolchain = 1,
    Resolution = 2,
    Api = 3,
    Generic = 4,
    Profile = 5,
    Facade = 6,
    Checked = 7,
    ProfileProof = 8,
    NativeProduct = 9,
    Overlay = 10,
    Runner = 11,
}

impl Kind {
    pub(crate) const fn from_code(code: u8) -> Option<Self> {
        match code {
            1 => Some(Self::Toolchain),
            2 => Some(Self::Resolution),
            3 => Some(Self::Api),
            4 => Some(Self::Generic),
            5 => Some(Self::Profile),
            6 => Some(Self::Facade),
            7 => Some(Self::Checked),
            8 => Some(Self::ProfileProof),
            9 => Some(Self::NativeProduct),
            10 => Some(Self::Overlay),
            11 => Some(Self::Runner),
            _ => None,
        }
    }

    pub(crate) const fn directory(self) -> &'static str {
        match self {
            Self::Toolchain => "toolchain",
            Self::Resolution => "resolution",
            Self::Api => "api",
            Self::Generic => "generic",
            Self::Profile => "profile",
            Self::Facade => "facade",
            Self::Checked => "checked",
            Self::ProfileProof => "profile-proof",
            Self::NativeProduct => "native-product",
            Self::Overlay => "overlay",
            Self::Runner => "runner",
        }
    }

    pub(crate) const fn metric(self) -> metrics::NodeKind {
        match self {
            Self::Toolchain => metrics::NodeKind::Toolchain,
            Self::Resolution => metrics::NodeKind::Resolution,
            Self::Api => metrics::NodeKind::Api,
            Self::Generic => metrics::NodeKind::Generic,
            Self::Profile => metrics::NodeKind::Profile,
            Self::Facade => metrics::NodeKind::Facade,
            Self::Checked => metrics::NodeKind::Checked,
            Self::ProfileProof => metrics::NodeKind::ProfileProof,
            Self::NativeProduct => metrics::NodeKind::NativeProduct,
            Self::Overlay => metrics::NodeKind::Overlay,
            Self::Runner => metrics::NodeKind::Runner,
        }
    }
}

/// A typed value that can cross the private node codec boundary.
pub trait Value: Sized {
    /// Closed transform family producing this value.
    const KIND: Kind;
    /// Version of this value's canonical output codec.
    const SCHEMA: u32;

    /// Encode this value canonically within the recipe's retained-output limit.
    fn encode(&self, limit: usize) -> Result<Box<[u8]>, Error>;
    /// Encode a value that is about to be dropped; one already holding its
    /// encoding can hand that buffer over instead of copying it.
    fn into_encoded(self, limit: usize) -> Result<Box<[u8]>, Error> {
        self.encode(limit)
    }
    /// Decode one fully consumed canonical output within its transform's
    /// prospective count, byte, depth, and allocation bounds.
    fn decode(bytes: &[u8], limit: usize) -> Result<Self, Error>;
    /// Decode owned store bytes; a value that can keep them avoids a copy.
    fn decode_owned(bytes: Box<[u8]>, limit: usize) -> Result<Self, Error> {
        Self::decode(&bytes, limit)
    }
}

/// A complete direct-node recipe captured before lookup or execution.
pub struct Direct<K> {
    action: ActionKey,
    recipe: Digest,
    context: Digest,
    output_limit: usize,
    parents: Box<[content::Provenance]>,
    _kind: PhantomData<fn() -> K>,
}

/// One bounded recipe whose complete action facts are learned by execution.
pub struct Discoverable<K> {
    lookup: LookupKey,
    execution: ExecutionKey,
    context: Digest,
    fixed_inputs: Box<[u8]>,
    output_limit: usize,
    parents: Box<[Parent]>,
    _kind: PhantomData<fn() -> K>,
}

/// One post-execution value whose exact pending bundle has already supplied
/// the discovered inputs needed to complete its action key.
pub(crate) struct Bundled<K> {
    value: K,
    bundle: crate::store::bundle::Pending,
    discovered_inputs: Box<[u8]>,
}

impl<K> Bundled<K> {
    pub(crate) fn validate(
        value: K,
        bundle: crate::store::bundle::Pending,
        validate: impl FnOnce(&K, &crate::store::bundle::Pending) -> Result<Box<[u8]>, Error>,
    ) -> Result<Self, Error> {
        let discovered_inputs = validate(&value, &bundle)?;
        Ok(Self {
            value,
            bundle,
            discovered_inputs,
        })
    }

    fn into_parts(self) -> (K, crate::store::bundle::Pending, Box<[u8]>) {
        (self.value, self.bundle, self.discovered_inputs)
    }
}

impl<K> Clone for Direct<K> {
    fn clone(&self) -> Self {
        Self {
            action: self.action,
            recipe: self.recipe,
            context: self.context,
            output_limit: self.output_limit,
            parents: self.parents.clone(),
            _kind: PhantomData,
        }
    }
}

impl<K> Clone for Discoverable<K> {
    fn clone(&self) -> Self {
        Self {
            lookup: self.lookup,
            execution: self.execution,
            context: self.context,
            fixed_inputs: self.fixed_inputs.clone(),
            output_limit: self.output_limit,
            parents: self.parents.clone(),
            _kind: PhantomData,
        }
    }
}

impl<K: Value> Direct<K> {
    /// Bind exact semantic inputs and the sealed import context before lookup.
    pub(crate) fn capture(
        context: [u8; 32],
        semantic_inputs: &[u8],
        output_limit: usize,
        parents: &[Parent],
        parent_limit: usize,
    ) -> Result<Self, Error> {
        Self::capture_edges(
            context,
            semantic_inputs,
            output_limit,
            parents,
            parent_limit,
        )
    }

    fn capture_edges(
        context: [u8; 32],
        semantic_inputs: &[u8],
        output_limit: usize,
        parents: &[impl content::Edge],
        parent_limit: usize,
    ) -> Result<Self, Error> {
        if K::SCHEMA == 0 {
            return Err(Error::Codec("node output schema zero is reserved"));
        }
        if semantic_inputs.len() > MAX_RECIPE_BYTES {
            return Err(Error::Limit {
                resource: "node recipe bytes",
                limit: MAX_RECIPE_BYTES,
                actual: semantic_inputs.len(),
            });
        }
        if output_limit == 0 {
            return Err(Error::Codec("node content limit zero is reserved"));
        }
        if output_limit > MAX_CONTENT_BYTES {
            return Err(Error::Limit {
                resource: "node content bytes",
                limit: MAX_CONTENT_BYTES,
                actual: output_limit,
            });
        }
        record_parents(parents.len(), parent_limit);
        let mut retained_parents = Vec::new();
        retained_parents
            .try_reserve_exact(parents.len())
            .map_err(|_| Error::Allocation)?;
        retained_parents.extend(parents.iter().map(content::Provenance::from_edge));
        let context = Digest::from_bytes(context);
        let recipe = Digest::recipe(context, semantic_inputs, parents);
        let action = ActionKey::derive(
            K::KIND,
            K::SCHEMA,
            context,
            semantic_inputs,
            output_limit,
            parents,
        );
        Ok(Self {
            action,
            recipe,
            context,
            output_limit,
            parents: retained_parents.into_boxed_slice(),
            _kind: PhantomData,
        })
    }

    /// The exact action identity used for single-flight and publication.
    #[must_use]
    pub const fn action(&self) -> ActionKey {
        self.action
    }

    pub(crate) const fn output_limit(&self) -> usize {
        self.output_limit
    }

    pub(crate) const fn parent_count(&self) -> usize {
        self.parents.len()
    }

    pub(crate) fn parents(&self) -> &[content::Provenance] {
        &self.parents
    }

    fn in_flight(&self, store: &crate::store::Store) -> Result<Option<ReadSetProof>, Error> {
        read_set::ReadSetProof::from_parents(store, &self.parents)
    }
}

impl<K: Value> Discoverable<K> {
    pub(crate) fn capture(
        context: [u8; 32],
        fixed_inputs: &[u8],
        output_limit: usize,
        parents: &[Parent],
        parent_limit: usize,
    ) -> Result<Self, Error> {
        validate_recipe_bounds::<K>(
            fixed_inputs.len(),
            output_limit,
            parents.len(),
            parent_limit,
        )?;
        let mut retained_inputs = Vec::new();
        retained_inputs
            .try_reserve_exact(fixed_inputs.len())
            .map_err(|_| Error::Allocation)?;
        retained_inputs.extend_from_slice(fixed_inputs);
        let mut retained_parents = Vec::new();
        retained_parents
            .try_reserve_exact(parents.len())
            .map_err(|_| Error::Allocation)?;
        retained_parents.extend_from_slice(parents);
        let context = Digest::from_bytes(context);
        let lookup = LookupKey::derive(
            K::KIND,
            K::SCHEMA,
            context,
            fixed_inputs,
            output_limit,
            parents,
        );
        Ok(Self {
            lookup,
            execution: ExecutionKey::derive(lookup),
            context,
            fixed_inputs: retained_inputs.into_boxed_slice(),
            output_limit,
            parents: retained_parents.into_boxed_slice(),
            _kind: PhantomData,
        })
    }

    fn complete(&self, discovered_inputs: &[u8]) -> Result<Direct<K>, Error> {
        let retained = 16_usize
            .checked_add(self.fixed_inputs.len())
            .and_then(|length| length.checked_add(discovered_inputs.len()))
            .unwrap_or(usize::MAX);
        if retained > MAX_RECIPE_BYTES {
            return Err(Error::Limit {
                resource: "node recipe bytes",
                limit: MAX_RECIPE_BYTES,
                actual: retained,
            });
        }
        let mut semantic_inputs = Vec::new();
        semantic_inputs
            .try_reserve_exact(retained)
            .map_err(|_| Error::Allocation)?;
        semantic_inputs.extend_from_slice(&(self.fixed_inputs.len() as u64).to_le_bytes());
        semantic_inputs.extend_from_slice(&self.fixed_inputs);
        semantic_inputs.extend_from_slice(&(discovered_inputs.len() as u64).to_le_bytes());
        semantic_inputs.extend_from_slice(discovered_inputs);
        Direct::capture(
            self.context.as_bytes(),
            &semantic_inputs,
            self.output_limit,
            &self.parents,
            self.parents.len(),
        )
    }

    /// Non-authoritative bounded candidate-bucket identity.
    #[must_use]
    pub const fn lookup(&self) -> LookupKey {
        self.lookup
    }

    pub(crate) const fn output_limit(&self) -> usize {
        self.output_limit
    }

    pub(crate) const fn parent_count(&self) -> usize {
        self.parents.len()
    }

    fn in_flight(&self, store: &crate::store::Store) -> Result<Option<ReadSetProof>, Error> {
        read_set::ReadSetProof::from_parents(store, &self.parents)
    }

    pub(crate) const fn execution(&self) -> ExecutionKey {
        self.execution
    }

    pub(crate) fn parents(&self) -> &[Parent] {
        &self.parents
    }

    /// Refresh the full parent union at the actual consumer after its waits.
    pub(crate) fn revalidate_parents(&self, store: &crate::store::Store) -> Result<(), Error> {
        self.in_flight(store).map(|_| ())
    }
}

/// The direct-parent count is measured against the `edges` accounting
/// target (`parent_limit`) and only recorded; the node envelope's physical
/// parent bound (`codec::MAX_PARENTS`) still refuses at encoding.
fn record_parents(parents: usize, parent_limit: usize) {
    if parents > parent_limit {
        crate::surface::account::Account::new(crate::exceedance::Account::Graph).exceeded(
            "direct node parents",
            parent_limit,
            parents,
        );
    }
}

fn validate_recipe_bounds<K: Value>(
    recipe_bytes: usize,
    output_limit: usize,
    parents: usize,
    parent_limit: usize,
) -> Result<(), Error> {
    if K::SCHEMA == 0 {
        return Err(Error::Codec("node output schema zero is reserved"));
    }
    if recipe_bytes > MAX_RECIPE_BYTES {
        return Err(Error::Limit {
            resource: "node recipe bytes",
            limit: MAX_RECIPE_BYTES,
            actual: recipe_bytes,
        });
    }
    if output_limit == 0 {
        return Err(Error::Codec("node content limit zero is reserved"));
    }
    if output_limit > MAX_CONTENT_BYTES {
        return Err(Error::Limit {
            resource: "node content bytes",
            limit: MAX_CONTENT_BYTES,
            actual: output_limit,
        });
    }
    record_parents(parents, parent_limit);
    Ok(())
}

/// One direct-node evaluator bound to a durable store and observational sink.
pub struct Evaluator<'store> {
    store: &'store crate::store::Store,
    metrics: &'store dyn metrics::Sink,
}

/// The authenticated result of one lookup, not a presence probe. Missing work
/// retains its recovery disposition for the bounded continuation.
// Returned by every lookup and matched at once; boxing the hit would allocate on the hot path.
#[allow(clippy::large_enum_variant)]
pub(crate) enum Lookup<K> {
    Hit(Validated<K>),
    Miss(Miss),
}

pub(crate) enum Miss {
    Absent,
    RecoveryRequired,
}

impl Miss {
    pub(crate) const fn recovery_required(&self) -> bool {
        matches!(self, Self::RecoveryRequired)
    }
}

const MAX_DEEP_VALIDATIONS_PER_LOOKUP: usize = 2;
pub(crate) const MAX_DEEP_VALIDATIONS_PER_GRAPH: usize = 8;
pub(crate) const MAX_DISCOVERABLE_LOOKUPS_PER_GRAPH: usize = 4;
pub(crate) const MAX_CANDIDATE_COMPARISONS_PER_GRAPH: usize = 64;
pub(crate) const MAX_DECODED_BYTES_PER_GRAPH: u64 = 32 * 1024 * 1024;

#[cfg(test)]
thread_local! {
    /// Graph decode charges on this test thread: call site → (calls, bytes).
    pub(crate) static DECODE_SITES: std::cell::RefCell<
        std::collections::BTreeMap<String, (usize, u64)>,
    > = const { std::cell::RefCell::new(std::collections::BTreeMap::new()) };
    /// Graph path-check charges on this test thread: call site → (calls, checks).
    pub(crate) static PATH_SITES: std::cell::RefCell<
        std::collections::BTreeMap<String, (usize, u64)>,
    > = const { std::cell::RefCell::new(std::collections::BTreeMap::new()) };
}
pub(crate) const MAX_PATH_CHECKS_PER_GRAPH: usize = 65_536;
pub(crate) const MAX_HASHED_BYTES_PER_GRAPH: u64 = 8 * 1024 * 1024 * 1024;

pub(crate) const LOOKUPS_RESOURCE: &str = "typed store discoverable lookups per graph";
pub(crate) const COMPARISONS_RESOURCE: &str = "typed store candidate comparisons per graph";
pub(crate) const DEEP_RESOURCE: &str = "typed store deep candidate validations per graph";
pub(crate) const DECODED_RESOURCE: &str = "typed store decoded candidate bytes per graph";
pub(crate) const PATHS_RESOURCE: &str = "typed store candidate path checks per graph";
pub(crate) const HASHED_RESOURCE: &str = "typed store hashed candidate bytes per graph";

/// Per-graph accounting targets. Crossing one is recorded and warned,
/// never refused; the standard values are the targets the implementation
/// optimizes toward.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct GraphTargets {
    pub(crate) lookups: usize,
    pub(crate) comparisons: usize,
    pub(crate) deep: usize,
    pub(crate) decoded: u64,
    pub(crate) paths: usize,
    pub(crate) hashed: u64,
}

impl GraphTargets {
    /// The standard per-graph targets.
    pub const STANDARD: Self = Self {
        lookups: MAX_DISCOVERABLE_LOOKUPS_PER_GRAPH,
        comparisons: MAX_CANDIDATE_COMPARISONS_PER_GRAPH,
        deep: MAX_DEEP_VALIDATIONS_PER_GRAPH,
        decoded: MAX_DECODED_BYTES_PER_GRAPH,
        paths: MAX_PATH_CHECKS_PER_GRAPH,
        hashed: MAX_HASHED_BYTES_PER_GRAPH,
    };
}

impl Default for GraphTargets {
    fn default() -> Self {
        Self::STANDARD
    }
}

#[derive(Clone)]
pub(crate) struct GraphBudget {
    usage: Arc<Mutex<GraphUsage>>,
    accounts: Arc<account::Registry>,
    targets: GraphTargets,
    /// Process-unique account identity shared by every clone of this graph.
    id: u64,
}

#[derive(Clone, Copy, Default)]
struct GraphUsage {
    lookups: usize,
    comparisons: usize,
    deep: usize,
    decoded: u64,
    paths: usize,
    hashed: u64,
    exceeded: crate::exceedance::Record,
}

impl GraphBudget {
    #[cfg(test)]
    pub(crate) fn census_vector(&self) -> Result<[u64; 6], Error> {
        let usage = self
            .usage
            .lock()
            .map_err(|_| Error::State("graph census lock poisoned"))?;
        let count =
            |value| u64::try_from(value).map_err(|_| Error::State("graph census count overflow"));
        Ok([
            count(usage.lookups)?,
            count(usage.comparisons)?,
            count(usage.deep)?,
            usage.decoded,
            count(usage.paths)?,
            usage.hashed,
        ])
    }

    #[cfg(test)]
    pub(crate) fn census_usage(&self) -> Result<(u64, usize, u64), Error> {
        let usage = self
            .usage
            .lock()
            .map_err(|_| Error::State("graph census lock poisoned"))?;
        Ok((usage.decoded, usage.paths, usage.hashed))
    }

    pub(crate) fn shares_with(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.usage, &other.usage)
    }

    #[cfg(test)]
    pub(crate) fn new() -> Self {
        Self::with_targets(GraphTargets::STANDARD)
    }

    pub(crate) fn with_targets(targets: GraphTargets) -> Self {
        Self {
            usage: Arc::new(Mutex::new(GraphUsage::default())),
            accounts: Arc::new(account::Registry::default()),
            targets,
            id: crate::exceedance::next_id(),
        }
    }

    /// The accounting targets this graph is measured against.
    pub(crate) const fn targets(&self) -> GraphTargets {
        self.targets
    }

    /// Process-unique identity of this graph account.
    #[cfg(test)]
    pub(crate) const fn id(&self) -> u64 {
        self.id
    }

    /// The axes of this graph that crossed their targets so far.
    #[cfg(test)]
    pub(crate) fn exceedances(&self) -> crate::exceedance::Record {
        self.usage
            .lock()
            .map(|usage| usage.exceeded)
            .unwrap_or_default()
    }

    pub(crate) fn lookup(&self) -> Result<(), Error> {
        self.charge_usize(
            |usage| &mut usage.lookups,
            1,
            self.targets.lookups,
            LOOKUPS_RESOURCE,
            None,
        )
    }

    pub(crate) fn comparison(&self) -> Result<(), Error> {
        self.charge_usize(
            |usage| &mut usage.comparisons,
            1,
            self.targets.comparisons,
            COMPARISONS_RESOURCE,
            None,
        )
    }

    fn deep(&self) -> Result<(), Error> {
        self.charge_usize(
            |usage| &mut usage.deep,
            1,
            self.targets.deep,
            DEEP_RESOURCE,
            None,
        )
    }

    #[track_caller]
    pub(crate) fn decode(&self, bytes: usize) -> Result<(), Error> {
        let caller = std::panic::Location::caller();
        #[cfg(test)]
        DECODE_SITES.with(|sites| {
            let mut sites = sites.borrow_mut();
            let entry = sites
                .entry(format!("{}:{}", caller.file(), caller.line()))
                .or_default();
            entry.0 += 1;
            entry.1 += bytes as u64;
        });
        self.charge_u64(
            |usage| &mut usage.decoded,
            u64::try_from(bytes).map_err(|_| Error::Allocation)?,
            self.targets.decoded,
            DECODED_RESOURCE,
            Some(caller),
        )
    }

    #[track_caller]
    pub(crate) fn paths(&self, paths: usize) -> Result<(), Error> {
        let caller = std::panic::Location::caller();
        #[cfg(test)]
        PATH_SITES.with(|sites| {
            let mut sites = sites.borrow_mut();
            let entry = sites
                .entry(format!("{}:{}", caller.file(), caller.line()))
                .or_default();
            entry.0 += 1;
            entry.1 += paths as u64;
        });
        self.charge_usize(
            |usage| &mut usage.paths,
            paths,
            self.targets.paths,
            PATHS_RESOURCE,
            Some(caller),
        )
    }

    pub(crate) fn hash(&self, bytes: usize) -> Result<(), Error> {
        self.charge_u64(
            |usage| &mut usage.hashed,
            u64::try_from(bytes).map_err(|_| Error::Allocation)?,
            self.targets.hashed,
            HASHED_RESOURCE,
            None,
        )
    }

    // An accounting axis never yields `Error::Limit`. Every charge is
    // added to the original account; crossing the target records the axis,
    // target, requested amount and call site, and the charge still succeeds.
    // Only checked-arithmetic overflow remains terminal.
    fn charge_usize(
        &self,
        select: impl FnOnce(&mut GraphUsage) -> &mut usize,
        amount: usize,
        target: usize,
        resource: &'static str,
        site: Option<&'static std::panic::Location<'static>>,
    ) -> Result<(), Error> {
        let mut usage = self
            .usage
            .lock()
            .map_err(|_| Error::State("typed store graph budget lock was poisoned"))?;
        let current = select(&mut usage);
        let requested = current.checked_add(amount);
        let (result, exceeded) = match requested {
            None => (Err(Error::Allocation), false),
            Some(actual) => {
                *current = actual;
                let exceeded = actual > target
                    && usage
                        .exceeded
                        .exceeded(resource, target as u64, actual as u64, site);
                (Ok(()), exceeded)
            }
        };
        self.observe_charge(usage, result, exceeded)
    }

    fn charge_u64(
        &self,
        select: impl FnOnce(&mut GraphUsage) -> &mut u64,
        amount: u64,
        target: u64,
        resource: &'static str,
        site: Option<&'static std::panic::Location<'static>>,
    ) -> Result<(), Error> {
        let mut usage = self
            .usage
            .lock()
            .map_err(|_| Error::State("typed store graph budget lock was poisoned"))?;
        let current = select(&mut usage);
        let requested = current.checked_add(amount);
        let (result, exceeded) = match requested {
            None => (Err(Error::Allocation), false),
            Some(actual) => {
                *current = actual;
                let exceeded =
                    actual > target && usage.exceeded.exceeded(resource, target, actual, site);
                (Ok(()), exceeded)
            }
        };
        self.observe_charge(usage, result, exceeded)
    }

    /// Release the account lock, then publish the exceedance record. A write-only sink is still arbitrary caller code:
    /// never invoke it while the original account lock is held, and never
    /// change the original result.
    fn observe_charge(
        &self,
        usage: std::sync::MutexGuard<'_, GraphUsage>,
        result: Result<(), Error>,
        exceeded: bool,
    ) -> Result<(), Error> {
        let record = usage.exceeded;
        drop(usage);
        if exceeded {
            crate::exceedance::publish(crate::exceedance::Account::Graph, self.id, &record);
        }
        result
    }

    /// Test helper: charge comparisons up to exactly the target.
    #[cfg(test)]
    pub(crate) fn exhaust_comparisons(&self) {
        let spent = self.usage.lock().unwrap().comparisons;
        for _ in spent..self.targets.comparisons {
            self.comparison().unwrap();
        }
    }

    /// Test helper: charge path checks up to exactly the target.
    #[cfg(test)]
    pub(crate) fn exhaust_paths(&self) {
        let spent = self.usage.lock().unwrap().paths;
        self.paths(self.targets.paths.saturating_sub(spent))
            .unwrap();
    }

    #[cfg(test)]
    pub(crate) fn deep_validations(&self) -> usize {
        self.usage
            .lock()
            .map(|usage| usage.deep)
            .unwrap_or(usize::MAX)
    }
}

impl<'store> Evaluator<'store> {
    /// Bind evaluation to one already-opened versioned store.
    #[must_use]
    pub const fn new(
        store: &'store crate::store::Store,
        metrics: &'store dyn metrics::Sink,
    ) -> Self {
        Self { store, metrics }
    }

    /// Validate a hit or execute, seal, and revalidate one exact miss.
    pub fn direct<K: Value>(
        &self,
        recipe: &Direct<K>,
        execute: impl FnOnce() -> Result<K, Error>,
    ) -> Result<Validated<K>, Error> {
        let miss = match self.lookup(recipe)? {
            Lookup::Hit(hit) => return Ok(hit),
            Lookup::Miss(miss) => miss,
        };
        self.store.with_ownership(|ownership| {
            self.execute_direct(ownership, recipe, execute, miss.recovery_required())
        })
    }

    /// Authenticate and decode an exact hit once, without executing a transform,
    /// acquiring an execution lease, or replaying mutable ancestor witnesses.
    /// Operational refusals stay errors; only cleanup-required state becomes a
    /// recovery disposition. Continuations must still recheck after waiting.
    pub(crate) fn lookup<K: Value>(&self, recipe: &Direct<K>) -> Result<Lookup<K>, Error> {
        let outcome = self.lookup_outcome(recipe)?;
        match &outcome {
            Lookup::Hit(_) => self
                .metrics
                .node(K::KIND.metric(), metrics::NodeEvent::ValidatedHit),
            Lookup::Miss(Miss::RecoveryRequired) => self
                .metrics
                .node(K::KIND.metric(), metrics::NodeEvent::RejectedHit),
            Lookup::Miss(Miss::Absent) => {}
        }
        Ok(outcome)
    }

    fn lookup_outcome<K: Value>(&self, recipe: &Direct<K>) -> Result<Lookup<K>, Error> {
        match self.store.lookup(recipe, self.store.graph_budget()) {
            Ok(Some(hit)) => Ok(Lookup::Hit(hit)),
            Ok(None) => Ok(Lookup::Miss(Miss::Absent)),
            Err(Error::CleanupRequired(_)) => Ok(Lookup::Miss(Miss::RecoveryRequired)),
            Err(error) => {
                self.metrics
                    .node(K::KIND.metric(), metrics::NodeEvent::RejectedHit);
                Err(error)
            }
        }
    }

    /// Compose an owned pure output under retention, then promote it through
    /// the complete current union before releasing its parent pin.
    pub(crate) fn staged_direct<K: Value>(
        &self,
        recipe: &Direct<K>,
        retention: &read_set::Retention,
        execute: impl FnOnce() -> Result<K, Error>,
    ) -> Result<Validated<K>, Error> {
        retention.require_parents(&recipe.parents)?;
        let mut rejected = false;
        if self.lookup_staged(recipe, &mut rejected)?.is_some() {
            return self.store.with_existing_ownership(|ownership| {
                self.staged_direct_owned(ownership, recipe, retention, execute, rejected)
            });
        }
        self.store.with_ownership(|ownership| {
            self.staged_direct_owned(ownership, recipe, retention, execute, rejected)
        })
    }

    fn staged_direct_owned<K: Value>(
        &self,
        ownership: &crate::store::Ownership<'_>,
        recipe: &Direct<K>,
        retention: &read_set::Retention,
        execute: impl FnOnce() -> Result<K, Error>,
        mut rejected: bool,
    ) -> Result<Validated<K>, Error> {
        if let Some(staged) = self.lookup_staged(recipe, &mut rejected)? {
            let current = self
                .store
                .with_retained_in_flight(ownership, retention, || {
                    staged.validate_current(self.store)
                })?;
            self.metrics
                .node(K::KIND.metric(), metrics::NodeEvent::ValidatedHit);
            return Ok(current);
        }
        self.store.with_action_lease(recipe.action, || {
            if let Some(staged) = self.lookup_staged(recipe, &mut rejected)? {
                let current = self
                    .store
                    .with_retained_in_flight(ownership, retention, || {
                        staged.validate_current(self.store)
                    })?;
                self.metrics
                    .node(K::KIND.metric(), metrics::NodeEvent::ValidatedHit);
                return Ok(current);
            }
            self.store.with_staged_action_reservation(
                ownership,
                recipe,
                retention,
                || {
                    self.metrics
                        .node(K::KIND.metric(), metrics::NodeEvent::ExecutedMiss);
                    let bytes = execute()?.into_encoded(recipe.output_limit)?;
                    if bytes.len() > recipe.output_limit {
                        return Err(Error::Limit {
                            resource: "node content bytes",
                            limit: recipe.output_limit,
                            actual: bytes.len(),
                        });
                    }
                    Ok(crate::store::Reserved::Publish {
                        result: (),
                        recipe: recipe.clone(),
                        bytes,
                        bundle: None,
                    })
                },
                |()| {
                    self.metrics
                        .node(K::KIND.metric(), metrics::NodeEvent::Published);
                    self.store
                        .lookup_staged(recipe, self.store.graph_budget())?
                        .ok_or(Error::State(
                            "published staged node disappeared before promotion",
                        ))?
                        .validate_current(self.store)
                },
            )
        })
    }

    fn lookup_staged<K: Value>(
        &self,
        recipe: &Direct<K>,
        rejected: &mut bool,
    ) -> Result<Option<replay::Staged<K>>, Error> {
        match self.store.lookup_staged(recipe, self.store.graph_budget()) {
            Ok(value) => Ok(value),
            Err(Error::CleanupRequired(_)) => {
                if !*rejected {
                    self.metrics
                        .node(K::KIND.metric(), metrics::NodeEvent::RejectedHit);
                    *rejected = true;
                }
                Ok(None)
            }
            Err(error) => {
                self.metrics
                    .node(K::KIND.metric(), metrics::NodeEvent::RejectedHit);
                Err(error)
            }
        }
    }

    /// Evaluate an exact native action without taking execution leases on hits.
    pub(crate) fn native<K: Value>(
        &self,
        recipe: &Direct<K>,
        execute: impl FnOnce() -> Result<K, Error>,
    ) -> Result<Validated<K>, Error> {
        let mut recovery_required = false;
        if let Some(hit) = self.lookup_direct(recipe, &mut recovery_required)? {
            return Ok(hit);
        }
        self.store.with_ownership(|ownership| {
            self.store.with_heavy_lease(|| {
                if let Some(hit) = self.lookup_direct(recipe, &mut recovery_required)? {
                    return Ok(hit);
                }
                self.execute_direct(ownership, recipe, execute, recovery_required)
            })
        })
    }

    fn lookup_direct<K: Value>(
        &self,
        recipe: &Direct<K>,
        recovery_required: &mut bool,
    ) -> Result<Option<Validated<K>>, Error> {
        match self.lookup_outcome(recipe)? {
            Lookup::Hit(hit) => {
                self.metrics
                    .node(K::KIND.metric(), metrics::NodeEvent::ValidatedHit);
                Ok(Some(hit))
            }
            Lookup::Miss(Miss::Absent) => Ok(None),
            Lookup::Miss(Miss::RecoveryRequired) => {
                if !*recovery_required {
                    self.metrics
                        .node(K::KIND.metric(), metrics::NodeEvent::RejectedHit);
                    *recovery_required = true;
                }
                Ok(None)
            }
        }
    }

    fn execute_direct<K: Value>(
        &self,
        ownership: &crate::store::Ownership<'_>,
        recipe: &Direct<K>,
        execute: impl FnOnce() -> Result<K, Error>,
        mut recovery_required: bool,
    ) -> Result<Validated<K>, Error> {
        let mut execute = Some(execute);
        self.store.with_action_lease(recipe.action, || {
            if let Some(hit) = self.lookup_direct(recipe, &mut recovery_required)? {
                return Ok(hit);
            }

            let in_flight = recipe.in_flight(self.store)?;
            self.store
                .with_action_reservation(ownership, recipe, in_flight.as_ref(), || {
                    self.metrics
                        .node(K::KIND.metric(), metrics::NodeEvent::ExecutedMiss);
                    let transform = execute.take().ok_or(Error::State(
                        "direct node executor was consumed before the final miss",
                    ))?;
                    let bytes = transform()?.into_encoded(recipe.output_limit)?;
                    if bytes.len() > recipe.output_limit {
                        return Err(Error::Limit {
                            resource: "node content bytes",
                            limit: recipe.output_limit,
                            actual: bytes.len(),
                        });
                    }
                    Ok(crate::store::Reserved::Publish {
                        result: (),
                        recipe: recipe.clone(),
                        bytes,
                        bundle: None,
                    })
                })?;
            self.metrics
                .node(K::KIND.metric(), metrics::NodeEvent::Published);
            self.store
                .lookup(recipe, self.store.graph_budget())?
                .ok_or(Error::State(
                    "published direct node was absent during exact revalidation",
                ))
        })
    }

    /// Validate a bounded candidate or execute and publish one discovered action.
    pub fn discover<K>(
        &self,
        recipe: &Discoverable<K>,
        validate: impl Fn(&K) -> Result<Box<[u8]>, Error> + Send + Sync + 'static,
        execute: impl FnOnce() -> Result<K, Error>,
    ) -> Result<Validated<K>, Error>
    where
        K: Value + Send + Sync + 'static,
    {
        self.discover_budgeted(recipe, move |value, _budget| validate(value), execute)
    }

    pub(crate) fn discover_budgeted<K>(
        &self,
        recipe: &Discoverable<K>,
        validate: impl Fn(&K, &GraphBudget) -> Result<Box<[u8]>, Error> + Send + Sync + 'static,
        execute: impl FnOnce() -> Result<K, Error>,
    ) -> Result<Validated<K>, Error>
    where
        K: Value + Send + Sync + 'static,
    {
        let validate: read_set::ScopedValidator<K> =
            Arc::new(move |value, budget, _| validate(value, budget));
        let budget = self.store.graph_budget();
        // Candidates found stale stay stale for this evaluation: the rechecks
        // under the leases look only for a concurrently published one.
        let mut stale = Vec::new();
        let first = self.lookup_discoverable_scoped(recipe, &validate, budget, &mut stale)?;
        if let Some(hit) = first.hit {
            self.metrics
                .node(K::KIND.metric(), metrics::NodeEvent::ValidatedHit);
            return Ok(hit);
        }
        if first.rejected {
            self.metrics
                .node(K::KIND.metric(), metrics::NodeEvent::RejectedHit);
        }

        let mut rejection_recorded = first.rejected;
        let mut execute = Some(execute);
        self.store.with_ownership(|ownership| {
            self.store.with_heavy_lease(|| {
                let after_heavy =
                    self.lookup_discoverable_scoped(recipe, &validate, budget, &mut stale)?;
                if let Some(hit) = after_heavy.hit {
                    self.metrics
                        .node(K::KIND.metric(), metrics::NodeEvent::ValidatedHit);
                    return Ok(hit);
                }
                if after_heavy.rejected && !rejection_recorded {
                    self.metrics
                        .node(K::KIND.metric(), metrics::NodeEvent::RejectedHit);
                    rejection_recorded = true;
                }

                self.store.with_execution_lease(recipe.execution, || {
                    let rechecked =
                        self.lookup_discoverable_scoped(recipe, &validate, budget, &mut stale)?;
                    if let Some(hit) = rechecked.hit {
                        self.metrics
                            .node(K::KIND.metric(), metrics::NodeEvent::ValidatedHit);
                        return Ok(hit);
                    }
                    if rechecked.rejected && !rejection_recorded {
                        self.metrics
                            .node(K::KIND.metric(), metrics::NodeEvent::RejectedHit);
                        rejection_recorded = true;
                    }

                    let in_flight = recipe.in_flight(self.store)?;
                    let (action, published) = self.store.with_execution_reservation(
                        ownership,
                        recipe,
                        in_flight.as_ref(),
                        || {
                            self.metrics
                                .node(K::KIND.metric(), metrics::NodeEvent::ExecutedMiss);
                            let transform = execute.take().ok_or(Error::State(
                                "discoverable node executor was consumed before the final miss",
                            ))?;
                            let value = transform()?;
                            let discovered_inputs =
                                validate(&value, budget, read_set::Scope::Consumer)?;
                            let complete = recipe.complete(&discovered_inputs)?;
                            let bytes = value.encode(recipe.output_limit)?;
                            if bytes.len() > recipe.output_limit {
                                return Err(Error::Limit {
                                    resource: "node content bytes",
                                    limit: recipe.output_limit,
                                    actual: bytes.len(),
                                });
                            }
                            if self
                                .replay_discovered_scoped_exact(
                                    recipe,
                                    &validate,
                                    complete.action(),
                                )?
                                .is_some()
                            {
                                return Ok(crate::store::Reserved::Reuse((
                                    complete.action(),
                                    false,
                                )));
                            }
                            Ok(crate::store::Reserved::Publish {
                                result: (complete.action(), true),
                                recipe: complete,
                                bytes,
                                bundle: None,
                            })
                        },
                    )?;
                    self.store
                        .hint_candidate::<K>(recipe.lookup, action, rejection_recorded);
                    if published {
                        self.metrics
                            .node(K::KIND.metric(), metrics::NodeEvent::Published);
                    }
                    self.replay_discovered_scoped_exact(recipe, &validate, action)?
                        .ok_or(Error::State(
                            "published discoverable node was absent during exact revalidation",
                        ))
                })
            })
        })
    }

    /// Validate a discoverable bundle hit or allocate and execute one fresh
    /// immutable bundle only after every heavyweight and exact-key recheck.
    #[cfg(test)]
    pub(crate) fn discover_bundled<K>(
        &self,
        recipe: &Discoverable<K>,
        validate: impl Fn(&K) -> Result<Box<[u8]>, Error> + Send + Sync + 'static,
        execute: impl FnOnce(crate::store::bundle::Attempt) -> Result<Bundled<K>, Error>,
    ) -> Result<Validated<K>, Error>
    where
        K: Value + Send + Sync + 'static,
    {
        self.discover_bundled_budgeted(recipe, move |value, _budget| validate(value), execute)
    }

    pub(crate) fn discover_bundled_budgeted<K>(
        &self,
        recipe: &Discoverable<K>,
        validate: impl Fn(&K, &GraphBudget) -> Result<Box<[u8]>, Error> + Send + Sync + 'static,
        execute: impl FnOnce(crate::store::bundle::Attempt) -> Result<Bundled<K>, Error>,
    ) -> Result<Validated<K>, Error>
    where
        K: Value + Send + Sync + 'static,
    {
        self.discover_bundled_with_retention(
            recipe,
            None,
            move |value, budget, _| validate(value, budget),
            execute,
        )
    }

    /// Retain exact parent allocations during reservation without producing
    /// current consumer authority. The executor must refresh at its actual
    /// consumer after any additional waits it performs.
    pub(crate) fn discover_bundled_retained_budgeted<K>(
        &self,
        recipe: &Discoverable<K>,
        retention: &read_set::Retention,
        validate: impl Fn(&K, &GraphBudget) -> Result<Box<[u8]>, Error> + Send + Sync + 'static,
        execute: impl FnOnce(crate::store::bundle::Attempt) -> Result<Bundled<K>, Error>,
    ) -> Result<Validated<K>, Error>
    where
        K: Value + Send + Sync + 'static,
    {
        retention.covers_parents(&recipe.parents)?;
        self.discover_bundled_with_retention(
            recipe,
            Some(retention),
            move |value, budget, _| validate(value, budget),
            execute,
        )
    }

    fn discover_bundled_with_retention<K>(
        &self,
        recipe: &Discoverable<K>,
        retention: Option<&read_set::Retention>,
        validate: impl Fn(&K, &GraphBudget, read_set::Scope) -> Result<Box<[u8]>, Error>
            + Send
            + Sync
            + 'static,
        execute: impl FnOnce(crate::store::bundle::Attempt) -> Result<Bundled<K>, Error>,
    ) -> Result<Validated<K>, Error>
    where
        K: Value + Send + Sync + 'static,
    {
        let validate: read_set::ScopedValidator<K> = Arc::new(validate);
        let budget = self.store.graph_budget();
        // Candidates found stale stay stale for this evaluation: the rechecks
        // under the leases look only for a concurrently published one.
        let mut stale = Vec::new();
        let first = self.lookup_discoverable_scoped(recipe, &validate, budget, &mut stale)?;
        if let Some(hit) = first.hit {
            self.metrics
                .node(K::KIND.metric(), metrics::NodeEvent::ValidatedHit);
            return Ok(hit);
        }
        if first.rejected {
            self.metrics
                .node(K::KIND.metric(), metrics::NodeEvent::RejectedHit);
        }

        let mut rejection_recorded = first.rejected;
        let mut execute = Some(execute);
        self.store.with_ownership(|ownership| {
            self.store.with_heavy_lease(|| {
                let after_heavy =
                    self.lookup_discoverable_scoped(recipe, &validate, budget, &mut stale)?;
                if let Some(hit) = after_heavy.hit {
                    self.metrics
                        .node(K::KIND.metric(), metrics::NodeEvent::ValidatedHit);
                    return Ok(hit);
                }
                if after_heavy.rejected && !rejection_recorded {
                    self.metrics
                        .node(K::KIND.metric(), metrics::NodeEvent::RejectedHit);
                    rejection_recorded = true;
                }

                self.store.with_execution_lease(recipe.execution, || {
                    let rechecked =
                        self.lookup_discoverable_scoped(recipe, &validate, budget, &mut stale)?;
                    if let Some(hit) = rechecked.hit {
                        self.metrics
                            .node(K::KIND.metric(), metrics::NodeEvent::ValidatedHit);
                        return Ok(hit);
                    }
                    if rechecked.rejected && !rejection_recorded {
                        self.metrics
                            .node(K::KIND.metric(), metrics::NodeEvent::RejectedHit);
                        rejection_recorded = true;
                    }

                    let evaluate = |attempt| {
                        self.metrics
                            .node(K::KIND.metric(), metrics::NodeEvent::ExecutedMiss);
                        let transform = execute.take().ok_or(Error::State(
                            "bundled node executor was consumed before the final miss",
                        ))?;
                        let (value, bundle, discovered_inputs) = transform(attempt)?.into_parts();
                        let complete = recipe.complete(&discovered_inputs)?;
                        let bytes = value.encode(recipe.output_limit)?;
                        if bytes.len() > recipe.output_limit {
                            return Err(Error::Limit {
                                resource: "node content bytes",
                                limit: recipe.output_limit,
                                actual: bytes.len(),
                            });
                        }
                        if self
                            .replay_discovered_scoped_exact(recipe, &validate, complete.action())?
                            .is_some()
                        {
                            return Ok(crate::store::Reserved::Reuse((complete.action(), false)));
                        }
                        Ok(crate::store::Reserved::Publish {
                            result: (complete.action(), true),
                            recipe: complete,
                            bytes,
                            bundle: Some(bundle),
                        })
                    };
                    let (action, published) = match retention {
                        Some(retention) => self.store.with_retained_execution_bundle_reservation(
                            ownership,
                            recipe,
                            retention,
                            &rechecked.rejected_actions,
                            evaluate,
                        ),
                        None => {
                            let in_flight = recipe.in_flight(self.store)?;
                            self.store.with_rejected_execution_bundle_reservation(
                                ownership,
                                recipe,
                                in_flight.as_ref(),
                                &rechecked.rejected_actions,
                                evaluate,
                            )
                        }
                    }?;
                    self.store
                        .hint_candidate::<K>(recipe.lookup, action, rejection_recorded);
                    if published {
                        self.metrics
                            .node(K::KIND.metric(), metrics::NodeEvent::Published);
                    }
                    self.replay_discovered_scoped_exact(recipe, &validate, action)?
                        .ok_or(Error::State(
                            "published bundled node was absent during exact revalidation",
                        ))
                })
            })
        })
    }

    /// Seal and validate the exact deduplicated union of current graph receipts.
    pub fn seal_read_set(
        &self,
        fragments: impl IntoIterator<Item = Fragment>,
    ) -> Result<ReadSetProof, Error> {
        Fragment::seal(self.store, fragments)
    }

    /// Revalidate a previously sealed union immediately before admission.
    pub fn revalidate_read_set(&self, proof: &ReadSetProof) -> Result<(), Error> {
        proof.revalidate(self.store)
    }

    #[cfg(test)]
    fn lookup_discoverable<K>(
        &self,
        recipe: &Discoverable<K>,
        validate: &read_set::Validator<K>,
        budget: &GraphBudget,
    ) -> Result<Discovery<K>, Error>
    where
        K: Value + Send + Sync + 'static,
    {
        let validate = Arc::clone(validate);
        let scoped: read_set::ScopedValidator<K> =
            Arc::new(move |value, budget, _| validate(value, budget));
        self.lookup_discoverable_scoped(recipe, &scoped, budget, &mut Vec::new())
    }

    fn lookup_discoverable_scoped<K>(
        &self,
        recipe: &Discoverable<K>,
        validate: &read_set::ScopedValidator<K>,
        budget: &GraphBudget,
        stale: &mut Vec<ActionKey>,
    ) -> Result<Discovery<K>, Error>
    where
        K: Value + Send + Sync + 'static,
    {
        let candidates = self.store.candidates::<K>(recipe.lookup, budget)?;
        let mut admitted = None;
        let mut rejected = candidates.rejected;
        let mut rejected_actions = [None; MAX_DEEP_VALIDATIONS_PER_LOOKUP];
        for (slot, action) in candidates
            .actions
            .iter()
            .copied()
            .take(MAX_DEEP_VALIDATIONS_PER_LOOKUP)
            .enumerate()
        {
            // The deep-validation target records and warns; only a
            // genuine arithmetic failure stops the lookup here.
            if stale.contains(&action) {
                rejected = true;
                continue;
            }
            budget.deep()?;
            let current = match self.replay_discovered(recipe, validate, action) {
                Ok(Some(current)) => current,
                Err(Error::CandidateCorrupt(proof)) => {
                    note_damaged(K::KIND);
                    rejected = true;
                    rejected_actions[slot] = Some((action, *proof));
                    continue;
                }
                Err(error) if terminal_replay_failure(&error) => return Err(error),
                miss => {
                    // An ordinary input change rebuilds silently under the
                    // progress bar; only damaged entries earn a line.
                    if matches!(miss, Err(Error::Corrupt { .. })) {
                        note_damaged(K::KIND);
                    }
                    stale.push(action);
                    rejected = true;
                    continue;
                }
            };
            if admitted.is_some() {
                return Ok(Discovery {
                    hit: None,
                    rejected: true,
                    rejected_actions,
                });
            }
            admitted = Some(current);
        }
        if candidates.actions.len() > MAX_DEEP_VALIDATIONS_PER_LOOKUP {
            return Ok(Discovery {
                hit: None,
                rejected: true,
                rejected_actions,
            });
        }
        Ok(Discovery {
            hit: admitted,
            rejected,
            rejected_actions,
        })
    }

    pub(crate) fn replay_discovered_exact<K>(
        &self,
        recipe: &Discoverable<K>,
        validate: &read_set::Validator<K>,
        action: ActionKey,
    ) -> Result<Option<Validated<K>>, Error>
    where
        K: Value + Send + Sync + 'static,
    {
        let validate = Arc::clone(validate);
        let scoped: read_set::ScopedValidator<K> =
            Arc::new(move |value, budget, _| validate(value, budget));
        self.replay_discovered_scoped_exact(recipe, &scoped, action)
    }

    fn replay_discovered_scoped_exact<K>(
        &self,
        recipe: &Discoverable<K>,
        validate: &read_set::ScopedValidator<K>,
        action: ActionKey,
    ) -> Result<Option<Validated<K>>, Error>
    where
        K: Value + Send + Sync + 'static,
    {
        self.replay_discovered(recipe, validate, action)
    }

    fn replay_discovered<K>(
        &self,
        recipe: &Discoverable<K>,
        validate: &read_set::ScopedValidator<K>,
        action: ActionKey,
    ) -> Result<Option<Validated<K>>, Error>
    where
        K: Value + Send + Sync + 'static,
    {
        // Decoding the stored bytes fails only for damaged entries; a later
        // validation failure is an ordinary input change.
        let Some(inspected) = self
            .store
            .inspect_candidate::<K>(action, recipe.output_limit, self.store.graph_budget())
            .map_err(|error| match error {
                Error::Codec(reason) => Error::Corrupt {
                    path: self
                        .store
                        .root()
                        .join(crate::store::action_relative(K::KIND, action)),
                    reason,
                },
                error => error,
            })?
        else {
            return Ok(None);
        };
        let discovered_inputs = validate(
            inspected.value(),
            self.store.graph_budget(),
            read_set::Scope::Consumer,
        )?;
        let complete = recipe.complete(&discovered_inputs)?;
        if complete.action() != action {
            return Err(Error::StaleReadSet { action });
        }
        let content = inspected.content();
        let witness = read_set::Witness::discovered_scoped(
            recipe,
            inspected.value_arc(),
            Arc::clone(validate),
            &complete,
            content,
            inspected.bundled(),
        );
        inspected
            .validate(&complete, witness, self.store.graph_budget())
            .map(Some)
    }
}

struct Discovery<K> {
    hit: Option<Validated<K>>,
    rejected: bool,
    rejected_actions: [Option<(ActionKey, Corruption)>; MAX_DEEP_VALIDATIONS_PER_LOOKUP],
}

fn note_damaged(kind: Kind) {
    let kind = kind.directory();
    crate::progress::note(
        kind,
        &format!("rawr: ignoring a damaged cache entry ({kind}); rebuilding it"),
    );
}

// Candidate absence, stale inputs and malformed historical evidence may miss.
// A substantive account or operational failure must leave with its original
// error before another candidate or miss-side reservation can spend more work.
// The separate speculative index/deep lookup limits retain their handling.
fn terminal_replay_failure(error: &Error) -> bool {
    match error {
        Error::Limit { .. }
        | Error::Allocation
        | Error::State(_)
        | Error::Filesystem { .. }
        | Error::Cleanup { .. } => true,
        Error::Io { source, .. } | Error::ObservationIo { source, .. } => {
            source.kind() != std::io::ErrorKind::NotFound
        }
        Error::Importer(error) => match error.as_ref() {
            crate::Error::Node { source, .. } => terminal_replay_failure(source),
            crate::Error::Io { source, .. } => source.kind() != std::io::ErrorKind::NotFound,
            crate::Error::Owner { .. }
            | crate::Error::Allocation { .. }
            | crate::Error::Limit { .. }
            | crate::Error::Process { .. }
            | crate::Error::NonSendFuture { .. }
            | crate::Error::ErasedBound { .. }
            | crate::Error::InputAccountRefused => true,
            crate::Error::Request { .. }
            | crate::Error::Path { .. }
            | crate::Error::Toolchain { .. }
            | crate::Error::Schema { .. }
            | crate::Error::ResolutionDrift { .. }
            | crate::Error::Input { .. }
            | crate::Error::Extraction { .. } => false,
        },
        Error::CandidateCorrupt(_)
        | Error::Shape(_)
        | Error::OutsideRoot(_)
        | Error::Codec(_)
        | Error::Corrupt { .. }
        | Error::CleanupRequired(_)
        | Error::StaleReadSet { .. }
        | Error::SourceChanged(_)
        | Error::Transform(_) => false,
    }
}

/// An importer failure as a node reports it. A failure in the user's own
/// Rust code is said as it is.
fn importer_message(error: &crate::Error) -> String {
    match error {
        crate::Error::NonSendFuture { .. } | crate::Error::ErasedBound { .. } => error.to_string(),
        error => format!("node transform could not obtain importer authority: {error}"),
    }
}

/// Independent typed-node, replay, or store failure.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("candidate-owned corruption: {0}")]
    CandidateCorrupt(Box<Corruption>),
    #[error("{}", importer_message(.0))]
    Importer(#[source] Box<crate::Error>),
    #[error("node store I/O failed while attempting to {operation} at {path}")]
    Io {
        operation: &'static str,
        path: std::path::PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("node current-input observation failed while attempting to {operation}: {source}")]
    ObservationIo {
        operation: &'static str,
        #[source]
        source: std::io::Error,
    },
    #[error("node store path has an unsupported shape: {0}")]
    Shape(std::path::PathBuf),
    #[error("node store path escaped its canonical root: {0}")]
    OutsideRoot(std::path::PathBuf),
    #[error("{resource} exceeded limit {limit} with observed size {actual}")]
    Limit {
        resource: &'static str,
        limit: usize,
        actual: usize,
    },
    #[error("node codec rejected stored evidence: {0}")]
    Codec(&'static str),
    #[error("node store entry is corrupt at {path}: {reason}")]
    Corrupt {
        path: std::path::PathBuf,
        reason: &'static str,
    },
    #[error("node store allocation failed")]
    Allocation,
    #[error("node store mutation requires bounded cleanup: {0}")]
    CleanupRequired(&'static str),
    #[error("node evaluation failed and its reservation cleanup also failed")]
    Cleanup {
        #[source]
        failure: Box<Error>,
        cleanup: Box<Error>,
    },
    #[error("node store filesystem query failed while attempting to {operation} at {path}")]
    Filesystem {
        operation: &'static str,
        path: std::path::PathBuf,
        #[source]
        source: rustix::io::Errno,
    },
    #[error("node read-set entry became stale for action {action}")]
    StaleReadSet { action: ActionKey },
    /// A source file changed after its snapshot was read and before the
    /// generation it prepared was admitted: a newer save supersedes it.
    #[error("the source changed while it was prepared: {}", .0.display())]
    SourceChanged(std::path::PathBuf),
    #[error("node evaluation state is invalid: {0}")]
    State(&'static str),
    #[error("node transform child failed: {0}")]
    Transform(Box<str>),
}

impl From<Corruption> for Error {
    fn from(corruption: Corruption) -> Self {
        Self::CandidateCorrupt(Box::new(corruption))
    }
}

impl From<crate::Error> for Error {
    fn from(error: crate::Error) -> Self {
        Self::Importer(Box::new(error))
    }
}

pub(crate) fn content_digest<K: Value>(bytes: &[u8]) -> Digest {
    content_digest_parts(K::KIND, K::SCHEMA, bytes)
}

pub(crate) fn content_digest_parts(kind: Kind, schema: u32, bytes: &[u8]) -> Digest {
    let mut digest = Sha256::new();
    digest.update(b"rawr.node.content/v1");
    digest.update([kind as u8]);
    digest.update(schema.to_le_bytes());
    digest.update((bytes.len() as u64).to_le_bytes());
    digest.update(bytes);
    Digest::from_bytes(digest.finalize().into())
}

#[cfg(test)]
mod tests;
