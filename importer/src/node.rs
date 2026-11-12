//! Typed content-addressed importer transformations.

use std::marker::PhantomData;
use std::sync::Arc;

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

pub(crate) const STORAGE_SCHEMA: u16 = 4;
pub(crate) const STORAGE_DIRECTORY: &str = "0004";

/// An executed value's schema with the versions its execution key carries
/// folded in. A store entry whose schema differs from the current one was
/// written by a rawr whose classifier, generator, protocol or codec differs,
/// so no current lookup can reach it and the store evicts it (#577).
pub(crate) const fn reader_schema(codec: u32, versions: &[u32]) -> u32 {
    let mut hash = fold(0x811c_9dc5, &codec.to_le_bytes());
    let mut index = 0;
    while index < versions.len() {
        hash = fold(hash, &versions[index].to_le_bytes());
        index += 1;
    }
    hash
}

/// A version string as one word for [`reader_schema`].
pub(crate) const fn version_word(version: &str) -> u32 {
    fold(0x811c_9dc5, version.as_bytes())
}

/// FNV-1a over `bytes`, continuing from `hash`.
const fn fold(mut hash: u32, bytes: &[u8]) -> u32 {
    let mut index = 0;
    while index < bytes.len() {
        hash = (hash ^ bytes[index] as u32).wrapping_mul(0x0100_0193);
        index += 1;
    }
    hash
}

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
    /// Whether this value's key carries the import context: the project,
    /// its Cargo home, build environment, target and limits. A value its
    /// transform computes from its parents' content and its own inputs
    /// alone, as a classification is from its API, leaves the context out:
    /// a new `CONTEXT_SCHEMA` or a moved Cargo home that gives the same
    /// parents keeps it (#1766).
    const CONTEXTUAL: bool = true;

    /// Encode this value canonically. Its size is never a reason to
    /// refuse it: the store's sizes are eviction targets.
    fn encode(&self) -> Result<Box<[u8]>, Error>;
    /// Encode a value that is about to be dropped; one already holding its
    /// encoding can hand that buffer over instead of copying it.
    fn into_encoded(self) -> Result<Box<[u8]>, Error> {
        self.encode()
    }
    /// Decode one fully consumed canonical output. Every length it reads is
    /// bounded by the bytes it was given.
    fn decode(bytes: &[u8]) -> Result<Self, Error>;
    /// Decode owned store bytes; a value that can keep them avoids a copy.
    fn decode_owned(bytes: Box<[u8]>) -> Result<Self, Error> {
        Self::decode(&bytes)
    }
    /// Whether the store keeps this value's encoding as content-defined parts
    /// (`store::parts`), so a value that differs from an earlier one in a few
    /// regions costs only the parts it changed.
    const PARTS: bool = false;
    /// This value as the store keeps it, consuming it.
    fn into_payload(self) -> Result<crate::store::Payload, Error> {
        let bytes = self.into_encoded()?;
        Ok(if Self::PARTS {
            crate::store::Payload::Parts(bytes)
        } else {
            crate::store::Payload::Whole(bytes)
        })
    }
    /// This value as the store keeps it.
    fn payload(&self) -> Result<crate::store::Payload, Error> {
        let bytes = self.encode()?;
        Ok(if Self::PARTS {
            crate::store::Payload::Parts(bytes)
        } else {
            crate::store::Payload::Whole(bytes)
        })
    }
    /// Decode the stored manifest of a parted value whose parts are kept in
    /// `parts`. The default reads them back; a part that is missing or
    /// altered makes the entry one to rebuild, as Cargo rebuilds what was
    /// deleted from its target.
    fn decode_parts(manifest: Box<[u8]>, parts: &std::path::Path) -> Result<Self, Error> {
        let bytes = crate::store::parts::read(parts, &manifest).map_err(|error| match error {
            Error::Io { .. } | Error::Corrupt { .. } => {
                Error::CleanupRequired("a stored value's part is missing or changed")
            }
            error => error,
        })?;
        Self::decode_owned(bytes)
    }
}

