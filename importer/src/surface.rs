//! Exact Rawr-snapshot-bound symbolic bridge demand.

use std::io::{self, Write};

use serde::ser::SerializeSeq as _;
use serde::{Deserialize, Serialize, Serializer};
use sha2::{Digest as _, Sha256};

use crate::contract::{Classified, Contract};

#[doc(hidden)]
pub mod account;
pub(crate) mod adapter;
pub mod application;
pub mod builder;
pub mod callable;
#[doc(hidden)]
pub mod facade;
pub mod headers;
pub mod identity;
pub mod instance;
pub mod member;
pub mod opaque;
mod runner;
pub mod text;

pub(crate) const SURFACE_SCHEMA: u32 = 3;

fn has_unresolved_structural_parameters(
    representability: &crate::classify::representability::Representability,
) -> bool {
    use crate::classify::representability::Representability;

    match representability {
        // A struct whose every parameter has a default is reached at that
        // instance, as palette's `Rgb` is `Rgb<Srgb, f32>`.
        Representability::Struct(plan) => plan.declaration_only(),
        Representability::Enum(plan) => !plan.parameters().is_empty(),
        _ => false,
    }
}

/// Uninhabited marker proving canonical demand sealing completed.
///
/// ```compile_fail
/// let _ = rawr_import::Sealed {};
/// ```
pub enum Sealed {}

mod phase {
    pub trait Sealed {}
}

/// A closed phase accepted by [`Demand`].
pub trait Phase: phase::Sealed {}

impl phase::Sealed for Sealed {}
impl Phase for Sealed {}

/// Runner behavior carried by the compiler-owned demand brand.
pub trait Runner {
    /// Return whether the generated process must compile and evaluate snapshots.
    fn compile_snapshot(&self) -> bool;

    /// Borrow the exact compiler facade plan embedded into generated source.
    #[doc(hidden)]
    fn facade_plan(&self) -> Option<&[u8]> {
        None
    }
}

impl Runner for () {
    fn compile_snapshot(&self) -> bool {
        false
    }
}

/// SHA-256 identity of exact canonical Rawr source and manifest bytes.
///
/// ```compile_fail
/// let _ = rawr_import::surface::Snapshot([0; 32]);
/// ```
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct Snapshot([u8; 32], SnapshotUsage);

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
struct SnapshotUsage {
    items: usize,
    bytes: usize,
}

impl Serialize for Snapshot {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        self.0.serialize(serializer)
    }
}

/// Prospective bounded hasher for one canonically ordered Rawr snapshot.
#[doc(hidden)]
pub struct SnapshotBuilder {
    digest: Sha256,
    entry: bool,
    files: usize,
    previous: Option<Box<str>>,
    items: usize,
    bytes: usize,
    item_limit: usize,
    byte_limit: usize,
    string_limit: usize,
    account: account::Account,
}

/// SHA-256 identity of sorted symbolic bridge entries.
///
/// ```compile_fail
/// let _ = rawr_import::surface::Fingerprint([0; 32]);
/// ```
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct Fingerprint([u8; 32]);

/// One positioned symbolic use selected by checked Rawr source.
///
/// ```compile_fail
/// let _ = rawr_import::surface::Entry {};
/// ```
#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub struct Entry {
    identity: identity::Identity,
    origin: Origin,
}

/// Exact Rawr use-site evidence retained in a sealed demand.
#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub struct Origin {
    source: Box<str>,
    byte_start: u32,
    byte_end: u32,
}

/// One proof-backed local application and the use site that required it.
#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub(crate) struct Applied {
    invocation: application::Invocation,
    origin: Origin,
}

/// The distinct applications a runner is built from, without use sites.
fn unique_applications(applied: &[Applied]) -> Vec<&application::Invocation> {
    let mut unique = applied
        .iter()
        .map(|applied| &applied.invocation)
        .collect::<Vec<_>>();
    unique.sort_unstable();
    unique.dedup();
    unique
}

/// A phase-indexed immutable symbolic bridge demand.
///
/// Sealed demands are inspection evidence only. They deliberately expose no
/// executable authority:
///
/// ```compile_fail
/// # fn cannot_execute<B>(demand: &rawr_import::Demand<rawr_import::Sealed, B>) {
/// let _authority = demand.authority();
/// # }
/// ```
///
/// ```compile_fail
/// # fn clone_demand<B>(demand: &rawr_import::Demand<rawr_import::Sealed, B>) {
/// let _: rawr_import::Demand<rawr_import::Sealed, B> = demand.clone();
/// # }
/// ```
///
/// ```compile_fail
/// # fn erase_brand<B>(demand: rawr_import::Demand<rawr_import::Sealed, B>)
/// #     -> rawr_import::Demand<rawr_import::Sealed, ()> {
/// demand
/// # }
/// ```
pub struct Demand<P: Phase, B> {
    _brand: B,
    contract_chain: Option<runner::Binding>,
    runner: Option<runner::Inputs>,
    contract: crate::contract::fingerprint::Fingerprint,
    snapshot: Snapshot,
    entries: Box<[Entry]>,
    plans: Box<[adapter::Plan]>,
    canonical: Box<[u8]>,
    fingerprint: Fingerprint,
    _allocation: crate::contract::Allocation,
    phase: std::marker::PhantomData<fn() -> P>,
}

/// Consumed demand parts available only to the generated-bridge transform.
pub(crate) struct Request<B> {
    pub(crate) _brand: B,
    pub(crate) runner: Option<runner::Inputs>,
    pub(crate) contract: crate::contract::fingerprint::Fingerprint,
    pub(crate) snapshot: Snapshot,
    pub(crate) entries: Box<[Entry]>,
    pub(crate) plans: Box<[adapter::Plan]>,
    pub(crate) canonical: Box<[u8]>,
    pub(crate) fingerprint: Fingerprint,
    pub(crate) allocation: crate::contract::Allocation,
}

#[cfg(feature = "test-fixtures")]
impl<B> Request<B> {
    pub(crate) fn fixture(brand: B, limits: &crate::cargo::Limits) -> Self {
        let contract = crate::contract::fingerprint::Fingerprint([0x43; 32]);
        Self {
            _brand: brand,
            runner: None,
            contract,
            snapshot: Snapshot::from_rawr_digest([0x53; 32]),
            entries: Box::default(),
            plans: Box::default(),
            canonical: Box::default(),
            fingerprint: Fingerprint([0x46; 32]),
            allocation: crate::contract::Allocation::fixture(contract, limits),
        }
    }
}

/// Contract-bound capability used by the Rawr Engine to seal one demand.
pub struct Compiler<'contract, B> {
    brand: B,
    contract: &'contract Contract<Classified>,
    snapshot: Snapshot,
    entries: Vec<Entry>,
    builders: Vec<builder::Plan>,
    applications: Vec<Applied>,
    allocation: crate::contract::Allocation,
    event_projection: Option<crate::profile::contract::EventProjectionRules>,
    item_limit: usize,
    byte_limit: usize,
    string_limit: usize,
    account: account::Account,
}

#[derive(Serialize)]
struct Canonical<'a> {
    schema: u32,
    contract: &'a [u8; 32],
    snapshot: Snapshot,
    entries: &'a [Entry],
    builders: &'a [builder::Plan],
    #[serde(skip_serializing_if = "<[_]>::is_empty")]
    applications: &'a [Applied],
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct StoredCanonical {
    schema: u32,
    contract: [u8; 32],
    snapshot: [u8; 32],
    entries: Box<[Entry]>,
    builders: Box<[builder::Plan]>,
    #[serde(default, skip_serializing_if = "<[_]>::is_empty")]
    applications: Box<[Applied]>,
}

#[derive(Serialize)]
struct SurfaceCanonical<'a> {
    schema: u32,
    contract: &'a [u8; 32],
    entries: UniqueIdentities<'a>,
    builders: &'a [builder::Plan],
    #[serde(skip_serializing_if = "<[_]>::is_empty")]
    applications: Vec<&'a application::Invocation>,
}

/// A demand's native items, each once, without the use sites that named
/// them: what a runner is built from.
#[derive(Clone, Copy)]
pub(crate) struct UniqueIdentities<'a>(pub(crate) &'a [Entry]);

/// Counts canonical bytes; `overflow` marks a checked-arithmetic failure only.
/// The byte target is compared afterwards and recorded, never refused.
struct Measured {
    bytes: usize,
    overflow: bool,
}

struct Hashed {
    digest: Sha256,
    bytes: usize,
    overflow: bool,
}

impl Snapshot {
    /// Promote an exact Rawr-owned source digest into importer snapshot evidence.
    ///
    /// The Rawr Engine is responsible for deriving these bytes from its opaque
    /// revalidatable snapshot. Admission separately checks the exact files
    /// named by bound snapshot transport for freshness.
    #[doc(hidden)]
    #[must_use]
    pub const fn from_rawr_digest(digest: [u8; 32]) -> Self {
        Self(digest, SnapshotUsage { items: 0, bytes: 0 })
    }

    /// Start exact, length-framed hashing of canonically ordered Rawr inputs.
    #[doc(hidden)]
    #[must_use]
    pub fn builder(limits: &crate::cargo::Limits) -> SnapshotBuilder {
        SnapshotBuilder {
            digest: Sha256::new(),
            entry: false,
            files: 0,
            previous: None,
            items: 0,
            bytes: 0,
            item_limit: limits.items,
            byte_limit: limits.json_bytes,
            string_limit: limits.string_bytes,
            account: account::Account::surface(),
        }
    }

    /// Borrow the exact digest bytes.
    #[must_use]
    pub const fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    /// Independently inspect retained snapshot entry and file ownership.
    #[cfg(feature = "rustc-driver")]
    #[doc(hidden)]
    #[must_use]
    pub const fn driver_retained_usage(&self) -> (usize, usize) {
        (self.1.items, self.1.bytes)
    }
}

impl SnapshotBuilder {
    /// The accounting axes this snapshot crossed so far.
    #[doc(hidden)]
    #[must_use]
    pub const fn exceedances(&self) -> crate::exceedance::Record {
        self.account.exceedances()
    }

    /// Bind the exact application entry target before retaining input files.
    ///
    /// The path, item and byte targets are accounting; crossing one is
    /// recorded on the surface account and the entry is still retained.
    /// Checked-arithmetic overflow stays terminal.
    #[doc(hidden)]
    pub fn entry(&mut self, path: &str) -> Result<(), Error> {
        if self.entry || self.items != 0 {
            return Err(Error::SnapshotOrder);
        }
        let path_length = u64::try_from(path.len()).map_err(|_| Error::SnapshotOrder)?;
        let items = self.items.checked_add(1).ok_or(Error::Limit {
            resource: "snapshot retained items",
            limit: self.item_limit,
            actual: usize::MAX,
        })?;
        self.account
            .exceeded("snapshot entry path bytes", self.string_limit, path.len());
        self.account
            .exceeded("snapshot retained bytes", self.byte_limit, path.len());
        self.account
            .exceeded("snapshot retained items", self.item_limit, items);
        let mut digest = self.digest.clone();
        digest.update(b"rawr.snapshot.entry/v1");
        digest.update(path_length.to_le_bytes());
        digest.update(path.as_bytes());
        self.digest = digest;
        self.entry = true;
        self.items = items;
        self.bytes = path.len();
        Ok(())
    }

    /// Retain one exact canonical path and its already-captured bytes.
    #[doc(hidden)]
    pub fn file(&mut self, path: &str, contents: &[u8]) -> Result<(), Error> {
        if !self.entry {
            return Err(Error::MissingEntry);
        }
        if self
            .previous
            .as_deref()
            .is_some_and(|previous| previous >= path)
        {
            return Err(Error::SnapshotOrder);
        }
        let items = self.items.checked_add(1).ok_or(Error::Limit {
            resource: "snapshot retained items",
            limit: self.item_limit,
            actual: usize::MAX,
        })?;
        let files = self.files.checked_add(1).ok_or(Error::Limit {
            resource: "snapshot source inputs",
            limit: self.item_limit,
            actual: usize::MAX,
        })?;
        let bytes = self
            .bytes
            .checked_add(path.len())
            .and_then(|bytes| bytes.checked_add(contents.len()))
            .ok_or(Error::Limit {
                resource: "snapshot retained bytes",
                limit: self.byte_limit,
                actual: usize::MAX,
            })?;
        // Accounting targets are recorded on the surface account; the
        // file is retained and the cumulative charge continues.
        self.account
            .exceeded("snapshot path bytes", self.string_limit, path.len());
        self.account
            .exceeded("snapshot retained items", self.item_limit, items);
        self.account
            .exceeded("snapshot retained bytes", self.byte_limit, bytes);
        let path_length = u64::try_from(path.len()).map_err(|_| Error::SnapshotOrder)?;
        let content_length = u64::try_from(contents.len()).map_err(|_| Error::SnapshotOrder)?;
        let mut retained = String::new();
        retained
            .try_reserve_exact(path.len())
            .map_err(|_| Error::Allocation)?;
        retained.push_str(path);
        let mut digest = self.digest.clone();
        digest.update(path_length.to_le_bytes());
        digest.update(path.as_bytes());
        digest.update(content_length.to_le_bytes());
        digest.update(contents);
        self.digest = digest;
        self.previous = Some(retained.into_boxed_str());
        self.items = items;
        self.files = files;
        self.bytes = bytes;
        Ok(())
    }

    /// Seal the exact snapshot digest after at least one source input.
    #[doc(hidden)]
    pub fn seal(self) -> Result<Snapshot, Error> {
        if !self.entry {
            return Err(Error::MissingEntry);
        }
        if self.files == 0 {
            return Err(Error::EmptySnapshot);
        }
        Ok(Snapshot(
            self.digest.finalize().into(),
            SnapshotUsage {
                items: self.items,
                bytes: self.bytes,
            },
        ))
    }
}

impl Origin {
    /// Promote one checked Rawr use-site range into bounded demand evidence.
    #[doc(hidden)]
    pub fn checked(
        source: &str,
        byte_start: u32,
        byte_end: u32,
        limits: &crate::cargo::Limits,
    ) -> Result<Self, Error> {
        // A free-standing origin has no compiler account in scope, so
        // its string target is recorded on a surface account of its own.
        let mut account = account::Account::surface();
        Self::validate(
            source,
            byte_start,
            byte_end,
            limits.string_bytes,
            &mut account,
        )?;
        Self::retain(source, byte_start, byte_end)
    }

    /// Prove the half-open range; the source-length target is an accounting
    /// comparison recorded on `account`, never a refusal.
    fn validate(
        source: &str,
        byte_start: u32,
        byte_end: u32,
        string_limit: usize,
        account: &mut account::Account,
    ) -> Result<(), Error> {
        if byte_start > byte_end {
            return Err(Error::InvalidOrigin);
        }
        account.exceeded("demand origin bytes", string_limit, source.len());
        Ok(())
    }

    fn retain(source: &str, byte_start: u32, byte_end: u32) -> Result<Self, Error> {
        let mut retained = String::new();
        retained
            .try_reserve_exact(source.len())
            .map_err(|_| Error::Allocation)?;
        retained.push_str(source);
        Ok(Self {
            source: retained.into_boxed_str(),
            byte_start,
            byte_end,
        })
    }

    /// Return the canonical Rawr source identity.
    #[must_use]
    pub fn source(&self) -> &str {
        &self.source
    }

    /// Return the half-open byte range in the checked Rawr source.
    #[must_use]
    pub const fn byte_range(&self) -> std::ops::Range<u32> {
        self.byte_start..self.byte_end
    }
}

fn entry_retained_bytes(identity: Option<usize>, source_bytes: usize) -> Option<usize> {
    let identity_dynamic = identity?.checked_sub(std::mem::size_of::<identity::Identity>())?;
    std::mem::size_of::<Entry>()
        .checked_add(identity_dynamic)?
        .checked_add(source_bytes)
}

impl Contract<Classified> {
    /// Bind symbolic demand collection to this contract and one exact snapshot.
    #[doc(hidden)]
    pub fn compiler<B>(
        &self,
        brand: B,
        snapshot: Snapshot,
        limits: &crate::cargo::Limits,
    ) -> Result<Compiler<'_, B>, Error> {
        let allocation = self.allocation(limits)?;
        self.compiler_with_allocation(brand, snapshot, limits, allocation)
    }

    /// Continue demand compilation from Rawr's exact facade allocation proof.
    #[doc(hidden)]
    pub fn compiler_with_allocation<B>(
        &self,
        brand: B,
        snapshot: Snapshot,
        limits: &crate::cargo::Limits,
        mut allocation: crate::contract::Allocation,
    ) -> Result<Compiler<'_, B>, Error> {
        if !allocation.matches(self.fingerprint()) {
            return Err(Error::ForeignAllocation);
        }
        allocation.retain(snapshot.1.items, snapshot.1.bytes)?;
        Ok(Compiler {
            brand,
            contract: self,
            snapshot,
            entries: Vec::new(),
            builders: Vec::new(),
            applications: Vec::new(),
            allocation,
            event_projection: None,
            item_limit: limits.items,
            byte_limit: limits.json_bytes,
            string_limit: limits.string_bytes,
            account: account::Account::surface(),
        })
    }
}