/// A complete direct-node recipe captured before lookup or execution.
pub struct Direct<K> {
    action: ActionKey,
    recipe: Digest,
    context: Digest,
    parents: Box<[content::Provenance]>,
    _kind: PhantomData<fn() -> K>,
}

/// One bounded recipe whose complete action facts are learned by execution.
pub struct Discoverable<K> {
    lookup: LookupKey,
    execution: ExecutionKey,
    context: Digest,
    fixed_inputs: Box<[u8]>,
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
        parents: &[Parent],
    ) -> Result<Self, Error> {
        Self::capture_edges(context, semantic_inputs, parents)
    }

    fn capture_edges(
        context: [u8; 32],
        semantic_inputs: &[u8],
        parents: &[impl content::Edge],
    ) -> Result<Self, Error> {
        if K::SCHEMA == 0 {
            return Err(Error::Codec("node output schema zero is reserved"));
        }
        let mut retained_parents = Vec::new();
        retained_parents
            .try_reserve_exact(parents.len())
            .map_err(|_| Error::Allocation)?;
        retained_parents.extend(parents.iter().map(content::Provenance::from_edge));
        // The read set's lineage keeps the context; the keys carry it only
        // for a contextual value.
        let keyed = keyed_context::<K>(context);
        let context = Digest::from_bytes(context);
        let recipe = Digest::recipe(keyed, semantic_inputs, parents);
        let action = ActionKey::derive(K::KIND, K::SCHEMA, keyed, semantic_inputs, parents);
        Ok(Self {
            action,
            recipe,
            context,
            parents: retained_parents.into_boxed_slice(),
            _kind: PhantomData,
        })
    }

    /// The exact action identity used for single-flight and publication.
    #[must_use]
    pub const fn action(&self) -> ActionKey {
        self.action
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
        parents: &[Parent],
    ) -> Result<Self, Error> {
        if K::SCHEMA == 0 {
            return Err(Error::Codec("node output schema zero is reserved"));
        }
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
        let lookup = LookupKey::derive(
            K::KIND,
            K::SCHEMA,
            keyed_context::<K>(context),
            fixed_inputs,
            parents,
        );
        let context = Digest::from_bytes(context);
        Ok(Self {
            lookup,
            execution: ExecutionKey::derive(lookup),
            context,
            fixed_inputs: retained_inputs.into_boxed_slice(),
            parents: retained_parents.into_boxed_slice(),
            _kind: PhantomData,
        })
    }

    fn complete(&self, discovered_inputs: &[u8]) -> Result<Direct<K>, Error> {
        let retained = 16_usize
            .checked_add(self.fixed_inputs.len())
            .and_then(|length| length.checked_add(discovered_inputs.len()))
            .ok_or(Error::Allocation)?;
        let mut semantic_inputs = Vec::new();
        semantic_inputs
            .try_reserve_exact(retained)
            .map_err(|_| Error::Allocation)?;
        semantic_inputs.extend_from_slice(&(self.fixed_inputs.len() as u64).to_le_bytes());
        semantic_inputs.extend_from_slice(&self.fixed_inputs);
        semantic_inputs.extend_from_slice(&(discovered_inputs.len() as u64).to_le_bytes());
        semantic_inputs.extend_from_slice(discovered_inputs);
        Direct::capture(self.context.as_bytes(), &semantic_inputs, &self.parents)
    }

    /// Non-authoritative bounded candidate-bucket identity.
    #[must_use]
    pub const fn lookup(&self) -> LookupKey {
        self.lookup
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

/// The identity of one graph evaluation and its typed scratch accounts.
#[derive(Clone)]
pub(crate) struct GraphBudget {
    accounts: Arc<account::Registry>,
}

impl GraphBudget {
    pub(crate) fn shares_with(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.accounts, &other.accounts)
    }

    pub(crate) fn new() -> Self {
        Self {
            accounts: Arc::new(account::Registry::default()),
        }
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
                    let payload = execute().map_err(Error::fresh)?.into_payload()?;
                    Ok(crate::store::Reserved::Publish {
                        result: (),
                        recipe: recipe.clone(),
                        payload,
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
                    let payload = transform().map_err(Error::fresh)?.into_payload()?;
                    Ok(crate::store::Reserved::Publish {
                        result: (),
                        recipe: recipe.clone(),
                        payload,
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
        let first = self.lookup_discoverable_scoped(recipe, &validate, &mut stale)?;
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
        let mut repair = first.repair;
        let mut execute = Some(execute);
        self.store.with_ownership(|ownership| {
            self.store.with_heavy_lease(|| {
                let after_heavy = self.lookup_discoverable_scoped(recipe, &validate, &mut stale)?;
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
                repair |= after_heavy.repair;

                self.store.with_execution_lease(recipe.execution, || {
                    let rechecked =
                        self.lookup_discoverable_scoped(recipe, &validate, &mut stale)?;
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
                    repair |= rechecked.repair;

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
                            let value = transform().map_err(Error::fresh)?;
                            let discovered_inputs =
                                validate(&value, budget, read_set::Scope::Consumer)?;
                            let complete = recipe.complete(&discovered_inputs)?;
                            let payload = value.payload()?;
                            // An older entry at this action that no longer
                            // checks is replaced when this one publishes (#614).
                            match self.replay_discovered_scoped_exact(
                                recipe,
                                &validate,
                                complete.action(),
                            ) {
                                Ok(Some(_)) => {
                                    return Ok(crate::store::Reserved::Reuse((
                                        complete.action(),
                                        false,
                                    )));
                                }
                                Ok(None) => {}
                                Err(error) if rebuilds(&error) => {}
                                Err(error) => return Err(error),
                            }
                            Ok(crate::store::Reserved::Publish {
                                result: (complete.action(), true),
                                recipe: complete,
                                payload,
                                bundle: None,
                            })
                        },
                    )?;
                    self.store
                        .hint_candidate::<K>(recipe.lookup, action, repair);
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
        // Candidates found stale stay stale for this evaluation: the rechecks
        // under the leases look only for a concurrently published one.
        let mut stale = Vec::new();
        let first = self.lookup_discoverable_scoped(recipe, &validate, &mut stale)?;
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
        let mut repair = first.repair;
        let mut execute = Some(execute);
        self.store.with_ownership(|ownership| {
            self.store.with_heavy_lease(|| {
                let after_heavy = self.lookup_discoverable_scoped(recipe, &validate, &mut stale)?;
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
                repair |= after_heavy.repair;

                self.store.with_execution_lease(recipe.execution, || {
                    let rechecked =
                        self.lookup_discoverable_scoped(recipe, &validate, &mut stale)?;
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
                    repair |= rechecked.repair;

                    let evaluate = |attempt| {
                        self.metrics
                            .node(K::KIND.metric(), metrics::NodeEvent::ExecutedMiss);
                        let transform = execute.take().ok_or(Error::State(
                            "bundled node executor was consumed before the final miss",
                        ))?;
                        let (value, bundle, discovered_inputs) =
                            transform(attempt).map_err(Error::fresh)?.into_parts();
                        let complete = recipe.complete(&discovered_inputs)?;
                        let payload = value.payload()?;
                        if self
                            .replay_discovered_scoped_exact(recipe, &validate, complete.action())?
                            .is_some()
                        {
                            return Ok(crate::store::Reserved::Reuse((complete.action(), false)));
                        }
                        Ok(crate::store::Reserved::Publish {
                            result: (complete.action(), true),
                            recipe: complete,
                            payload,
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
                        .hint_candidate::<K>(recipe.lookup, action, repair);
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
    ) -> Result<Discovery<K>, Error>
    where
        K: Value + Send + Sync + 'static,
    {
        let validate = Arc::clone(validate);
        let scoped: read_set::ScopedValidator<K> =
            Arc::new(move |value, budget, _| validate(value, budget));
        self.lookup_discoverable_scoped(recipe, &scoped, &mut Vec::new())
    }

    fn lookup_discoverable_scoped<K>(
        &self,
        recipe: &Discoverable<K>,
        validate: &read_set::ScopedValidator<K>,
        stale: &mut Vec<Seen>,
    ) -> Result<Discovery<K>, Error>
    where
        K: Value + Send + Sync + 'static,
    {
        let candidates = self.store.candidates::<K>(recipe.lookup)?;
        let mut admitted = None;
        let mut rejected = candidates.rejected;
        let mut repair = candidates.rejected;
        let mut rejected_actions = [None; MAX_DEEP_VALIDATIONS_PER_LOOKUP];
        // A candidate its inputs' change superseded costs no deep
        // validation and stays in its bucket, for a return to those inputs
        // (#1794). The rest are validated up to the per-lookup bound.
        let mut deep = 0;
        for action in candidates.actions.iter().copied() {
            if let Some(seen) = stale.iter().find(|seen| seen.action == action) {
                rejected = true;
                repair |= !seen.superseded;
                continue;
            }
            if deep == MAX_DEEP_VALIDATIONS_PER_LOOKUP {
                return Ok(Discovery {
                    hit: None,
                    rejected: true,
                    repair: true,
                    rejected_actions,
                });
            }
            // An ordinary input change rebuilds silently under the progress
            // bar; only damaged entries earn a line.
            let check = Check::of(self.replay_discovered(recipe, validate, action))?;
            let slot = deep;
            if !matches!(check, Check::Superseded) {
                deep += 1;
            }
            let current = match check {
                Check::Hit(current) => current,
                Check::Superseded => {
                    stale.push(Seen::superseded(action));
                    rejected = true;
                    continue;
                }
                Check::Damaged(Some(proof)) => {
                    note_damaged(K::KIND);
                    rejected = true;
                    repair = true;
                    rejected_actions[slot] = Some((action, proof));
                    continue;
                }
                Check::Damaged(None) => {
                    note_damaged(K::KIND);
                    stale.push(Seen::stale(action));
                    rejected = true;
                    repair = true;
                    continue;
                }
                Check::Stale => {
                    stale.push(Seen::stale(action));
                    rejected = true;
                    repair = true;
                    continue;
                }
            };
            if admitted.is_some() {
                return Ok(Discovery {
                    hit: None,
                    rejected: true,
                    repair: true,
                    rejected_actions,
                });
            }
            admitted = Some(current);
        }
        Ok(Discovery {
            hit: admitted,
            rejected,
            repair,
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
            .inspect_candidate::<K>(action, self.store.graph_budget())
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
    /// Whether the next publication replaces the bucket: a candidate in it
    /// was stale or damaged, or the bucket did not read whole. Candidates
    /// their inputs' change superseded keep it.
    repair: bool,
    rejected_actions: [Option<(ActionKey, Corruption)>; MAX_DEEP_VALIDATIONS_PER_LOOKUP],
}

/// A candidate one evaluation found no hit, and whether only its inputs'
/// change superseded it.
struct Seen {
    action: ActionKey,
    superseded: bool,
}

impl Seen {
    const fn stale(action: ActionKey) -> Self {
        Self {
            action,
            superseded: false,
        }
    }

    const fn superseded(action: ActionKey) -> Self {
        Self {
            action,
            superseded: true,
        }
    }
}

fn note_damaged(kind: Kind) {
    let kind = kind.directory();
    crate::progress::note(
        kind,
        &format!("rawr: ignoring a damaged cache entry ({kind}); rebuilding it"),
    );
}

/// What the cache check found for one stored entry: its value, or an entry
/// to rebuild. A stale one, whose inputs changed or which another version
/// of rawr wrote, rebuilds silently, and a damaged one with a note. A
/// failure that stops the build is the `Err` beside it, never one of these,
/// so the same error from a fresh build is a failure, or a rawr bug (#522).
pub(crate) enum Check<T> {
    Hit(T),
    Stale,
    /// Its recorded inputs provably changed, shown without replaying it:
    /// it holds for those inputs, should they return.
    Superseded,
    /// With the proof of the candidate's own corruption, when there is one.
    Damaged(Option<Corruption>),
}

impl<T> Check<T> {
    /// The check of one entry from its replay.
    fn of(replay: Result<Option<T>, Error>) -> Result<Self, Error> {
        match replay {
            Ok(Some(hit)) => Ok(Self::Hit(hit)),
            Ok(None) => Ok(Self::Stale),
            Err(Error::Superseded) => Ok(Self::Superseded),
            Err(Error::CandidateCorrupt(proof)) => Ok(Self::Damaged(Some(*proof))),
            Err(Error::Corrupt { .. }) => Ok(Self::Damaged(None)),
            Err(error) if stale(&error) => Ok(Self::Stale),
            Err(error) => Err(error),
        }
    }
}

/// Whether a stored entry's replay failed only because it is stale: its
/// inputs changed, or another version of rawr wrote it. Damage and
/// failures that stop the build are not stale.
pub(crate) fn stale(error: &Error) -> bool {
    !matches!(error, Error::CandidateCorrupt(_) | Error::Corrupt { .. })
        && !terminal_replay_failure(error)
}

/// Whether a stored entry's replay failed because it is to be rebuilt:
/// stale or damaged, as [`Check::of`] reads it.
fn rebuilds(error: &Error) -> bool {
    !terminal_replay_failure(error)
}

impl Error {
    /// Whether this failure is an entry that went stale after it was read,
    /// so the run starts again and rebuilds it rather than failing (#609).
    #[must_use]
    pub fn is_stale(&self) -> bool {
        match self {
            Self::StaleReadSet { .. } => true,
            Self::Importer(error) => error.is_stale(),
            _ => false,
        }
    }
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
            crate::Error::Owner
            | crate::Error::Allocation { .. }
            | crate::Error::Limit { .. }
            | crate::Error::Process { .. }
            | crate::Error::NonSendFuture { .. }
            | crate::Error::ErasedBound { .. }
            | crate::Error::InputAccountRefused => true,
            // The installed toolchain or driver is not the one this rawr
            // supports: a rebuild meets the same mismatch, so it is
            // reported at once instead of after every round it reaches.
            // A Cargo document's schema may be an older entry's.
            crate::Error::Toolchain { field, .. } => {
                !matches!(*field, "graph schema" | "unit schema")
            }
            crate::Error::Request { .. }
            | crate::Error::Path { .. }
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
        | Error::Superseded
        | Error::Transform(_) => false,
    }
}

/// Independent typed-node, replay, or store failure.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("candidate-owned corruption: {0}")]
    CandidateCorrupt(Box<Corruption>),
    #[error("{0}")]
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
    /// A candidate's recorded inputs provably changed, shown without
    /// replaying it: it is no hit now, and holds should they return.
    #[error("a recorded input of the candidate changed")]
    Superseded,
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

impl Error {
    /// A fresh build's failure as it leaves its transform. The cache check
    /// reads the same errors as an entry to rebuild ([`Check`]), but a fresh
    /// build has no entry: one that only a rawr bug can cause panics here,
    /// naming its invariant (#522, D664).
    pub(crate) fn fresh(self) -> Self {
        if let Some(invariant) = self.invariant() {
            panic!("{invariant}");
        }
        self
    }

    /// The invariant this failure breaks, when only a rawr bug can cause it.
    pub(crate) fn invariant(&self) -> Option<String> {
        match self {
            Self::Importer(error) => error.invariant(),
            _ => None,
        }
    }
}

/// The context a `K`'s key carries: the import context's identity, or none
/// for a value that leaves it out ([`Value::CONTEXTUAL`]).
pub(crate) const fn keyed_context<K: Value>(context: [u8; 32]) -> Digest {
    Digest::from_bytes(if K::CONTEXTUAL { context } else { [0; 32] })
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