impl<B> Compiler<'_, B> {
    /// The accounting axes this compilation crossed so far.
    #[doc(hidden)]
    #[must_use]
    pub const fn exceedances(&self) -> crate::exceedance::Record {
        self.account.exceedances()
    }

    /// Bind the exact selected profile projection rule used only to prove a
    /// mapper-bearing invocation. This does not install a facade projection.
    #[doc(hidden)]
    pub fn with_event_projection_rules(
        mut self,
        rules: crate::profile::contract::EventProjectionRules,
    ) -> Result<Self, Error> {
        if self.event_projection.replace(rules).is_some() {
            return Err(Error::InstanceMismatch);
        }
        Ok(self)
    }

    /// Retain one checked symbolic consuming-builder replay. No native receiver
    /// or runner-local handle enters durable Rawr state.
    #[doc(hidden)]
    pub fn require_builder(&mut self, plan: builder::Plan) -> Result<(), Error> {
        for step in plan.steps() {
            let identity = step.entry().identity();
            let outcome = self
                .contract
                .outcome(identity)
                .ok_or(Error::ForeignIdentity)?;
            if !self.contract.accepts(identity, identity.selector())
                || !outcome.representability().is_demandable()
            {
                return Err(Error::NotDemandable);
            }
        }
        let actual = self.builders.len().checked_add(1).ok_or(Error::Limit {
            resource: "symbolic builder plans",
            limit: self.item_limit,
            actual: usize::MAX,
        })?;
        self.account
            .exceeded("symbolic builder plans", self.item_limit, actual);
        self.allocation
            .retain_checked(Some(1), plan.retained_bytes())?;
        self.builders
            .try_reserve(1)
            .map_err(|_| Error::Allocation)?;
        self.builders.push(plan);
        Ok(())
    }

    /// Retain one compiler-checked symbolic entry.
    pub fn require(&mut self, identity: identity::Identity, origin: Origin) -> Result<(), Error> {
        let outcome = self
            .contract
            .outcome(&identity)
            .ok_or(Error::ForeignIdentity)?;
        if has_unresolved_structural_parameters(outcome.representability()) {
            return Err(Error::UnconstrainedGeneric);
        }
        if !outcome.representability().is_demandable() {
            return Err(Error::NotDemandable);
        }
        let actual = self.entries.len().checked_add(1).ok_or(Error::Limit {
            resource: "symbolic demand entries",
            limit: self.item_limit,
            actual: usize::MAX,
        })?;
        self.account
            .exceeded("symbolic demand entries", self.item_limit, actual);
        let bytes = entry_retained_bytes(identity.checked_retained_bytes(), origin.source.len());
        self.allocation.retain_checked(Some(1), bytes)?;
        self.entries.try_reserve(1).map_err(|_| Error::Allocation)?;
        self.entries.push(Entry { identity, origin });
        Ok(())
    }

    /// Retain one proof-backed local application at its Rawr use site. It
    /// is not a classified selector: its plan comes from the application.
    #[doc(hidden)]
    pub fn require_application(
        &mut self,
        application: &application::Application,
        source: &str,
        byte_start: u32,
        byte_end: u32,
    ) -> Result<(), Error> {
        let actual = self.applications.len().checked_add(1).ok_or(Error::Limit {
            resource: "symbolic applications",
            limit: self.item_limit,
            actual: usize::MAX,
        })?;
        self.account
            .exceeded("symbolic applications", self.item_limit, actual);
        Origin::validate(
            source,
            byte_start,
            byte_end,
            self.string_limit,
            &mut self.account,
        )?;
        let invocation = application.invocation.clone();
        let bytes = serde_json::to_vec(&invocation)
            .map_err(|_| Error::Encoding)?
            .len()
            .checked_add(source.len());
        self.allocation.retain_checked(Some(1), bytes)?;
        self.applications
            .try_reserve(1)
            .map_err(|_| Error::Allocation)?;
        let origin = Origin::retain(source, byte_start, byte_end)?;
        self.applications.push(Applied { invocation, origin });
        Ok(())
    }

    /// Validate, charge, then own one selector and Rawr origin exactly once.
    #[doc(hidden)]
    pub fn require_selected(
        &mut self,
        base: &identity::Identity,
        selector: identity::Selector,
        source: &str,
        byte_start: u32,
        byte_end: u32,
    ) -> Result<(), Error> {
        if !self.contract.accepts(base, &selector) {
            return Err(Error::ForeignIdentity);
        }
        let outcome = self.contract.outcome(base).ok_or(Error::ForeignIdentity)?;
        if has_unresolved_structural_parameters(outcome.representability()) {
            return Err(Error::UnconstrainedGeneric);
        }
        let actual = self.entries.len().checked_add(1).ok_or(Error::Limit {
            resource: "symbolic demand entries",
            limit: self.item_limit,
            actual: usize::MAX,
        })?;
        self.account
            .exceeded("symbolic demand entries", self.item_limit, actual);
        Origin::validate(
            source,
            byte_start,
            byte_end,
            self.string_limit,
            &mut self.account,
        )?;
        let bytes = entry_retained_bytes(base.checked_retained_bytes_with(&selector), source.len());
        self.allocation.retain_checked(Some(1), bytes)?;
        self.entries.try_reserve(1).map_err(|_| Error::Allocation)?;
        let identity = base.selected(selector);
        let origin = Origin::retain(source, byte_start, byte_end)?;
        self.entries.push(Entry { identity, origin });
        Ok(())
    }

    /// Retain one compiler-checked concrete generic or callable instance.
    #[doc(hidden)]
    pub fn require_instance(
        &mut self,
        base: &identity::Identity,
        selector: identity::Selector,
        mut instance: instance::Arguments,
        source: &str,
        byte_start: u32,
        byte_end: u32,
    ) -> Result<(), Error> {
        if !self.contract.accepts(base, &selector) {
            return Err(Error::ForeignIdentity);
        }
        let outcome = self.contract.outcome(base).ok_or(Error::ForeignIdentity)?;
        // A generic struct's constructor at the types its use infers.
        let structure = matches!(
            (outcome.representability(), &selector),
            (
                crate::classify::representability::Representability::Struct(_),
                identity::Selector::StructConstructor(_) | identity::Selector::StructField(_)
            )
        );
        if (!structure || instance.types().is_empty())
            && has_unresolved_structural_parameters(outcome.representability())
        {
            return Err(Error::UnconstrainedGeneric);
        }
        instance.matches(outcome.representability())?;
        if !structure {
            validate_function_items(
                self.contract,
                outcome,
                &mut instance,
                self.event_projection.as_ref(),
            )?;
        }
        let predicates = predicate_proofs(self.contract, outcome, &instance)?;
        instance.seal_predicates(predicates);
        let actual = self.entries.len().checked_add(1).ok_or(Error::Limit {
            resource: "symbolic demand entries",
            limit: self.item_limit,
            actual: usize::MAX,
        })?;
        self.account
            .exceeded("symbolic demand entries", self.item_limit, actual);
        Origin::validate(
            source,
            byte_start,
            byte_end,
            self.string_limit,
            &mut self.account,
        )?;
        let retained = base
            .checked_retained_bytes_with_instance(&selector, &instance)
            .and_then(|bytes| bytes.checked_add(source.len()))
            .ok_or(Error::Limit {
                resource: "surface instance retained bytes",
                limit: self.byte_limit,
                actual: usize::MAX,
            })?;
        self.allocation.retain_checked(Some(1), Some(retained))?;
        self.entries.try_reserve(1).map_err(|_| Error::Allocation)?;
        let identity = base.instantiated(selector, instance);
        let origin = Origin::retain(source, byte_start, byte_end)?;
        self.entries.push(Entry { identity, origin });
        Ok(())
    }

    /// Canonically sort, deduplicate, encode, and seal this symbolic demand.
    pub fn seal(mut self) -> Result<Demand<Sealed, B>, Error> {
        self.entries.sort_unstable();
        self.entries.dedup();
        self.builders.sort_unstable();
        self.builders.dedup();
        self.applications.sort_unstable();
        self.applications.dedup();
        let contract = self.contract.fingerprint();
        let canonical_value = Canonical {
            schema: SURFACE_SCHEMA,
            contract: contract.as_bytes(),
            snapshot: self.snapshot,
            entries: &self.entries,
            builders: &self.builders,
            applications: &self.applications,
        };
        let mut measured = Measured {
            bytes: 0,
            overflow: false,
        };
        let result = serde_json::to_writer(&mut measured, &canonical_value);
        if measured.overflow {
            return Err(Error::Limit {
                resource: "canonical demand bytes",
                limit: self.byte_limit,
                actual: usize::MAX,
            });
        }
        result.map_err(|_| Error::Encoding)?;
        // The canonical size is an accounting target recorded on the
        // surface account; the physical bounds are the P2 allocation and the
        // reservation below.
        self.account
            .exceeded("canonical demand bytes", self.byte_limit, measured.bytes);
        let mut canonical = Vec::new();
        self.allocation.retain(1, measured.bytes)?;
        canonical
            .try_reserve_exact(measured.bytes)
            .map_err(|_| Error::Allocation)?;
        serde_json::to_writer(&mut canonical, &canonical_value).map_err(|_| Error::Encoding)?;
        if canonical.len() != measured.bytes {
            return Err(Error::Encoding);
        }
        let unique = unique_identity_count(&self.entries)
            .checked_add(self.builders.len())
            .and_then(|count| count.checked_add(unique_applications(&self.applications).len()))
            .ok_or(Error::Limit {
                resource: "adapter plan count",
                limit: self.item_limit,
                actual: usize::MAX,
            })?;
        let plan_bytes =
            unique_identities(&self.entries).try_fold(0_usize, |bytes, identity| {
                let outcome = self
                    .contract
                    .outcome(identity)
                    .ok_or(Error::ForeignIdentity)?;
                let retained = adapter::Plan::retained_bytes(self.contract, outcome, identity)?
                    .ok_or(Error::Limit {
                        resource: "adapter plan retained bytes",
                        limit: self.byte_limit,
                        actual: usize::MAX,
                    })?;
                bytes.checked_add(retained).ok_or(Error::Limit {
                    resource: "adapter plan retained bytes",
                    limit: self.byte_limit,
                    actual: usize::MAX,
                })
            })?;
        let plan_bytes = self
            .builders
            .iter()
            .try_fold(plan_bytes, |bytes, builder| {
                let retained = adapter::Plan::retained_builder_bytes(self.contract, builder)?
                    .ok_or(Error::Limit {
                        resource: "builder adapter retained bytes",
                        limit: self.byte_limit,
                        actual: usize::MAX,
                    })?;
                bytes.checked_add(retained).ok_or(Error::Limit {
                    resource: "adapter plan retained bytes",
                    limit: self.byte_limit,
                    actual: usize::MAX,
                })
            })?;
        self.allocation.retain(unique, plan_bytes)?;
        let mut plans = Vec::new();
        plans
            .try_reserve_exact(unique)
            .map_err(|_| Error::Allocation)?;
        for identity in unique_identities(&self.entries) {
            let outcome = self
                .contract
                .outcome(identity)
                .ok_or(Error::ForeignIdentity)?;
            plans.push(adapter::Plan::try_new(self.contract, outcome, identity)?);
        }
        for builder in &self.builders {
            plans.push(adapter::Plan::try_builder(self.contract, builder)?);
        }
        for invocation in unique_applications(&self.applications) {
            plans.push(adapter::Plan::application(invocation));
        }
        plans.sort_unstable_by_key(adapter::Plan::identity);
        if plans
            .windows(2)
            .any(|pair| pair[0].identity() == pair[1].identity())
        {
            return Err(Error::BuilderShape);
        }
        let mut surface = Hashed {
            digest: Sha256::new(),
            bytes: 0,
            overflow: false,
        };
        let result = serde_json::to_writer(
            &mut surface,
            &SurfaceCanonical {
                schema: SURFACE_SCHEMA,
                contract: contract.as_bytes(),
                entries: UniqueIdentities(&self.entries),
                builders: &self.builders,
                applications: unique_applications(&self.applications),
            },
        );
        if surface.overflow {
            return Err(Error::Limit {
                resource: "canonical surface bytes",
                limit: self.byte_limit,
                actual: usize::MAX,
            });
        }
        result.map_err(|_| Error::Encoding)?;
        self.account
            .exceeded("canonical surface bytes", self.byte_limit, surface.bytes);
        let fingerprint = Fingerprint(surface.digest.finalize().into());
        Ok(Demand {
            _brand: self.brand,
            contract_chain: self.contract.node_chain().map(runner::Binding::capture),
            runner: None,
            contract,
            snapshot: self.snapshot,
            entries: self.entries.into_boxed_slice(),
            plans: plans.into_boxed_slice(),
            canonical: canonical.into_boxed_slice(),
            fingerprint,
            _allocation: self.allocation,
            phase: std::marker::PhantomData,
        })
    }
}

fn demanded_signature(
    representability: &crate::classify::representability::Representability,
) -> Option<&crate::classify::representability::signature::Signature> {
    use crate::classify::representability::Representability;
    match representability {
        Representability::Function(plan) => Some(plan.signature()),
        Representability::FunctionItem(plan) => Some(plan.signature()),
        Representability::Invocation(plan) => Some(plan.signature()),
        Representability::Method(plan) => Some(plan.signature()),
        _ => None,
    }
}

fn validate_function_items(
    contract: &Contract<Classified>,
    outcome: crate::classify::Outcome<'_>,
    instance: &mut instance::Arguments,
    event_rules: Option<&crate::profile::contract::EventProjectionRules>,
) -> Result<(), Error> {
    use crate::classify::representability::{convert, Representability};
    let signature =
        demanded_signature(outcome.representability()).ok_or(Error::InstanceMismatch)?;
    let expected = signature
        .inputs()
        .iter()
        .enumerate()
        .filter_map(|(position, ty)| {
            matches!(ty, convert::Type::FunctionItem { .. }).then_some((position, ty))
        })
        .collect::<Vec<_>>();
    // A crate function may fill the callable parameter a script closure
    // would, as `.style(container::rounded_box)` does: the adapter passes
    // the Rust function itself, and rustc checks it against the bound.
    let call = match outcome.representability() {
        Representability::FunctionItem(plan) => plan.call(),
        Representability::Invocation(plan) => plan.call(),
        _ => None,
    };
    let callable_at = |position: u32| {
        call.zip(usize::try_from(position).ok())
            .and_then(|(call, position)| Some((call, signature.inputs().get(position)?)))
            .is_some_and(|(call, ty)| *ty == convert::Type::Parameter(call.parameter()))
    };
    for selected in instance.function_items() {
        if !callable_at(selected.position()) {
            continue;
        }
        let call = call.ok_or(Error::InstanceMismatch)?;
        let selected_outcome = contract
            .outcome(selected.identity())
            .ok_or(Error::ForeignIdentity)?;
        // A generic function is selected at its instance's types, one per
        // type parameter, as `window::close::<Message>` is. Its bounds, as
        // `Task::done`'s `T: MaybeSend + 'static`, hold or fail where rustc
        // checks the adapter's call.
        let selected_signature = match selected_outcome.representability() {
            Representability::Function(plan) => plan.signature(),
            Representability::FunctionItem(plan)
                if plan.receiver().is_none()
                    && plan.call().is_none()
                    && plan.crossings().is_empty()
                    && plan.signature().parameters().iter().all(|parameter| {
                        parameter.kind() == crate::classify::representability::signature::Kind::Type
                    }) =>
            {
                plan.signature()
            }
            // An associated function of a generic impl, as `Task::done` of
            // `impl<T> Task<T>` is, at the impl's types and its own.
            Representability::Invocation(plan)
                if plan.receiver().is_none()
                    && plan.call().is_none()
                    && plan.crossings().is_empty()
                    && plan.signature().parameters().iter().all(|parameter| {
                        parameter.kind() == crate::classify::representability::signature::Kind::Type
                    }) =>
            {
                plan.signature()
            }
            _ => return Err(Error::InstanceMismatch),
        };
        if selected_signature.parameters().len() != selected.identity().types().len()
            || selected_signature.inputs().len() != call.inputs().len()
        {
            return Err(Error::InstanceMismatch);
        }
    }
    let statics = instance
        .function_items()
        .iter()
        .filter(|selected| !callable_at(selected.position()));
    if expected.len()
        != statics
            .clone()
            .count()
            .checked_add(instance.event_mappers().len())
            .ok_or(Error::InstanceMismatch)?
    {
        return Err(Error::InstanceMismatch);
    }
    let mut functions = statics.peekable();
    let mut mappers = instance.event_mappers().iter().peekable();
    let mut projections = Vec::new();
    projections
        .try_reserve_exact(instance.event_mappers().len())
        .map_err(|_| Error::Allocation)?;
    for (position, expected) in expected {
        let function_here = functions
            .peek()
            .is_some_and(|selected| usize::try_from(selected.position()).ok() == Some(position));
        let mapper_here = mappers
            .peek()
            .is_some_and(|selected| usize::try_from(selected.position()).ok() == Some(position));
        if function_here == mapper_here {
            return Err(Error::InstanceMismatch);
        }
        let convert::Type::FunctionItem { input, output } = expected else {
            unreachable!("filter retains only static function-item formals")
        };
        if mapper_here {
            let mapper = mappers.next().ok_or(Error::InstanceMismatch)?;
            let Representability::Invocation(plan) = outcome.representability() else {
                return Err(Error::InstanceMismatch);
            };
            let convert::Type::Parameter(parameter) = output.as_ref() else {
                return Err(Error::InstanceMismatch);
            };
            let type_position = plan
                .signature()
                .parameters()
                .iter()
                .filter(|formal| {
                    formal.kind() == crate::classify::representability::signature::Kind::Type
                })
                .position(|formal| formal.index() == *parameter)
                .ok_or(Error::InstanceMismatch)?;
            let Some(convert::Type::ProfileMessage {
                mapper: seal,
                payload,
            }) = instance.types().get(type_position)
            else {
                return Err(Error::InstanceMismatch);
            };
            if mapper.seal() != seal || payload.as_ref() != input.as_ref() {
                return Err(Error::InstanceMismatch);
            }
            let rules = event_rules.ok_or(Error::InstanceMismatch)?;
            projections.push(event_projection(
                contract,
                outcome,
                plan,
                mapper,
                rules,
                *parameter,
                concrete_message(instance, type_position)?,
            )?);
            continue;
        }
        let selected = functions.next().ok_or(Error::InstanceMismatch)?;
        let selected_outcome = contract
            .outcome(selected.identity())
            .ok_or(Error::ForeignIdentity)?;
        let selected_signature = match selected_outcome.representability() {
            Representability::Function(plan) => plan.signature(),
            Representability::FunctionItem(plan)
                if plan.signature().parameters().is_empty()
                    && plan.receiver().is_none()
                    && plan.predicates().is_empty() =>
            {
                plan.signature()
            }
            _ => return Err(Error::InstanceMismatch),
        };
        if !selected_signature.parameters().is_empty()
            || selected_signature.inputs() != std::slice::from_ref(input.as_ref())
            || selected_signature.output() != output.as_ref()
        {
            return Err(Error::InstanceMismatch);
        }
    }
    if functions.next().is_some() || mappers.next().is_some() {
        return Err(Error::InstanceMismatch);
    }
    for (mapper, projection) in instance.event_mappers_mut().iter_mut().zip(projections) {
        mapper.seal_projection(projection)?;
    }
    Ok(())
}

fn concrete_message(
    instance: &instance::Arguments,
    position: usize,
) -> Result<&crate::classify::representability::convert::Type, Error> {
    instance
        .types()
        .get(position)
        .ok_or(Error::InstanceMismatch)
}

#[allow(
    clippy::too_many_arguments,
    reason = "the private projection witness joins every exact compiler/profile proof"
)]
fn event_projection(
    contract: &Contract<Classified>,
    outcome: crate::classify::Outcome<'_>,
    plan: &crate::classify::representability::callable::Invocation,
    mapper: &instance::EventMapper,
    rules: &crate::profile::contract::EventProjectionRules,
    message_parameter: u32,
    message: &crate::classify::representability::convert::Type,
) -> Result<instance::EventProjection, Error> {
    use crate::api::model;
    use crate::classify::representability::convert::Type;

    let Type::ProfileMessage { mapper: seal, .. } = message else {
        return Err(Error::InstanceMismatch);
    };
    if seal != mapper.seal() {
        return Err(Error::InstanceMismatch);
    }
    let Type::Nominal {
        definition: source,
        arguments,
    } = plan.signature().output()
    else {
        return Err(Error::InstanceMismatch);
    };
    if outcome.identity().definition() != Some(*source) {
        return Err(Error::InstanceMismatch);
    }
    let mut message_arguments = arguments
        .iter()
        .enumerate()
        .filter_map(|(position, argument)| {
            matches!(argument, Type::Parameter(parameter) if *parameter == message_parameter)
                .then_some(position)
        });
    let source_message_position = message_arguments.next().ok_or(Error::InstanceMismatch)?;
    if message_arguments.next().is_some() {
        return Err(Error::InstanceMismatch);
    }

    let graph = contract.raw_graph();
    let source_definition = exact_model_definition(graph, source)?;
    let target_identity = identity::Definition::authenticated(*rules.target());
    exact_model_definition(graph, &target_identity)?;
    let widget_trait_identity = identity::Definition::authenticated(*rules.widget_trait());
    let widget_trait = exact_model_definition(graph, &widget_trait_identity)?;
    let source_message = type_parameter(source_definition, source_message_position)?;
    let widget_message = first_type_parameter(widget_trait)?;

    // The Widget implementation is the whole projection proof; the
    // generated runner boxes it through the trait's own `_boxed`.

    let mut widget = None;
    for implementation in graph.implementations() {
        if implementation.polarity() != model::Polarity::Positive {
            continue;
        }
        let model::Trait::Trait {
            id: trait_id,
            arguments: trait_arguments,
        } = implementation.trait_ref()
        else {
            continue;
        };
        let trait_identity = identity::Definition::of(trait_id);
        if trait_identity == widget_trait_identity
            && definition_type(implementation.self_type(), source_definition)
            && trait_arguments
                .first()
                .is_some_and(|argument| matches!(argument, model::Argument::Type(ty) if ty == implementation.self_type()))
        {
            let self_message = definition_argument(
                implementation.self_type(),
                source_definition,
                source_message,
            );
            let trait_message = trait_argument(trait_arguments, widget_trait, widget_message);
            if self_message.is_some() && self_message == trait_message {
                let proof = identity::Implementation::of(implementation)
                    .map_err(|_| Error::InstanceMismatch)?;
                if widget.replace(proof).is_some() {
                    return Err(Error::InstanceMismatch);
                }
            }
        }
    }
    let widget = widget.ok_or(Error::InstanceMismatch)?;
    let message = identity::converted_type_fingerprint(message);
    let invocation = outcome.identity().selector().fingerprint();
    let (byte_start, byte_end) = mapper.byte_range();
    let mut digest = Sha256::new();
    digest.update(b"rawr.import.surface.event-projection/v2");
    digest.update(rules.profile());
    digest.update(rules.version().to_le_bytes());
    digest.update(rules.rules());
    digest.update(rules.source());
    digest.update(rules.projection());
    digest.update(source.as_bytes());
    digest.update(target_identity.as_bytes());
    digest.update(widget.as_bytes());
    digest.update(message);
    digest.update(invocation);
    digest.update(mapper.position().to_le_bytes());
    digest.update(mapper.seal());
    digest.update(byte_start.to_le_bytes());
    digest.update(byte_end.to_le_bytes());
    let selector = digest.finalize().into();
    Ok(instance::EventProjection::sealed(
        *rules.profile(),
        rules.version(),
        *rules.rules(),
        *rules.source(),
        *rules.projection(),
        *source,
        target_identity,
        widget,
        message,
        invocation,
        selector,
    ))
}

fn exact_model_definition<'graph>(
    graph: &'graph crate::api::Graph<crate::api::Extracted>,
    identity: &identity::Definition,
) -> Result<&'graph crate::api::model::Definition, Error> {
    let mut candidates = graph
        .definitions()
        .filter(|definition| identity::Definition::of(definition.id()) == *identity);
    let definition = candidates.next().ok_or(Error::InstanceMismatch)?;
    if candidates.next().is_some() {
        return Err(Error::InstanceMismatch);
    }
    Ok(definition)
}

fn definition_type(
    ty: &crate::api::model::Type,
    definition: &crate::api::model::Definition,
) -> bool {
    matches!(ty, crate::api::model::Type::Definition { id, .. } if id == definition.id())
}

fn definition_generics(
    definition: &crate::api::model::Definition,
) -> Option<&crate::api::model::Generics> {
    use crate::api::model::Kind;
    match definition.kind() {
        Kind::Struct { generics, .. }
        | Kind::Union { generics, .. }
        | Kind::Enum { generics, .. }
        | Kind::TypeAlias { generics, .. }
        | Kind::Function { generics, .. }
        | Kind::Trait { generics, .. }
        | Kind::AssociatedType { generics, .. } => Some(generics),
        Kind::Rejected { .. }
        | Kind::Module
        | Kind::Const { .. }
        | Kind::TypeConst { .. }
        | Kind::Static { .. }
        | Kind::Sysroot { .. }
        | Kind::MacroRules { .. } => None,
    }
}

fn type_parameter(
    definition: &crate::api::model::Definition,
    position: usize,
) -> Result<&crate::api::model::Parameter, Error> {
    definition_generics(definition)
        .into_iter()
        .flat_map(crate::api::model::Generics::parameters)
        .filter(|parameter| {
            matches!(
                parameter.kind(),
                crate::api::model::ParameterKind::Type { .. }
            )
        })
        .nth(position)
        .ok_or(Error::InstanceMismatch)
}

fn first_type_parameter(
    definition: &crate::api::model::Definition,
) -> Result<&crate::api::model::Parameter, Error> {
    type_parameter(definition, 0)
}

fn definition_argument<'value>(
    ty: &'value crate::api::model::Type,
    definition: &crate::api::model::Definition,
    parameter: &crate::api::model::Parameter,
) -> Option<&'value crate::api::model::Type> {
    let crate::api::model::Type::Definition { id, arguments } = ty else {
        return None;
    };
    if id != definition.id() {
        return None;
    }
    definition_generics(definition)?
        .parameters()
        .zip(arguments)
        .find_map(|(candidate, argument)| {
            (candidate.owner() == parameter.owner() && candidate.index() == parameter.index())
                .then_some(argument)
        })
        .and_then(|argument| match argument {
            crate::api::model::Argument::Type(ty) => Some(ty),
            crate::api::model::Argument::Lifetime(_) | crate::api::model::Argument::Const(_) => {
                None
            }
        })
}

fn trait_argument<'value>(
    arguments: &'value [crate::api::model::Argument],
    definition: &crate::api::model::Definition,
    parameter: &crate::api::model::Parameter,
) -> Option<&'value crate::api::model::Type> {
    definition_generics(definition)?
        .parameters()
        .zip(arguments.iter().skip(1))
        .find_map(|(candidate, argument)| {
            (candidate.owner() == parameter.owner() && candidate.index() == parameter.index())
                .then_some(argument)
        })
        .and_then(|argument| match argument {
            crate::api::model::Argument::Type(ty) => Some(ty),
            crate::api::model::Argument::Lifetime(_) | crate::api::model::Argument::Const(_) => {
                None
            }
        })
}

fn predicate_proofs(
    contract: &Contract<Classified>,
    outcome: crate::classify::Outcome<'_>,
    instance: &instance::Arguments,
) -> Result<Box<[identity::Implementation]>, Error> {
    use crate::classify::representability::Representability;
    let predicates = match outcome.representability() {
        Representability::FunctionItem(plan) => plan.predicates(),
        Representability::Invocation(plan) => plan.predicates(),
        _ => &[],
    };
    if predicates.is_empty() {
        return Ok(Box::default());
    }
    let signature =
        demanded_signature(outcome.representability()).ok_or(Error::InstanceMismatch)?;
    let mut proofs = Vec::new();
    proofs
        .try_reserve_exact(predicates.len())
        .map_err(|_| Error::Allocation)?;
    for predicate in predicates {
        let position = signature
            .parameters()
            .iter()
            .filter(|parameter| {
                parameter.kind() == crate::classify::representability::signature::Kind::Type
            })
            .position(|parameter| parameter.index() == predicate.parameter())
            .ok_or(Error::InstanceMismatch)?;
        let concrete = instance
            .types()
            .get(position)
            .ok_or(Error::InstanceMismatch)?;
        let owner = outcome
            .identity()
            .definition()
            .ok_or(Error::InstanceMismatch)?;
        // A bound the importer cannot prove for this type is left to
        // rustc, which checks it where the bridge calls the item.
        let Some(scalar) = scalar_predicate_type(concrete) else {
            continue;
        };
        let mut completed = None;
        for proof in contract.raw_graph().scalar_predicate_proofs() {
            if identity::Definition::of(proof.owner()) != owner
                || proof.parameter() != predicate.parameter()
                || identity::Definition::of(proof.trait_id()) != predicate.trait_definition()
                || proof.scalar() != scalar
            {
                continue;
            }
            if completed.replace(proof).is_some() {
                return Err(Error::InstanceMismatch);
            }
        }
        let Some(completed) = completed else {
            continue;
        };
        let crate::api::model::ScalarPredicateOutcome::Positive {
            implementation: proven,
        } = completed.outcome()
        else {
            continue;
        };
        let mut selected = None;
        for implementation in contract.raw_graph().implementations() {
            if implementation.id() != proven {
                continue;
            }
            if implementation.polarity() != crate::api::model::Polarity::Positive
                || implementation.generics().parameters().len() != 0
                || implementation.generics().predicates().len() != 0
                || !implementation
                    .relevance()
                    .any(|definition| identity::Definition::of(definition) == owner)
            {
                continue;
            }
            let crate::api::model::Trait::Trait { id, .. } = implementation.trait_ref() else {
                continue;
            };
            if identity::Definition::of(id) != predicate.trait_definition()
                || !model_type_matches(implementation.self_type(), concrete)
            {
                continue;
            }
            let proof = identity::Implementation::of(implementation)
                .map_err(|_| Error::InstanceMismatch)?;
            if selected.replace(proof).is_some() {
                return Err(Error::InstanceMismatch);
            }
        }
        if let Some(selected) = selected {
            proofs.push(selected);
        }
    }
    Ok(proofs.into_boxed_slice())
}

fn scalar_predicate_type(
    ty: &crate::classify::representability::convert::Type,
) -> Option<crate::api::model::Scalar> {
    use crate::api::model::Scalar;
    use crate::classify::representability::convert::{Builtin, Type};
    match ty {
        Type::Unit => Some(Scalar::Unit),
        Type::Builtin(Builtin::Bool) => Some(Scalar::Bool),
        Type::Builtin(Builtin::Char) => Some(Scalar::Char),
        Type::Builtin(Builtin::F64) => Some(Scalar::F64),
        Type::Builtin(Builtin::I64) => Some(Scalar::I64),
        _ => None,
    }
}

fn model_type_matches(
    model: &crate::api::model::Type,
    converted: &crate::classify::representability::convert::Type,
) -> bool {
    use crate::api::model::{Argument, Type as Model};
    use crate::classify::representability::convert::{Builtin, Type};
    match (model, converted) {
        (Model::Tuple(members), Type::Unit) => members.is_empty(),
        (Model::Primitive(name), Type::Builtin(expected)) => {
            Builtin::primitive(name) == Some(*expected)
        }
        (
            Model::Definition { id, arguments },
            Type::Nominal {
                definition,
                arguments: converted,
            },
        ) => {
            identity::Definition::of(id) == *definition
                && arguments.len() == converted.len()
                && arguments.iter().zip(converted).all(|(argument, converted)| {
                    matches!(argument, Argument::Type(model) if model_type_matches(model, converted))
                })
        }
        _ => false,
    }
}

impl<B> Demand<Sealed, B> {
    /// Reconstruct one current sealed demand from its exact canonical checked-node output.
    #[doc(hidden)]
    pub fn replay_cached(
        bytes: &[u8],
        retained: (usize, usize),
        brand: B,
        contract: &Contract<Classified>,
        snapshot: Snapshot,
        limits: &crate::cargo::Limits,
    ) -> Result<Self, Error> {
        // The cached document size is an accounting target recorded on
        // a surface account for this replay; the physical bound is the node
        // content ceiling that already admitted the bytes.
        let mut account = account::Account::surface();
        Self::replay_cached_accounted(
            bytes,
            retained,
            brand,
            contract,
            snapshot,
            limits,
            &mut account,
        )
    }

    pub(crate) fn replay_cached_accounted(
        bytes: &[u8],
        retained: (usize, usize),
        brand: B,
        contract: &Contract<Classified>,
        snapshot: Snapshot,
        limits: &crate::cargo::Limits,
        account: &mut account::Account,
    ) -> Result<Self, Error> {
        Self::replay_with(
            bytes,
            retained,
            brand,
            (contract.fingerprint(), contract.node_chain()),
            contract.allocation(limits)?,
            snapshot,
            limits,
            account,
            |stored, plans| {
                for identity in unique_identities(&stored.entries) {
                    let outcome = contract.outcome(identity).ok_or(Error::ForeignIdentity)?;
                    plans.push(adapter::Plan::try_new(contract, outcome, identity)?);
                }
                for builder in &stored.builders {
                    plans.push(adapter::Plan::try_builder(contract, builder)?);
                }
                for invocation in unique_applications(&stored.applications) {
                    plans.push(adapter::Plan::application(invocation));
                }
                Ok(())
            },
        )
    }

    /// Reconstruct the same sealed demand from plans already resolved under
    /// this exact contract identity and generic node (no contract replay).
    #[allow(
        clippy::too_many_arguments,
        reason = "the resolved contract parts replace one borrowed contract"
    )]
    #[cfg(feature = "rustc-driver")]
    pub(crate) fn replay_resolved(
        bytes: &[u8],
        retained: (usize, usize),
        brand: B,
        contract: (
            crate::contract::fingerprint::Fingerprint,
            &crate::node::Chain,
        ),
        allocation: crate::contract::Allocation,
        snapshot: Snapshot,
        limits: &crate::cargo::Limits,
        resolved: Vec<adapter::Plan>,
    ) -> Result<Self, Error> {
        let mut account = account::Account::surface();
        Self::replay_with(
            bytes,
            retained,
            brand,
            (contract.0, Some(contract.1)),
            allocation,
            snapshot,
            limits,
            &mut account,
            |_, plans| {
                plans.extend(resolved);
                Ok(())
            },
        )
    }

    #[allow(
        clippy::too_many_arguments,
        reason = "one replay shared by the contract and resolved-plan sources"
    )]
    fn replay_with(
        bytes: &[u8],
        retained: (usize, usize),
        brand: B,
        contract: (
            crate::contract::fingerprint::Fingerprint,
            Option<&crate::node::Chain>,
        ),
        mut allocation: crate::contract::Allocation,
        snapshot: Snapshot,
        limits: &crate::cargo::Limits,
        account: &mut account::Account,
        resolve: impl FnOnce(&StoredCanonical, &mut Vec<adapter::Plan>) -> Result<(), Error>,
    ) -> Result<Self, Error> {
        let (fingerprint, chain) = contract;
        account.exceeded(
            "cached symbolic demand bytes",
            limits.json_bytes,
            bytes.len(),
        );
        let stored: StoredCanonical =
            serde_json::from_slice(bytes).map_err(|_| Error::Canonical)?;
        let exact = serde_json::to_vec(&stored).map_err(|_| Error::Encoding)?;
        if exact.as_slice() != bytes
            || stored.schema != SURFACE_SCHEMA
            || stored.contract != *fingerprint.as_bytes()
            || stored.snapshot != *snapshot.as_bytes()
            || stored.entries.windows(2).any(|pair| pair[0] >= pair[1])
            || stored.builders.windows(2).any(|pair| pair[0] >= pair[1])
            || stored
                .applications
                .windows(2)
                .any(|pair| pair[0] >= pair[1])
        {
            return Err(Error::Canonical);
        }
        let prior = allocation.retained_usage();
        let additional_items = retained.0.checked_sub(prior.0).ok_or(Error::Canonical)?;
        let additional_bytes = retained.1.checked_sub(prior.1).ok_or(Error::Canonical)?;
        allocation.retain(additional_items, additional_bytes)?;

        for entry in &stored.entries {
            Origin::validate(
                entry.origin.source(),
                entry.origin.byte_start,
                entry.origin.byte_end,
                limits.string_bytes,
                account,
            )?;
        }
        for applied in stored.applications.iter() {
            Origin::validate(
                applied.origin.source(),
                applied.origin.byte_start,
                applied.origin.byte_end,
                limits.string_bytes,
                account,
            )?;
        }
        let unique = unique_identity_count(&stored.entries)
            .checked_add(stored.builders.len())
            .and_then(|count| count.checked_add(unique_applications(&stored.applications).len()))
            .ok_or(Error::Canonical)?;
        account.exceeded("cached adapter plan count", limits.items, unique);
        let mut plans = Vec::new();
        plans
            .try_reserve_exact(unique)
            .map_err(|_| Error::Allocation)?;
        resolve(&stored, &mut plans)?;
        plans.sort_unstable_by_key(adapter::Plan::identity);
        if plans.len() != unique
            || plans
                .windows(2)
                .any(|pair| pair[0].identity() == pair[1].identity())
        {
            return Err(Error::BuilderShape);
        }
        let mut surface = Hashed {
            digest: Sha256::new(),
            bytes: 0,
            overflow: false,
        };
        let result = serde_json::to_writer(
            &mut surface,
            &SurfaceCanonical {
                schema: SURFACE_SCHEMA,
                contract: fingerprint.as_bytes(),
                entries: UniqueIdentities(&stored.entries),
                builders: &stored.builders,
                applications: unique_applications(&stored.applications),
            },
        );
        if surface.overflow {
            return Err(Error::Limit {
                resource: "cached canonical surface bytes",
                limit: limits.json_bytes,
                actual: usize::MAX,
            });
        }
        result.map_err(|_| Error::Encoding)?;
        account.exceeded(
            "cached canonical surface bytes",
            limits.json_bytes,
            surface.bytes,
        );
        Ok(Self {
            _brand: brand,
            contract_chain: chain.map(runner::Binding::capture),
            runner: None,
            contract: fingerprint,
            snapshot,
            entries: stored.entries,
            plans: plans.into_boxed_slice(),
            canonical: exact.into_boxed_slice(),
            fingerprint: Fingerprint(surface.digest.finalize().into()),
            _allocation: allocation,
            phase: std::marker::PhantomData,
        })
    }

    /// Return the exact cumulative allocation carried by the cached checked product.
    #[doc(hidden)]
    #[must_use]
    pub const fn cached_retained_usage(&self) -> (usize, usize) {
        self._allocation.retained_usage()
    }

    /// Return the classified contract identity consumed by compilation.
    #[must_use]
    pub const fn contract_fingerprint(&self) -> crate::contract::fingerprint::Fingerprint {
        self.contract
    }

    /// Return exact Rawr snapshot evidence consumed by compilation.
    #[must_use]
    pub const fn snapshot(&self) -> Snapshot {
        self.snapshot
    }

    /// Iterate over sorted, deduplicated symbolic uses.
    pub fn entries(&self) -> impl ExactSizeIterator<Item = &Entry> {
        self.entries.iter()
    }

    /// Borrow the exact private adapter plans consumed by Phase 3 generation.
    #[cfg(feature = "rustc-driver")]
    pub(crate) fn adapter_plans(&self) -> &[adapter::Plan] {
        &self.plans
    }

    pub(crate) fn into_request(self) -> Request<B> {
        Request {
            _brand: self._brand,
            runner: self.runner,
            contract: self.contract,
            snapshot: self.snapshot,
            entries: self.entries,
            plans: self.plans,
            canonical: self.canonical,
            fingerprint: self.fingerprint,
            allocation: self._allocation,
        }
    }

    /// Return the bridge-surface identity, independent of Rawr body bytes.
    #[must_use]
    pub const fn fingerprint(&self) -> Fingerprint {
        self.fingerprint
    }

    /// Borrow the exact canonical demand encoding.
    #[must_use]
    pub fn canonical_bytes(&self) -> &[u8] {
        &self.canonical
    }

    /// Inspect the cumulative sealed lineage at the driver audit boundary.
    #[cfg(feature = "rustc-driver")]
    #[doc(hidden)]
    #[must_use]
    pub const fn driver_retained_usage(&self) -> (usize, usize) {
        self._allocation.driver_usage()
    }
}

impl Fingerprint {
    /// Borrow exact digest bytes.
    #[must_use]
    pub const fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

impl Entry {
    /// Borrow the semantic imported surface identity.
    #[must_use]
    pub const fn identity(&self) -> &identity::Identity {
        &self.identity
    }

    /// Borrow the checked Rawr use-site.
    #[must_use]
    pub const fn origin(&self) -> &Origin {
        &self.origin
    }
}

/// Demand sealing failure that cannot mint a partial proof.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("owned runner inputs do not match the exact authenticated demand ancestry")]
    ForeignRunner,
    #[error("symbolic surface identity belongs to a different contract")]
    ForeignIdentity,
    #[error("symbolic allocation proof belongs to a different contract")]
    ForeignAllocation,
    #[error("classified item is excluded, rejected, or declaration-only")]
    NotDemandable,
    #[error("surface instance leaves a generic argument unconstrained")]
    UnconstrainedGeneric,
    #[error("surface instance arguments do not match the selected declaration")]
    InstanceMismatch,
    #[error("consuming builder replay is not one constructor followed by owned methods")]
    BuilderShape,
    #[error("consuming builder arguments must be contiguous and used exactly once")]
    BuilderArgument,
    #[error("Rawr demand origin is not a valid half-open range")]
    InvalidOrigin,
    #[error("cannot derive exact adapter plan: {reason}")]
    AdapterPlan { reason: &'static str },
    #[error("Rawr snapshot inputs are duplicated or not in canonical order")]
    SnapshotOrder,
    #[error("Rawr snapshot has no application entry target")]
    MissingEntry,
    #[error("Rawr snapshot contains a non-UTF-8 canonical path")]
    NonUtf8Path,
    #[error("Rawr snapshot contains no source inputs")]
    EmptySnapshot,
    #[error("{resource} exceeded limit {limit} with observed size {actual}")]
    Limit {
        resource: &'static str,
        limit: usize,
        actual: usize,
    },
    #[error("cannot allocate canonical symbolic demand")]
    Allocation,
    #[error("cannot encode canonical symbolic demand")]
    Encoding,
    #[error("stored symbolic demand is not canonical")]
    Canonical,
}

impl Serialize for UniqueIdentities<'_> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut sequence = serializer.serialize_seq(Some(unique_identity_count(self.0)))?;
        for identity in unique_identities(self.0) {
            sequence.serialize_element(identity)?;
        }
        sequence.end()
    }
}

fn unique_identity_count(entries: &[Entry]) -> usize {
    unique_identities(entries).count()
}

fn unique_identities(entries: &[Entry]) -> impl Iterator<Item = &identity::Identity> {
    entries.iter().enumerate().filter_map(|(index, entry)| {
        (index == 0 || entries[index - 1].identity != entry.identity).then_some(&entry.identity)
    })
}

impl Write for Measured {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        let Some(actual) = self.bytes.checked_add(buffer.len()) else {
            self.overflow = true;
            return Err(io::Error::other("canonical demand byte count overflowed"));
        };
        self.bytes = actual;
        Ok(buffer.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl Write for Hashed {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        let Some(actual) = self.bytes.checked_add(buffer.len()) else {
            self.overflow = true;
            return Err(io::Error::other("canonical surface byte count overflowed"));
        };
        self.bytes = actual;
        self.digest.update(buffer);
        Ok(buffer.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::adapter::{Declaration, Operation};
    use super::{Error, Origin, Snapshot};
    use crate::classify::Policy;

    fn contract() -> crate::Contract<crate::Classified> {
        crate::test_fixture::graph()
            .classify(&Policy::standard(), &crate::cargo::Limits::standard())
            .expect("fixture graph classifies")
    }

    fn snapshot(contents: &[u8], limits: &crate::cargo::Limits) -> Snapshot {
        let mut builder = Snapshot::builder(limits);
        builder
            .entry("/fixture/Cargo.toml")
            .expect("snapshot entry is bounded");
        builder
            .file("/fixture/main.rs", contents)
            .expect("snapshot input is bounded");
        builder.seal().expect("snapshot has one exact input")
    }

    fn p2_limits(
        retained_bytes: usize,
        retained_items: usize,
        string_bytes: usize,
    ) -> crate::cargo::Limits {
        let mut limits =
            crate::cargo::Limits::fixture(retained_bytes, retained_items, string_bytes);
        limits.classified_contract_items = retained_items;
        limits.classified_contract_bytes = retained_bytes;
        limits
    }

    #[test]
    fn body_change_rebinds_demand_without_changing_its_surface() {
        let limits = crate::cargo::Limits::standard();
        let contract = contract();
        let identity = contract
            .outcomes()
            .next()
            .expect("fixture has a struct")
            .identity()
            .clone();
        let origin = Origin::checked("/fixture/main.rs", 0, 4, &limits).expect("origin is bounded");
        let mut first = contract
            .compiler((), snapshot(b"Pair", &limits), &limits)
            .expect("compiler binds first snapshot");
        first
            .require(identity.clone(), origin.clone())
            .expect("struct demand is admitted");
        let first = first.seal().expect("first demand seals");
        let mut changed = contract
            .compiler((), snapshot(b"Pair // body", &limits), &limits)
            .expect("compiler binds changed snapshot");
        changed
            .require(identity, origin)
            .expect("same surface demand is admitted");
        let changed = changed.seal().expect("changed demand seals");

        assert_ne!(first.snapshot(), changed.snapshot());
        assert_ne!(first.canonical_bytes(), changed.canonical_bytes());
        assert_eq!(first.fingerprint(), changed.fingerprint());
    }

    #[test]
    fn cached_demand_replays_exact_entries_plans_fingerprint_and_allocation() {
        let limits = crate::cargo::Limits::standard();
        let contract = contract();
        let identity = contract
            .outcomes()
            .find(|outcome| outcome.representability().is_demandable())
            .expect("the fixture has one demandable outcome")
            .identity()
            .clone();
        let source = snapshot(b"Pair", &limits);
        let origin = Origin::checked("/fixture/main.rs", 0, 4, &limits).expect("origin is bounded");
        let mut compiler = contract
            .compiler((), source, &limits)
            .expect("the fixture compiler starts");
        compiler
            .require(identity, origin)
            .expect("the fixture demand is retained");
        let demand = compiler.seal().expect("the fixture demand seals");
        let canonical = demand.canonical_bytes().to_vec();
        let fingerprint = demand.fingerprint();
        let retained = demand.cached_retained_usage();
        let replayed =
            super::Demand::replay_cached(&canonical, retained, (), &contract, source, &limits)
                .expect("the cached fixture demand replays");
        assert_eq!(replayed.canonical_bytes(), canonical.as_slice());
        assert_eq!(replayed.fingerprint(), fingerprint);
        assert_eq!(replayed.cached_retained_usage(), retained);

        let mut trailing = canonical;
        trailing.push(b' ');
        assert!(
            super::Demand::replay_cached(&trailing, retained, (), &contract, source, &limits,)
                .is_err()
        );
    }

    #[test]
    fn sealed_demand_retains_one_exact_plan_per_unique_operation() {
        let limits = crate::cargo::Limits::standard();
        let contract = contract();
        let outcomes = contract.outcomes().collect::<Vec<_>>();
        let structure = outcomes[0];
        let enumeration = outcomes[1];
        let constant = outcomes[2];
        let function = outcomes[3];
        let crate::classify::representability::Representability::Struct(structure_plan) =
            structure.representability()
        else {
            panic!("fixture structure is representable")
        };
        let crate::classify::representability::Representability::Enum(enumeration_plan) =
            enumeration.representability()
        else {
            panic!("fixture enum is representable")
        };
        let constructor = structure_plan
            .constructor()
            .expect("fixture structure has an exact constructor")
            .try_clone()
            .expect("constructor selector clones fallibly");
        let field = structure_plan.fields()[0]
            .selector()
            .try_clone()
            .expect("field selector clones fallibly");
        let variant = enumeration_plan.variants()[1]
            .selector()
            .try_clone()
            .expect("variant selector clones fallibly");
        let mut compiler = contract
            .compiler((), snapshot(b"adapter plans", &limits), &limits)
            .expect("adapter-plan compiler starts");
        compiler
            .require(
                structure.identity().clone(),
                Origin::checked("/fixture/main.rs", 0, 1, &limits).unwrap(),
            )
            .unwrap();
        compiler
            .require_selected(structure.identity(), constructor, "/fixture/main.rs", 2, 3)
            .unwrap();
        compiler
            .require_selected(structure.identity(), field, "/fixture/main.rs", 4, 5)
            .unwrap();
        compiler
            .require_selected(enumeration.identity(), variant, "/fixture/main.rs", 6, 7)
            .unwrap();
        for (index, outcome) in [constant, function].into_iter().enumerate() {
            compiler
                .require(
                    outcome.identity().clone(),
                    Origin::checked(
                        "/fixture/main.rs",
                        8 + index as u32,
                        9 + index as u32,
                        &limits,
                    )
                    .unwrap(),
                )
                .unwrap();
        }
        compiler
            .require(
                function.identity().clone(),
                Origin::checked("/fixture/main.rs", 20, 21, &limits).unwrap(),
            )
            .unwrap();
        let demand = compiler.seal().expect("adapter plans seal");

        assert_eq!(demand.entries().len(), 7);
        assert_eq!(demand.adapter_plans().len(), 6);
        assert!(demand.adapter_plans().iter().any(|plan| {
            plan.export().root_alias() == "fixture"
                && plan.export().path().eq(["item_0"])
                && plan.name() == "Pair"
                && matches!(
                    plan.operation(),
                    Operation::Declaration {
                        kind: Declaration::Struct,
                        parameters,
                    } if parameters.is_empty()
                )
        }));
        assert!(demand.adapter_plans().iter().any(|plan| matches!(
            plan.operation(),
            Operation::StructConstructor {
                parameters,
                form: crate::api::model::FieldForm::Record,
                fields,
                ..
            } if parameters.is_empty() && fields.len() == 2
                && fields[0].name() == "left"
                && fields[0].index() == 0
        )));
        assert!(demand.adapter_plans().iter().any(|plan| matches!(
            plan.operation(),
            Operation::StructField { parameters, field, .. }
                if parameters.is_empty()
                    && field.name() == "left"
                    && field.span().byte_range() == (10..14)
                    && matches!(
                        field.ty(),
                        crate::classify::representability::convert::Type::Builtin(
                            crate::classify::representability::convert::Builtin::I64
                        )
                    )
        )));
        assert!(demand.adapter_plans().iter().any(|plan| matches!(
            plan.operation(),
            Operation::EnumVariant {
                parameters,
                name,
                form: crate::api::model::FieldForm::Tuple,
                fields,
            } if parameters.is_empty() && name.as_ref() == "Count" && fields.len() == 1
        )));
        assert!(demand.adapter_plans().iter().any(|plan| matches!(
            plan.operation(),
            Operation::Constant { ty, value, .. }
                if matches!(
                    ty,
                    crate::classify::representability::convert::Type::Builtin(
                        crate::classify::representability::convert::Builtin::I64
                    )
                ) && value
                    .as_ref()
                    .is_some_and(|value| value.ty() == "i64" && value.value() == "42")
        )));
        assert!(demand.adapter_plans().iter().any(|plan| {
            matches!(plan.operation(), Operation::Function { signature, .. } if signature.inputs().len() == 1)
                && plan.origin().source().byte_range() == (103..104)
                && plan.origin().declaration().byte_range() == (60..78)
                && plan.identity().as_bytes() != &[0; 32]
        }));
    }

    #[test]
    fn unresolved_generic_structure_is_rejected_before_demand_or_adapter() {
        let limits = crate::cargo::Limits::standard();
        // A struct whose every parameter has a default is reached at that
        // instance, as iced's `Rectangle<T = f32>` is `Rectangle<f32>`.
        let contract = crate::test_fixture::iced_profile()
            .classify(&Policy::standard(), &limits)
            .expect("iced-profile fixture classifies");
        let rectangle = contract
            .outcomes()
            .find(|outcome| outcome.name() == "Rectangle")
            .expect("fixture has one generic Rectangle");
        let mut compiler = contract
            .compiler((), snapshot(b"Rectangle", &limits), &limits)
            .expect("defaulted-structure compiler starts");
        compiler
            .require(
                rectangle.identity().clone(),
                Origin::checked("/fixture/main.rs", 0, 1, &limits).unwrap(),
            )
            .expect("a defaulted generic struct is demandable");

        // One with a parameter without a default is not.
        let contract = crate::test_fixture::native_shapes()
            .classify(&Policy::standard(), &limits)
            .expect("native-shapes fixture classifies");
        let outcome = contract
            .outcomes()
            .find(|outcome| outcome.name() == "Pair")
            .expect("fixture has one generic Pair");
        let crate::classify::representability::Representability::Struct(plan) =
            outcome.representability()
        else {
            panic!("Pair is a structural outcome")
        };
        let field = plan
            .fields()
            .first()
            .expect("Pair has a field selector")
            .selector();
        let mut compiler = contract
            .compiler((), snapshot(b"Pair", &limits), &limits)
            .expect("generic-structure compiler starts");

        assert!(matches!(
            compiler.require(
                outcome.identity().clone(),
                Origin::checked("/fixture/main.rs", 0, 1, &limits).unwrap(),
            ),
            Err(Error::UnconstrainedGeneric)
        ));
        assert!(matches!(
            compiler.require_selected(
                outcome.identity(),
                field.try_clone().unwrap(),
                "/fixture/main.rs",
                2,
                3,
            ),
            Err(Error::UnconstrainedGeneric)
        ));
        let empty = super::instance::Arguments::new(
            Box::default(),
            Box::default(),
            Box::default(),
            None,
            &limits,
        )
        .expect("empty arguments are bounded");
        assert!(matches!(
            compiler.require_instance(
                outcome.identity(),
                field.try_clone().unwrap(),
                empty,
                "/fixture/main.rs",
                4,
                5,
            ),
            Err(Error::UnconstrainedGeneric)
        ));
        assert!(matches!(
            super::adapter::Plan::try_new(&contract, outcome, outcome.identity()),
            Err(Error::AdapterPlan {
                reason: "generic structural adapter requires concrete substitution proof",
            })
        ));
    }

    #[test]
    fn exact_receiver_plan_and_surface_fingerprint_remain_stable() {
        let limits = crate::cargo::Limits::standard();
        let contract = crate::test_fixture::representability_families()
            .classify(&Policy::standard(), &limits)
            .expect("receiver fixture classifies");
        let method = contract.outcomes().nth(1).expect("fixture method exists");
        let compile = |range: std::ops::Range<u32>| {
            let mut compiler = contract
                .compiler((), snapshot(b"receiver", &limits), &limits)
                .expect("receiver compiler starts");
            compiler
                .require(
                    method.identity().clone(),
                    Origin::checked("/fixture/main.rs", range.start, range.end, &limits).unwrap(),
                )
                .unwrap();
            compiler.seal().expect("receiver demand seals")
        };
        let first = compile(0..1);
        let second = compile(2..3);

        assert_eq!(first.fingerprint(), second.fingerprint());
        assert_ne!(first.canonical_bytes(), second.canonical_bytes());
        assert!(matches!(
            first.adapter_plans()[0].operation(),
            Operation::Method {
                receiver: crate::classify::representability::ownership::Receiver::Shared,
                signature,
                ..
            } if signature.inputs().is_empty()
        ));
    }

    #[test]
    fn adapter_plan_allocation_accepts_exact_and_records_one_short() {
        let standard = crate::cargo::Limits::standard();
        let contract = contract();
        let identity = contract
            .outcomes()
            .nth(3)
            .expect("fixture function exists")
            .identity()
            .clone();
        let seal = |limits: &crate::cargo::Limits| {
            let mut compiler = contract
                .compiler((), snapshot(b"plan allocation", limits), limits)
                .expect("allocation compiler starts");
            compiler
                .require(
                    identity.clone(),
                    Origin::checked("/fixture/main.rs", 0, 1, limits).unwrap(),
                )
                .expect("one function use is retained");
            compiler.seal()
        };
        let demand = seal(&standard).expect("standard adapter plan seals");
        let exact_usage = demand._allocation.usage();
        let exact = p2_limits(exact_usage.1, exact_usage.0, standard.string_bytes);
        seal(&exact).expect("exact adapter-plan allocation seals");

        // The P2 limits are accounting targets. One short on either
        // axis still seals and records the truthful cumulative amount.
        let short_items = p2_limits(exact_usage.1, exact_usage.0 - 1, standard.string_bytes);
        let sealed = seal(&short_items).expect("the item target records and admits");
        let axis = *sealed
            ._allocation
            .exceedances()
            .axis("P2 cumulative retained items")
            .unwrap();
        assert_eq!(axis.target as usize, exact_usage.0 - 1);
        assert_eq!(axis.high_water as usize, exact_usage.0);
        let short_bytes = p2_limits(exact_usage.1 - 1, exact_usage.0, standard.string_bytes);
        let sealed = seal(&short_bytes).expect("the byte target records and admits");
        let axis = *sealed
            ._allocation
            .exceedances()
            .axis("P2 cumulative retained bytes")
            .unwrap();
        assert_eq!(axis.target as usize, exact_usage.1 - 1);
        assert_eq!(axis.high_water as usize, exact_usage.1);
    }

    #[test]
    fn demand_is_sorted_deduplicated_and_numerically_bounded() {
        let limits = crate::cargo::Limits::standard();
        let contract = contract();
        let identities = contract
            .outcomes()
            .take(2)
            .map(|outcome| outcome.identity().clone())
            .collect::<Vec<_>>();
        let late = Origin::checked("/fixture/main.rs", 8, 12, &limits).unwrap();
        let early = Origin::checked("/fixture/main.rs", 0, 4, &limits).unwrap();
        let mut compiler = contract
            .compiler((), snapshot(b"Pair Choice", &limits), &limits)
            .unwrap();
        compiler.require(identities[1].clone(), late).unwrap();
        compiler
            .require(identities[0].clone(), early.clone())
            .unwrap();
        compiler.require(identities[0].clone(), early).unwrap();
        let demand = compiler.seal().unwrap();
        assert_eq!(demand.entries().len(), 2);
        assert!(demand.entries().is_sorted());

        let standard = crate::cargo::Limits::standard();
        let exact_snapshot = snapshot(b"Pair Choice", &standard);
        let base = contract
            .allocation(&standard)
            .expect("contract allocation proof starts")
            .usage();
        let exact_items = base
            .0
            .checked_add(exact_snapshot.1.items)
            .and_then(|items| items.checked_add(1))
            .expect("fixture retained items fit usize");
        let one = p2_limits(usize::MAX, exact_items, usize::MAX);
        let mut compiler = contract
            .compiler((), exact_snapshot, &one)
            .expect("the exact cumulative item boundary starts");
        compiler
            .require(
                identities[0].clone(),
                Origin::checked("/fixture/main.rs", 0, 1, &one).unwrap(),
            )
            .expect("the exact cumulative item boundary accepts");
        assert!(!compiler.allocation.exceedances().is_exceeded());
        // One past the cumulative item target is accepted and recorded.
        compiler
            .require(
                identities[1].clone(),
                Origin::checked("/fixture/main.rs", 2, 3, &one).unwrap(),
            )
            .expect("the item target records and admits");
        let axis = *compiler
            .allocation
            .exceedances()
            .axis("P2 cumulative retained items")
            .unwrap();
        assert_eq!(axis.target as usize, exact_items);
        assert_eq!(axis.first_requested as usize, exact_items + 1);

        let facade_bytes = 64_usize;
        let facade_items = 3_usize;
        let exact_facade_bytes = base
            .1
            .checked_add(facade_bytes)
            .expect("fixture facade aggregate fits usize");
        let exact_facade_items = base
            .0
            .checked_add(facade_items)
            .expect("fixture facade aggregate fits usize");
        let exact_facade = p2_limits(exact_facade_bytes, exact_facade_items, usize::MAX);
        contract
            .allocation(&exact_facade)
            .and_then(|mut allocation| {
                allocation.retain_checked(Some(facade_items), Some(facade_bytes))
            })
            .expect("the checked facade aggregate accepts its exact boundary");
        let short_facade = p2_limits(exact_facade_bytes - 1, exact_facade_items, usize::MAX);
        let recorded = contract
            .allocation(&short_facade)
            .and_then(|mut allocation| {
                allocation
                    .retain_checked(Some(facade_items), Some(facade_bytes))
                    .map(|()| allocation)
            })
            .expect("the byte target records and admits");
        let axis = *recorded
            .exceedances()
            .axis("P2 cumulative retained bytes")
            .unwrap();
        assert_eq!(axis.target as usize, exact_facade_bytes - 1);
        assert_eq!(axis.high_water as usize, exact_facade_bytes);

        let exact_origin = Origin::checked("/fixture/main.rs", 0, 1, &standard).unwrap();
        let entry_bytes = super::entry_retained_bytes(
            identities[0].checked_retained_bytes(),
            exact_origin.source.len(),
        )
        .expect("fixture entry size fits usize");
        let exact_bytes = base
            .1
            .checked_add(exact_snapshot.1.bytes)
            .and_then(|bytes| bytes.checked_add(entry_bytes))
            .expect("fixture retained bytes fit usize");
        let exact = p2_limits(exact_bytes, usize::MAX, usize::MAX);
        contract
            .compiler((), exact_snapshot, &exact)
            .and_then(|mut compiler| compiler.require(identities[0].clone(), exact_origin.clone()))
            .expect("the exact cumulative byte boundary accepts");
        let short = p2_limits(
            exact_bytes
                .checked_sub(1)
                .expect("positive exact fixture bytes"),
            usize::MAX,
            usize::MAX,
        );
        let compiler = contract
            .compiler((), exact_snapshot, &short)
            .and_then(|mut compiler| {
                compiler
                    .require(identities[0].clone(), exact_origin)
                    .map(|()| compiler)
            })
            .expect("the byte target records and admits");
        let axis = *compiler
            .allocation
            .exceedances()
            .axis("P2 cumulative retained bytes")
            .unwrap();
        assert_eq!(axis.target as usize, exact_bytes - 1);
        assert_eq!(axis.high_water as usize, exact_bytes);
    }

    #[test]
    fn snapshot_and_origin_reject_noncanonical_evidence_and_record_targets() {
        let limits = crate::cargo::Limits::fixture(4, 2, 8);
        let mut order = Snapshot::builder(&limits);
        order.entry("e").unwrap();
        order.file("b", b"").unwrap();
        assert!(matches!(order.file("a", b""), Err(Error::SnapshotOrder)));

        // The byte target is accounting. One over the target still
        // retains the file and records the truthful cumulative figure.
        let mut bytes = Snapshot::builder(&limits);
        bytes.entry("e").unwrap();
        bytes
            .file("path", b"x")
            .expect("a file past the byte target is retained");
        let axis = *bytes
            .exceedances()
            .axis("snapshot retained bytes")
            .expect("the byte target records its axis");
        assert_eq!(
            (axis.target, axis.first_requested, axis.high_water),
            (4, 6, 6)
        );
        assert!(bytes
            .exceedances()
            .axis("snapshot retained items")
            .is_none());
        assert!(matches!(
            Origin::checked("source", 5, 4, &limits),
            Err(Error::InvalidOrigin)
        ));
        // A free-standing origin past the string target admits as well.
        Origin::checked("source beyond eight", 0, 1, &limits)
            .expect("the origin string target is recorded, not refused");

        let item_short = crate::cargo::Limits::fixture(usize::MAX, 1, usize::MAX);
        let mut item_builder = Snapshot::builder(&item_short);
        item_builder.entry("e").unwrap();
        assert!(matches!(item_builder.seal(), Err(Error::EmptySnapshot)));

        let mut item_builder = Snapshot::builder(&item_short);
        item_builder.entry("e").unwrap();
        assert!(!item_builder.exceedances().is_exceeded());
        item_builder
            .file("a", b"")
            .expect("a file past the item target is retained");
        let axis = *item_builder
            .exceedances()
            .axis("snapshot retained items")
            .expect("the item target records its axis");
        assert_eq!((axis.target, axis.high_water), (1, 2));
        item_builder.file("b", b"").unwrap();
        assert_eq!(
            item_builder
                .exceedances()
                .axis("snapshot retained items")
                .unwrap()
                .high_water,
            3
        );
        assert_eq!(item_builder.exceedances().axes().count(), 1);
        let sealed = item_builder
            .seal()
            .expect("the snapshot seals past its item target");
        assert_eq!(sealed.1.items, 3);

        // Exactly at the targets admits silently and seals to the same digest as
        // a snapshot built under the standard targets.
        let exact = crate::cargo::Limits::fixture(3, 2, 8);
        let mut bounded = Snapshot::builder(&exact);
        bounded.entry("e").unwrap();
        bounded.file("a", b"x").unwrap();
        assert!(!bounded.exceedances().is_exceeded());
        let bounded = bounded.seal().unwrap();
        let mut standard = Snapshot::builder(&crate::cargo::Limits::standard());
        standard.entry("e").unwrap();
        standard.file("a", b"x").unwrap();
        assert_eq!(bounded, standard.seal().unwrap());

        // The path string target is recorded on the entry and on files alike.
        let strings = crate::cargo::Limits::fixture(usize::MAX, usize::MAX, 1);
        let mut paths = Snapshot::builder(&strings);
        paths.entry("ee").unwrap();
        paths.file("long", b"").unwrap();
        assert_eq!(
            paths
                .exceedances()
                .axis("snapshot entry path bytes")
                .map(|axis| (axis.target, axis.high_water)),
            Some((1, 2))
        );
        assert_eq!(
            paths
                .exceedances()
                .axis("snapshot path bytes")
                .map(|axis| (axis.target, axis.high_water)),
            Some((1, 4))
        );
    }

    /// The compiler's entry, origin and canonical byte targets are
    /// recorded on its surface account with truthful figures; the demand still
    /// seals and the only refusals left are the P2 allocation and arithmetic.
    #[test]
    fn compiler_entry_and_canonical_targets_record_without_refusal() {
        let standard = crate::cargo::Limits::standard();
        let contract = contract();
        let identities = contract
            .outcomes()
            .filter(|outcome| outcome.representability().is_demandable())
            .take(2)
            .map(|outcome| outcome.identity().clone())
            .collect::<Vec<_>>();
        assert_eq!(identities.len(), 2);
        let seal = |limits: &crate::cargo::Limits| {
            let mut compiler = contract
                .compiler((), snapshot(b"targets", &standard), limits)
                .expect("compiler starts under every accounting target");
            for (index, identity) in identities.iter().enumerate() {
                compiler
                    .require_selected(
                        identity,
                        identity.selector().try_clone().expect("selector clones"),
                        "/fixture/main.rs",
                        index as u32,
                        index as u32 + 1,
                    )
                    .expect("entries are retained past every accounting target");
            }
            let record = compiler.exceedances();
            let demand = compiler.seal().expect("the demand seals past every target");
            (record, demand)
        };
        let (record, demand) = seal(&standard);
        assert!(!record.is_exceeded());
        let canonical = demand.canonical_bytes().len();

        let items = crate::cargo::Limits::fixture(usize::MAX, 1, usize::MAX);
        let (record, bounded) = seal(&items);
        let axis = *record
            .axis("symbolic demand entries")
            .expect("the entry target records its axis");
        assert_eq!(
            (axis.target, axis.first_requested, axis.high_water),
            (1, 2, 2)
        );
        assert_eq!(bounded.entries().len(), demand.entries().len());
        assert_eq!(bounded.fingerprint(), demand.fingerprint());

        let strings = crate::cargo::Limits::fixture(usize::MAX, usize::MAX, 4);
        let (record, _) = seal(&strings);
        let axis = *record
            .axis("demand origin bytes")
            .expect("the origin string target records its axis");
        assert_eq!(
            (axis.target, axis.high_water),
            (4, "/fixture/main.rs".len() as u64)
        );

        // The canonical byte target is only measurable at sealing, so it is
        // observed through the cached replay of the sealed bytes below; here
        // the demand simply seals one byte under its own canonical size.
        let bytes = crate::cargo::Limits::fixture(canonical - 1, usize::MAX, usize::MAX);
        let (_, bounded) = seal(&bytes);
        assert_eq!(bounded.canonical_bytes(), demand.canonical_bytes());
    }

    /// A cached demand whose canonical bytes exceed the configured JSON
    /// target replays whole and records the axis; exactly at the target admits
    /// silently. The physical bound is the node content ceiling, not the target.
    #[test]
    fn cached_demand_past_the_json_target_replays_and_records() {
        let standard = crate::cargo::Limits::standard();
        let contract = contract();
        let identity = contract
            .outcomes()
            .find(|outcome| outcome.representability().is_demandable())
            .expect("the fixture has one demandable outcome")
            .identity()
            .clone();
        let source = snapshot(b"Pair", &standard);
        let mut compiler = contract
            .compiler((), source, &standard)
            .expect("the fixture compiler starts");
        compiler
            .require(
                identity,
                Origin::checked("/fixture/main.rs", 0, 4, &standard).unwrap(),
            )
            .expect("the fixture demand is retained");
        let demand = compiler.seal().expect("the fixture demand seals");
        let canonical = demand.canonical_bytes().to_vec();
        let retained = demand.cached_retained_usage();
        let replay = |json_bytes: usize| {
            let mut limits = standard;
            limits.json_bytes = json_bytes;
            let mut account = super::account::Account::surface();
            let replayed = super::Demand::replay_cached_accounted(
                &canonical,
                retained,
                (),
                &contract,
                source,
                &limits,
                &mut account,
            )
            .expect("the cached demand replays under every JSON target");
            assert_eq!(replayed.canonical_bytes(), canonical.as_slice());
            assert_eq!(replayed.fingerprint(), demand.fingerprint());
            account.exceedances()
        };
        assert!(!replay(canonical.len()).is_exceeded());
        let record = replay(canonical.len() - 1);
        let axis = *record
            .axis("cached symbolic demand bytes")
            .expect("the cached byte target records its axis");
        assert_eq!(
            (axis.target, axis.first_requested, axis.high_water),
            (
                (canonical.len() - 1) as u64,
                canonical.len() as u64,
                canonical.len() as u64
            )
        );
        // The surface canonical is smaller than the demand canonical, so a
        // target far below both records that axis too, with its own size.
        let record = replay(1);
        let surface = record
            .axis("cached canonical surface bytes")
            .expect("the surface byte target records its axis");
        assert!(surface.high_water > 1 && surface.high_water < canonical.len() as u64);
    }
}
