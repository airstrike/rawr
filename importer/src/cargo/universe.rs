//! Versioned Rawr-owned Cargo resolution universes.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::path::Path;
use std::sync::Arc;

#[cfg(test)]
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use serde_json::{Map as JsonMap, Value as JsonValue};
use sha2::{Digest as _, Sha256};
use toml_edit::{DocumentMut, Item, Table};

use super::unit::{Budgeted, JsonAllocation, JsonOwnership};
use super::{Digest, Limits};
use crate::{Error, Result};

const SCHEMA: u32 = 1;
pub(crate) const BUILD_PROJECTION_SCHEMA: u32 = 4;
const BUILD_PROJECTION_DOMAIN: &[u8] = b"rawr.import.cargo.protected-source-build/v1";
/// The fixtures' host package, whose closure their universe governs.
#[cfg(test)]
const ROOT_PACKAGE: &str = "rawr-iced";
/// The framework crate whose exact package the fixtures' universe fixes.
#[cfg(test)]
const FRAMEWORK_PACKAGE: &str = "iced";
/// The runtime crate a runner with no host profile links: the plain
/// universe governs its closure and fixes no framework.
const PLAIN_ROOT: &str = "rawr";
const PLAIN_NAME: &str = "rawr.plain/current";
const PLAIN_VERSION: u32 = 1;
const PLAIN_ENDPOINT_TABLE: u32 = 1;
const PLAIN_PROFILE: [u8; 32] = [0; 32];
/// The plain seed names only its root: every other package is the
/// project's own Cargo resolution.
const PLAIN_SEED_BYTES: &[u8] =
    b"version = 4\n\n[[package]]\nname = \"rawr\"\nversion = \"1.0.0\"\n";
const RECEIPT_SCHEMA: u32 = 2;
const TARGET: &str = super::toolchain::HOST;
const CARGO_PROFILE: &str = "dev";

/// Exact bounded canonical protected source/build facts observed after build.
#[derive(Debug, Eq, PartialEq)]
pub(crate) struct BuildProjection {
    bytes: Box<[u8]>,
    digest: Digest,
}

/// A supported, immutable profile-connected Cargo closure.
///
/// The lock bytes are a seed, not the whole-project lock. A fresh project may
/// add unrelated packages before the candidate is promoted to [`FinalLock`],
/// but every package and edge reachable from the selected profile root must
/// remain exact.
#[derive(Clone)]
pub(crate) struct Definition {
    seed: &'static [u8],
    /// The package whose lib unit roots the governed closure.
    root: &'static str,
    /// The framework package whose exact version and source the facade
    /// fixes, if any.
    framework: Option<&'static str>,
    protected: Closure,
    /// The identity's state before the target and the toolchain, which
    /// [`Definition::for_target`] completes for another target.
    unbound: Sha256,
    target: Box<str>,
    cargo_profile: &'static str,
    rustc_commit: &'static str,
    cargo_commit: &'static str,
    identity: Digest,
}

/// One selected current universe.
#[derive(Clone)]
pub struct Catalog {
    current: Definition,
}

/// A candidate lock whose project-specific packages have not yet been admitted.
///
/// Clones share one charged parse; a staged candidate can serve later replays
/// in the same evaluation without reinterpreting its exact owned bytes.
#[derive(Clone)]
pub(crate) struct Candidate(Arc<Locked>);

struct Locked {
    definition: Definition,
    bytes: Box<[u8]>,
    digest: Digest,
    closure: Closure,
}

/// Exact supported whole-project lock authority. It retains the exact admitted
/// bytes, through the candidate's shared parse, as its state authority.
pub(crate) struct FinalLock(Arc<Locked>);

/// Universe identity completed with the exact final metadata and unit graph.
#[derive(Clone, Copy)]
pub(crate) struct Proof {
    base: Digest,
    complete: Digest,
    final_lock: Digest,
}

#[derive(Clone, Copy, Eq, PartialEq)]
pub(crate) struct ContextProof {
    universe: Digest,
    digest: Digest,
}

impl ContextProof {
    /// The validated command, config and toolchain context.
    pub(crate) const fn digest(&self) -> Digest {
        self.digest
    }
}

pub(crate) struct MetadataProof {
    universe: Digest,
    digest: Digest,
    packages: BTreeMap<Arc<str>, ProtectedPackage>,
    _ownership: JsonOwnership,
}

pub(crate) struct MetadataObservation {
    universe: Digest,
    packages: Budgeted<BTreeMap<Arc<str>, ProtectedPackage>>,
    projected: Budgeted<BTreeMap<Arc<str>, ProjectedPackage>>,
}

impl MetadataObservation {
    #[cfg(test)]
    fn live_ownership(&self) -> Result<JsonOwnership> {
        self.packages
            .ownership()
            .checked_add(self.projected.ownership())
    }
}

#[derive(Clone, Copy)]
pub(crate) struct UnitProof {
    universe: Digest,
    digest: Digest,
}

#[derive(Clone, Copy)]
pub(crate) struct BuildProof {
    universe: Digest,
    projection: Digest,
    input: Digest,
}

/// Pre-build evidence that cannot mint a completed universe proof or receipt.
pub(crate) struct Pending {
    request: Digest,
    lock: FinalLock,
    context: ContextProof,
    metadata: MetadataProof,
    unit: UnitProof,
    receipt: Option<Receipt>,
}

pub(crate) struct Completion {
    proof: Proof,
    receipt: Option<Receipt>,
}

/// Persisted Cargo-final receipt. It is never API/profile/product admission.
#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Receipt {
    schema: u32,
    request: [u8; 32],
    universe: [u8; 32],
    manifest: [u8; 32],
    source: [u8; 32],
    lock: [u8; 32],
    metadata: [u8; 32],
    unit: [u8; 32],
    build: [u8; 32],
    input: [u8; 32],
    complete: [u8; 32],
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct Package {
    name: Box<str>,
    version: Box<str>,
    source: Box<str>,
    checksum: Box<str>,
    dependencies: Box<[Key]>,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct Key {
    name: Arc<str>,
    version: Arc<str>,
    source: KeySource,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
enum KeySource {
    Path,
    Cargo(Arc<str>),
}

impl KeySource {
    fn from_shared(source: Option<Arc<str>>) -> Self {
        source.map_or(Self::Path, Self::Cargo)
    }

    fn is_empty(&self) -> bool {
        matches!(self, Self::Path)
    }
}

impl AsRef<str> for KeySource {
    fn as_ref(&self) -> &str {
        match self {
            Self::Path => "",
            Self::Cargo(source) => source,
        }
    }
}

impl From<&str> for KeySource {
    fn from(source: &str) -> Self {
        if source.is_empty() {
            Self::Path
        } else {
            Self::Cargo(source.into())
        }
    }
}

impl From<Box<str>> for KeySource {
    fn from(source: Box<str>) -> Self {
        if source.is_empty() {
            Self::Path
        } else {
            Self::Cargo(source.into())
        }
    }
}

impl From<Arc<str>> for KeySource {
    fn from(source: Arc<str>) -> Self {
        if source.is_empty() {
            Self::Path
        } else {
            Self::Cargo(source)
        }
    }
}

impl fmt::Display for KeySource {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_ref())
    }
}

#[derive(Clone, Debug)]
struct Closure {
    packages: Box<[Package]>,
    digest: Digest,
}

#[derive(Clone)]
pub(crate) struct ProtectedPackage {
    key: Key,
    root: Arc<Path>,
}

struct ProjectedPackage {
    package: Box<[u8]>,
    resolve: Option<Box<[u8]>>,
}

type DependencyIndex = BTreeMap<Box<str>, Option<Key>>;
/// Reachable unit indices and the package ids they reach.
type Reachability = (Budgeted<BTreeSet<usize>>, Budgeted<BTreeSet<Arc<str>>>);
/// Canonical metadata bytes and the protected packages they project.
type MetadataProjection = (
    Budgeted<Box<[u8]>>,
    Budgeted<BTreeMap<Arc<str>, ProtectedPackage>>,
);

impl BuildProjection {
    /// Transform one already-canonical bounded JSON record into owned proof state.
    pub(crate) fn from_canonical(bytes: Box<[u8]>) -> Self {
        let digest = build_projection_digest(&bytes);
        Self { bytes, digest }
    }

    pub(crate) fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    pub(crate) const fn digest(&self) -> Digest {
        self.digest
    }
}

fn build_projection_digest(bytes: &[u8]) -> Digest {
    let mut digest = Sha256::new();
    digest.update(BUILD_PROJECTION_DOMAIN);
    digest.update(bytes);
    Digest(digest.finalize().into())
}

impl Definition {
    #[cfg(test)]
    pub(crate) fn fixture_authenticated(seed: &'static [u8]) -> Self {
        Self::new(
            ROOT_PACKAGE,
            Some(FRAMEWORK_PACKAGE),
            "authenticated-fixture",
            1,
            1,
            [7; 32],
            seed,
            "aarch64-apple-darwin",
            "dev",
            "rustc",
            "cargo",
        )
        .expect("authenticated fixture universe seals")
    }

    #[cfg(test)]
    pub(crate) fn fixture_context(
        self,
        target: &str,
        profile: &str,
        proof: &super::toolchain::Proof,
    ) -> Self {
        Self::new(
            self.root,
            self.framework,
            "authenticated-fixture",
            1,
            1,
            [7; 32],
            self.seed,
            Box::leak(target.into()),
            Box::leak(profile.into()),
            Box::leak(proof.rustc_commit().into()),
            Box::leak(proof.cargo_commit().into()),
        )
        .expect("fixture definition binds actual toolchain inputs")
    }

    /// Transform one authenticated seed into a supported universe definition.
    #[allow(
        clippy::too_many_arguments,
        reason = "the universe binds every independent compiler-affecting authority"
    )]
    pub(crate) fn new(
        root: &'static str,
        framework: Option<&'static str>,
        name: &'static str,
        version: u32,
        endpoint_table: u32,
        profile: [u8; 32],
        seed: &'static [u8],
        target: &'static str,
        cargo_profile: &'static str,
        rustc_commit: &'static str,
        cargo_commit: &'static str,
    ) -> Result<Self> {
        assert!(
            !name.is_empty() && version != 0 && endpoint_table != 0,
            "a compiled-in resolution universe has a name and nonzero versions"
        );
        // The seed is compiled-in program text, not admitted input; its
        // size is fixed at build time.
        let seed_digest = Digest(Sha256::digest(seed).into());
        let protected = Closure::parse_rooted(seed, root)?;
        let mut identity = Sha256::new();
        identity.update(b"rawr.import.cargo.resolution-universe/v1");
        identity.update(SCHEMA.to_le_bytes());
        identity.update((name.len() as u64).to_le_bytes());
        identity.update(name.as_bytes());
        identity.update(version.to_le_bytes());
        identity.update(endpoint_table.to_le_bytes());
        identity.update(profile);
        identity.update(seed_digest.as_bytes());
        identity.update(protected.digest.as_bytes());
        // Rawr's own source identity, baked in at build time. A new Rawr
        // simply addresses new cache entries; it is never refused.
        identity.update(crate::host::DIGEST);
        Ok(Self {
            seed,
            root,
            framework,
            protected,
            identity: bound(
                identity.clone(),
                [target, cargo_profile, rustc_commit, cargo_commit],
            ),
            unbound: identity,
            target: target.into(),
            cargo_profile,
            rustc_commit,
            cargo_commit,
        })
    }

    /// The same universe for `target`, the one Cargo builds the project for.
    pub(crate) fn for_target(&self, target: &str) -> Self {
        let mut definition = self.clone();
        if *target != *self.target {
            definition.identity = bound(
                self.unbound.clone(),
                [
                    target,
                    self.cargo_profile,
                    self.rustc_commit,
                    self.cargo_commit,
                ],
            );
            definition.target = target.into();
        }
        definition
    }

    /// Borrow the stable base identity used by the owning profile table.
    #[must_use]
    pub const fn identity(&self) -> &[u8; 32] {
        self.identity.as_bytes()
    }

    pub(crate) fn candidate(&self, bytes: Box<[u8]>) -> Result<Candidate> {
        let closure = Closure::parse_rooted(&bytes, self.root)?;
        let digest = Digest(Sha256::digest(&bytes).into());
        Ok(Candidate(Arc::new(Locked {
            definition: self.clone(),
            bytes,
            digest,
            closure,
        })))
    }

    pub(crate) const fn seed(&self) -> &'static [u8] {
        self.seed
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn validate_context(
        &self,
        target: &str,
        cargo_profile: &str,
        rustc_commit: &str,
        cargo_commit: &str,
        config_schema: u32,
        graph_schema: u32,
        unit_schema: u32,
        command: Digest,
        config_context: Digest,
        config: Digest,
    ) -> Result<ContextProof> {
        if *target != *self.target
            || cargo_profile != self.cargo_profile
            || rustc_commit != self.rustc_commit
            || cargo_commit != self.cargo_commit
        {
            return Err(Error::ResolutionDrift {
                command: "resolution universe selection",
                field: "target/profile/toolchain cohort",
            });
        }
        let mut digest = Sha256::new();
        digest.update(b"rawr.import.cargo.resolution-context/v1");
        digest.update(self.identity.as_bytes());
        digest.update(config_schema.to_le_bytes());
        digest.update(graph_schema.to_le_bytes());
        digest.update(unit_schema.to_le_bytes());
        digest.update(command.as_bytes());
        digest.update(config_context.as_bytes());
        digest.update(config.as_bytes());
        Ok(ContextProof {
            universe: self.identity,
            digest: Digest(digest.finalize().into()),
        })
    }

    #[cfg(test)]
    pub(super) fn observe_metadata_value(
        &self,
        value: Budgeted<JsonValue>,
        metadata: Option<&super::unit::Metadata>,
        limits: &Limits,
        allocation: &mut JsonAllocation,
    ) -> Result<MetadataObservation> {
        self.observe_metadata_parts(value, None, metadata, limits, allocation)
    }

    pub(super) fn observe_metadata_records(
        &self,
        value: Budgeted<JsonValue>,
        records: super::unit::MetadataRecords,
        metadata: Option<&super::unit::Metadata>,
        limits: &Limits,
        allocation: &mut JsonAllocation,
    ) -> Result<MetadataObservation> {
        self.observe_metadata_parts(value, Some(records), metadata, limits, allocation)
    }

    fn observe_metadata_parts(
        &self,
        value: Budgeted<JsonValue>,
        records: Option<super::unit::MetadataRecords>,
        metadata: Option<&super::unit::Metadata>,
        limits: &Limits,
        allocation: &mut JsonAllocation,
    ) -> Result<MetadataObservation> {
        let consumed = value.ownership().checked_add(
            records
                .as_ref()
                .map_or(JsonOwnership::empty(), Budgeted::ownership),
        )?;
        allocation.transform_owned(consumed, |allocation| {
            let packages = match metadata {
                Some(metadata) => metadata_packages(metadata, allocation)?,
                #[cfg(test)]
                None => metadata_packages_from_value(value.value(), allocation)?,
                #[cfg(not(test))]
                None => return Err(json_schema("typed Cargo metadata authority is absent")),
            };
            let projected =
                metadata_observation(value, records, packages.value(), limits, allocation)?;
            Ok(MetadataObservation {
                universe: self.identity,
                packages,
                projected,
            })
        })
    }

    pub(super) fn authenticate_value(
        &self,
        value: &JsonValue,
        metadata: MetadataObservation,
        limits: &Limits,
        allocation: &mut JsonAllocation,
    ) -> Result<(MetadataProof, UnitProof)> {
        let consumed = metadata
            .packages
            .ownership()
            .checked_add(metadata.projected.ownership())?;
        allocation.transform_owned(consumed, |allocation| {
            require_universe(self.identity, metadata.universe, "protected metadata proof")?;
            let (reachable_units, reachable_packages) = unit_reachability_rooted(
                value,
                self.root,
                self.framework.is_none(),
                metadata.packages.value(),
                allocation,
            )?;
            let (metadata_bytes, packages) = metadata_projection_from_observation(
                metadata.projected,
                metadata.packages.value(),
                reachable_packages.value(),
                limits,
                allocation,
            )?;
            reachable_packages.drop_from(allocation)?;
            metadata.packages.drop_from(allocation)?;
            let metadata_digest = projection_digest_bytes(
                b"rawr.import.cargo.protected-metadata/v1",
                metadata_bytes.value(),
            );
            let unit_projection =
                unit_projection(value, packages.value(), reachable_units.value(), allocation)?;
            reachable_units.drop_from(allocation)?;
            let unit_bytes = unit_projection;
            let unit_digest =
                projection_digest_bytes(b"rawr.import.cargo.protected-unit/v1", unit_bytes.value());
            unit_bytes.drop_from(allocation)?;
            metadata_bytes.drop_from(allocation)?;
            let (packages, ownership) = packages.into_parts();
            Ok((
                MetadataProof {
                    universe: self.identity,
                    digest: metadata_digest,
                    packages,
                    _ownership: ownership,
                },
                UnitProof {
                    universe: self.identity,
                    digest: unit_digest,
                },
            ))
        })
    }

    #[cfg(test)]
    fn observe_metadata(&self, bytes: &[u8], limits: &Limits) -> Result<MetadataObservation> {
        let value = super::unit::bounded_json("protected Cargo metadata", bytes, limits)?;
        let mut allocation = super::unit::JsonAllocation::empty();
        let ownership = JsonOwnership::tree(&value)?;
        allocation.acquire(ownership)?;
        self.observe_metadata_value(
            Budgeted::from_retained(value, ownership),
            None,
            limits,
            &mut allocation,
        )
    }
}

/// A universe's identity: its `unbound` state completed by its target,
/// Cargo profile and toolchain commits.
fn bound(mut identity: Sha256, values: [&str; 4]) -> Digest {
    for value in values {
        identity.update((value.len() as u64).to_le_bytes());
        identity.update(value.as_bytes());
    }
    Digest(identity.finalize().into())
}

impl Catalog {
    /// The universe of a runner with no host profile: it governs the
    /// closure of the `rawr` runtime crate and fixes no framework.
    #[doc(hidden)]
    pub fn plain() -> Result<Self> {
        Definition::new(
            PLAIN_ROOT,
            None,
            PLAIN_NAME,
            PLAIN_VERSION,
            PLAIN_ENDPOINT_TABLE,
            PLAIN_PROFILE,
            PLAIN_SEED_BYTES,
            TARGET,
            CARGO_PROFILE,
            super::toolchain::active().rustc_commit(),
            super::toolchain::active().cargo_commit(),
        )
        .map(Self::new)
    }

    /// Select one current universe.
    pub(crate) fn new(current: Definition) -> Self {
        Self { current }
    }

    /// The same universe for `target`, the one Cargo builds the project for.
    pub(crate) fn for_target(&self, target: &str) -> Self {
        Self::new(self.current.for_target(target))
    }

    pub(crate) const fn current(&self) -> &Definition {
        &self.current
    }

    /// Whether the current universe's seed closure has a package of
    /// this name (a Rawr patch for it is one the facade build needs).
    pub(crate) fn names_package(&self, name: &str) -> bool {
        self.current
            .protected
            .packages
            .iter()
            .any(|package| package.name.as_ref() == name)
    }

    /// Borrow the current base identity selected for new work.
    #[must_use]
    pub const fn identity(&self) -> &[u8; 32] {
        self.current.identity()
    }
}

impl Candidate {
    pub(crate) fn bytes(&self) -> &[u8] {
        &self.0.bytes
    }

    pub(crate) fn digest(&self) -> Digest {
        self.0.digest
    }

    /// Whether this parse may serve a later consumer: it was made under the
    /// same universe definition and its exact owned bytes are the ones named by
    /// that consumer's sealed `(length, SHA-256)` commitment.
    pub(crate) fn names(&self, definition: &Definition, commitment: (u64, [u8; 32])) -> bool {
        self.0.definition.identity == definition.identity
            && (self.0.bytes.len() as u64, *self.0.digest.as_bytes()) == commitment
    }

    pub(crate) fn admit(self, metadata: &MetadataProof) -> Result<FinalLock> {
        require_universe(
            self.0.definition.identity,
            metadata.universe,
            "protected metadata proof",
        )?;
        // The project's own resolution is admitted as Cargo produced
        // it; only the framework crate the facade was built against is fixed.
        self.0
            .definition
            .protected
            .require_framework(self.0.definition.framework, &self.0.closure)?;
        Ok(FinalLock(self.0))
    }
}

impl FinalLock {
    pub(crate) fn digest(&self) -> Digest {
        self.0.digest
    }

    #[cfg(test)]
    fn bind(
        &self,
        context: ContextProof,
        metadata: &MetadataProof,
        unit: UnitProof,
        build: BuildProof,
    ) -> Result<Proof> {
        self.bind_accounted(context, metadata, unit, build, |_| Ok::<(), Error>(()))
    }

    fn bind_accounted<E: From<Error>>(
        &self,
        context: ContextProof,
        metadata: &MetadataProof,
        unit: UnitProof,
        build: BuildProof,
        admit: impl FnOnce(usize) -> std::result::Result<(), E>,
    ) -> std::result::Result<Proof, E> {
        require_universe(
            self.0.definition.identity,
            context.universe,
            "Cargo context proof",
        )?;
        require_universe(
            self.0.definition.identity,
            metadata.universe,
            "protected metadata proof",
        )?;
        require_universe(
            self.0.definition.identity,
            unit.universe,
            "protected unit proof",
        )?;
        require_universe(
            self.0.definition.identity,
            build.universe,
            "protected source/build proof",
        )?;
        const DOMAIN: &[u8] = b"rawr.import.cargo.resolution-universe-proof/v1";
        admit(DOMAIN.len() + 7 * 32)?;
        let mut complete = Sha256::new();
        complete.update(DOMAIN);
        complete.update(self.0.definition.identity.as_bytes());
        complete.update(self.0.digest.as_bytes());
        complete.update(context.digest.as_bytes());
        complete.update(metadata.digest.as_bytes());
        complete.update(unit.digest.as_bytes());
        complete.update(build.projection.as_bytes());
        complete.update(build.input.as_bytes());
        Ok(Proof {
            base: self.0.definition.identity,
            complete: Digest(complete.finalize().into()),
            final_lock: self.0.digest,
        })
    }
}

impl Receipt {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn transform(
        request: Digest,
        lock: &FinalLock,
        manifest: Digest,
        source: Digest,
        metadata: Digest,
        unit: Digest,
        build: BuildProof,
        proof: Proof,
    ) -> Self {
        Self {
            schema: RECEIPT_SCHEMA,
            request: *request.as_bytes(),
            universe: *proof.base().as_bytes(),
            manifest: *manifest.as_bytes(),
            source: *source.as_bytes(),
            lock: *lock.digest().as_bytes(),
            metadata: *metadata.as_bytes(),
            unit: *unit.as_bytes(),
            build: *build.projection.as_bytes(),
            input: *build.input.as_bytes(),
            complete: *proof.complete().as_bytes(),
        }
    }

    pub(crate) fn encode(&self) -> Result<Box<[u8]>> {
        serde_json::to_vec(self)
            .map(Vec::into_boxed_slice)
            .map_err(|error| Error::Schema {
                document: "resolution universe receipt",
                reason: error.to_string().into_boxed_str(),
            })
    }

    pub(crate) fn parse(bytes: &[u8], definition: &Definition) -> Result<Self> {
        let receipt: Self = serde_json::from_slice(bytes).map_err(|error| Error::Schema {
            document: "resolution universe receipt",
            reason: error.to_string().into_boxed_str(),
        })?;
        if receipt.schema != RECEIPT_SCHEMA || receipt.universe != *definition.identity() {
            return Err(Error::ResolutionDrift {
                command: "persisted universe preflight",
                field: "resolution receipt request/universe",
            });
        }
        if receipt.encode()?.as_ref() != bytes {
            return Err(Error::ResolutionDrift {
                command: "persisted universe preflight",
                field: "canonical resolution receipt",
            });
        }
        Ok(receipt)
    }

    pub(crate) fn universe(bytes: &[u8]) -> Result<[u8; 32]> {
        let receipt: Self = serde_json::from_slice(bytes).map_err(|error| Error::Schema {
            document: "resolution universe receipt",
            reason: error.to_string().into_boxed_str(),
        })?;
        if receipt.schema != RECEIPT_SCHEMA || receipt.encode()?.as_ref() != bytes {
            return Err(Error::ResolutionDrift {
                command: "persisted universe preflight",
                field: "canonical supported resolution receipt",
            });
        }
        Ok(receipt.universe)
    }

    pub(crate) fn matches_request(&self, request: Digest) -> bool {
        self.request == *request.as_bytes()
    }

    pub(crate) fn preflight_files(
        &self,
        manifest: Digest,
        source: Digest,
        lock: Digest,
    ) -> Result<()> {
        if self.manifest != *manifest.as_bytes()
            || self.source != *source.as_bytes()
            || self.lock != *lock.as_bytes()
        {
            return Err(Error::ResolutionDrift {
                command: "persisted universe preflight",
                field: "manifest/source/whole-lock receipt",
            });
        }
        Ok(())
    }

    pub(crate) fn revalidate(
        &self,
        lock: &FinalLock,
        metadata: Digest,
        unit: Digest,
        build: BuildProof,
        proof: Proof,
    ) -> Result<()> {
        if self.lock != *lock.digest().as_bytes()
            || self.metadata != *metadata.as_bytes()
            || self.unit != *unit.as_bytes()
            || self.build != *build.projection.as_bytes()
            || self.input != *build.input.as_bytes()
            || self.complete != *proof.complete().as_bytes()
        {
            return Err(Error::ResolutionDrift {
                command: "persisted universe validation",
                field: "lock/metadata/unit completed proof",
            });
        }
        Ok(())
    }
}

impl Pending {
    #[allow(
        clippy::too_many_arguments,
        reason = "the pending state retains every independently authenticated pre-build proof"
    )]
    pub(crate) fn transform(
        request: Digest,
        lock: FinalLock,
        context: ContextProof,
        metadata: MetadataProof,
        unit: UnitProof,
        receipt: Option<Receipt>,
    ) -> Self {
        Self {
            request,
            lock,
            context,
            metadata,
            unit,
            receipt,
        }
    }

    pub(crate) const fn metadata(&self) -> &MetadataProof {
        &self.metadata
    }

    #[allow(
        clippy::too_many_arguments,
        reason = "completion joins the exact post-build observations retained by the receipt"
    )]
    pub(crate) fn complete(
        self,
        manifest: Digest,
        source: Digest,
        metadata: Digest,
        unit: Digest,
        input: &crate::input::Baseline,
        _limits: &Limits,
    ) -> Result<Completion> {
        let projection = input.universe_build().ok_or(Error::ResolutionDrift {
            command: "resolution universe completion",
            field: "post-build protected source/build observation",
        })?;
        self.complete_observed(
            manifest,
            source,
            metadata,
            unit,
            projection,
            input.fingerprint(),
            _limits,
            |_| Ok::<(), Error>(()),
        )
    }

    #[cfg(feature = "rustc-driver")]
    #[allow(
        clippy::too_many_arguments,
        reason = "replayed inputs undergo the same exact universe completion join"
    )]
    pub(crate) fn complete_replayed(
        &self,
        manifest: Digest,
        source: Digest,
        metadata: Digest,
        unit: Digest,
        input: crate::input::stored::Compared<'_, '_, '_>,
        limits: &Limits,
    ) -> crate::input::stored::Result<(Completion, crate::input::stored::Completed)> {
        // Projection and completion use this exact pending metadata association.
        let input = input.protect(&self.metadata)?;
        input.finish(|projection, fingerprint, admit| {
            self.complete_observed(
                manifest,
                source,
                metadata,
                unit,
                projection,
                fingerprint,
                limits,
                admit,
            )
        })
    }

    #[allow(
        clippy::too_many_arguments,
        reason = "one private join preserves cold and replay completion semantics"
    )]
    fn complete_observed<E: From<Error>>(
        &self,
        manifest: Digest,
        source: Digest,
        metadata: Digest,
        unit: Digest,
        projection: &BuildProjection,
        fingerprint: crate::input::fingerprint::Fingerprint,
        _limits: &Limits,
        admit: impl FnOnce(usize) -> std::result::Result<(), E>,
    ) -> std::result::Result<Completion, E> {
        let build = BuildProof {
            universe: self.lock.0.definition.identity,
            projection: projection.digest(),
            input: Digest(*fingerprint.as_bytes()),
        };
        let proof =
            self.lock
                .bind_accounted(self.context, &self.metadata, self.unit, build, admit)?;
        let receipt = match &self.receipt {
            Some(receipt) => {
                receipt.revalidate(&self.lock, metadata, unit, build, proof)?;
                None
            }
            None => Some(Receipt::transform(
                self.request,
                &self.lock,
                manifest,
                source,
                metadata,
                unit,
                build,
                proof,
            )),
        };
        Ok(Completion { proof, receipt })
    }
}

impl Completion {
    pub(crate) const fn proof(&self) -> Proof {
        self.proof
    }

    pub(crate) fn receipt(self) -> Option<Receipt> {
        self.receipt
    }
}

impl Proof {
    pub(crate) const fn final_lock(self) -> Digest {
        self.final_lock
    }

    pub(crate) const fn base(self) -> Digest {
        self.base
    }

    pub(crate) const fn complete(self) -> Digest {
        self.complete
    }
}

impl MetadataProof {
    #[cfg(test)]
    pub(crate) fn fixture_package(
        package_id: &str,
        name: &str,
        version: &str,
        source: &str,
        root: PathBuf,
    ) -> Self {
        Self {
            universe: Digest([0; 32]),
            digest: Digest([0; 32]),
            packages: [(
                package_id.into(),
                ProtectedPackage {
                    key: Key {
                        name: name.into(),
                        version: version.into(),
                        source: source.into(),
                    },
                    root: root.into(),
                },
            )]
            .into(),
            _ownership: JsonOwnership::empty(),
        }
    }

    #[cfg(test)]
    pub(crate) fn with_fixture_package(
        mut self,
        package_id: &str,
        name: &str,
        version: &str,
        source: &str,
        root: PathBuf,
    ) -> Self {
        self.packages.insert(
            package_id.into(),
            ProtectedPackage {
                key: Key {
                    name: name.into(),
                    version: version.into(),
                    source: source.into(),
                },
                root: root.into(),
            },
        );
        self
    }

    pub(crate) fn package(&self, id: &str) -> Option<&ProtectedPackage> {
        self.packages.get(id)
    }

    pub(crate) fn normalization_roots(&self) -> impl ExactSizeIterator<Item = &ProtectedPackage> {
        self.packages.values()
    }

    pub(crate) fn package_token(&self, package_id: &str) -> Option<String> {
        self.packages
            .get(package_id)
            .map(|package| key_token(&package.key))
    }

    pub(crate) fn normalized_text(&self, value: &str) -> String {
        let mut roots = self.normalization_roots().collect::<Vec<_>>();
        ProtectedPackage::sort_roots(&mut roots);
        let mut normalized = value.to_owned();
        for package in roots {
            let Some(root) = package.root.to_str() else {
                continue;
            };
            normalized = normalized.replace(root, &format!("$PACKAGE/{}", key_token(&package.key)));
        }
        normalized
    }
}

impl ProtectedPackage {
    pub(crate) fn root(&self) -> &Path {
        &self.root
    }

    pub(crate) fn sort_roots(roots: &mut [&Self]) {
        roots.sort_unstable_by_key(|package| std::cmp::Reverse(package.root.as_os_str().len()));
    }

    pub(crate) fn token_bytes(&self) -> [u8; 64] {
        key_token_bytes(&self.key)
    }
}

fn require_universe(expected: Digest, actual: Digest, field: &'static str) -> Result<()> {
    if expected == actual {
        Ok(())
    } else {
        Err(Error::ResolutionDrift {
            command: "resolution universe proof join",
            field,
        })
    }
}

fn metadata_packages(
    metadata: &super::unit::Metadata,
    allocation: &mut JsonAllocation,
) -> Result<Budgeted<BTreeMap<Arc<str>, ProtectedPackage>>> {
    let mut ownership = JsonOwnership::collection();
    allocation.acquire(ownership)?;
    let mut by_id = BTreeMap::new();
    let outcome = (|| {
        for (id, package) in metadata.package_entries() {
            let entry = JsonOwnership::members(1);
            allocation.acquire(entry)?;
            ownership = ownership.checked_add(entry)?;
            if by_id
                .insert(
                    Arc::clone(id),
                    ProtectedPackage {
                        key: Key {
                            name: package.shared_name(),
                            version: package.shared_version(),
                            source: KeySource::from_shared(
                                package.source().shared_universe_source(),
                            ),
                        },
                        root: package.shared_root(),
                    },
                )
                .is_some()
            {
                return Err(json_schema("Cargo package id is ambiguous"));
            }
        }
        Ok(())
    })();
    match outcome {
        Ok(()) => Ok(Budgeted::from_retained(by_id, ownership)),
        Err(error) => {
            drop(by_id);
            allocation.release(ownership)?;
            Err(error)
        }
    }
}

#[cfg(test)]
fn metadata_packages_from_value(
    value: &JsonValue,
    allocation: &mut JsonAllocation,
) -> Result<Budgeted<BTreeMap<Arc<str>, ProtectedPackage>>> {
    let object = json_object(value, "protected Cargo metadata root")?;
    let packages = json_array(object.get("packages"), "metadata packages")?;
    let mut ownership = JsonOwnership::collection();
    allocation.acquire(ownership)?;
    let mut by_id = BTreeMap::new();
    let outcome = (|| {
        for package in packages {
            let package = json_object(package, "metadata package")?;
            let id = json_bounded_str(package, "id")?;
            let manifest = json_bounded_str(package, "manifest_path")?;
            let root = Path::new(manifest)
                .parent()
                .ok_or_else(|| json_schema("Cargo manifest path has no parent"))?;
            let name = json_bounded_str(package, "name")?;
            let version = json_bounded_str(package, "version")?;
            let source = match package.get("source") {
                Some(JsonValue::Null) | None => "",
                Some(JsonValue::String(source)) => source.as_str(),
                Some(_) => return Err(json_schema("metadata package source is invalid")),
            };
            let mut entry = JsonOwnership::members(1)
                .checked_add(JsonOwnership::string(id.len()))?
                .checked_add(JsonOwnership::string(name.len()))?
                .checked_add(JsonOwnership::string(version.len()))?
                .checked_add(JsonOwnership::string(
                    root.as_os_str().as_encoded_bytes().len(),
                ))?;
            if !source.is_empty() {
                entry = entry.checked_add(JsonOwnership::string(source.len()))?;
            }
            allocation.acquire(entry)?;
            ownership = ownership.checked_add(entry)?;
            if by_id
                .insert(
                    id.into(),
                    ProtectedPackage {
                        key: Key {
                            name: name.into(),
                            version: version.into(),
                            source: source.into(),
                        },
                        root: root.into(),
                    },
                )
                .is_some()
            {
                return Err(json_schema("Cargo package id is ambiguous"));
            }
        }
        Ok(())
    })();
    match outcome {
        Ok(()) => Ok(Budgeted::from_retained(by_id, ownership)),
        Err(error) => {
            drop(by_id);
            allocation.release(ownership)?;
            Err(error)
        }
    }
}

fn metadata_observation(
    value: Budgeted<JsonValue>,
    records: Option<super::unit::MetadataRecords>,
    all_packages: &BTreeMap<Arc<str>, ProtectedPackage>,
    limits: &Limits,
    allocation: &mut JsonAllocation,
) -> Result<Budgeted<BTreeMap<Arc<str>, ProjectedPackage>>> {
    let document_ownership = value.ownership();
    let (mut value, _) = value.into_parts();
    let object = json_object_mut(&mut value, "protected Cargo metadata root")?;
    let mut packages_value = object
        .remove("packages")
        .ok_or_else(|| json_schema("metadata packages are absent"))?;
    let mut resolve = object
        .remove("resolve")
        .ok_or_else(|| json_schema("metadata resolve is absent"))?;
    let mut nodes_value = json_object_mut(&mut resolve, "metadata resolve")?
        .remove("nodes")
        .ok_or_else(|| json_schema("metadata resolve nodes are absent"))?;
    let packages = packages_value
        .as_array_mut()
        .ok_or_else(|| json_schema("metadata packages are invalid"))?;
    let nodes = nodes_value
        .as_array_mut()
        .ok_or_else(|| json_schema("metadata resolve nodes are invalid"))?;
    let mut ownership = JsonOwnership::collection();
    allocation.acquire(ownership)?;
    let mut projected = BTreeMap::<Arc<str>, ProjectedPackage>::new();
    let mut moved_ownership = JsonOwnership::empty();
    let mut edges = 0_usize;
    let mut record_structure = JsonOwnership::empty();
    let mut records = match records {
        Some(records) => {
            if records.value().len() != packages.len() {
                return Err(json_schema(
                    "complete metadata package record count differs",
                ));
            }
            record_structure = records.ownership();
            for record in records.value() {
                record_structure = record_structure.checked_sub(record.ownership())?;
            }
            Some(records.into_parts().0.into_iter())
        }
        None => None,
    };
    for package in std::mem::take(packages) {
        let package_ownership = JsonOwnership::tree(&package)?;
        moved_ownership = moved_ownership.checked_add(package_ownership)?;
        let mut package = Budgeted::from_retained(package, package_ownership);
        if let Some(records) = records.as_mut() {
            let record = records
                .next()
                .ok_or_else(|| json_schema("complete metadata package record is absent"))?;
            let decoded = super::unit::parse_budgeted_json(
                "complete Cargo metadata package",
                record.value(),
                limits,
                allocation,
            )?;
            if decoded.value().get("id") != package.value().get("id") {
                return Err(json_schema(
                    "complete metadata package record identity differs",
                ));
            }
            package.drop_from(allocation)?;
            package = decoded;
            record.drop_from(allocation)?;
        }
        let shared_id = {
            let id = json_bounded_str(json_object(package.value(), "metadata package")?, "id")?;
            all_packages
                .get_key_value(id)
                .map(|(id, _)| Arc::clone(id))
                .ok_or_else(|| json_schema("metadata observation package is absent"))?
        };
        let entry = JsonOwnership::members(1);
        allocation.acquire(entry)?;
        normalize_metadata_package(&mut package, shared_id.as_ref(), all_packages, allocation)?;
        let package = projection_bytes_budgeted(package, allocation)?;
        ownership = ownership
            .checked_add(entry)?
            .checked_add(package.ownership())?;
        if projected
            .insert(
                shared_id,
                ProjectedPackage {
                    package: package.into_parts().0,
                    resolve: None,
                },
            )
            .is_some()
        {
            return Err(json_schema("metadata observation package is ambiguous"));
        }
    }
    drop(records);
    allocation.release(record_structure)?;
    for node in std::mem::take(nodes) {
        let node_ownership = JsonOwnership::tree(&node)?;
        moved_ownership = moved_ownership.checked_add(node_ownership)?;
        let id = json_bounded_str(json_object(&node, "metadata resolve node")?, "id")?;
        let entry = projected
            .get_mut(id)
            .ok_or_else(|| json_schema("metadata observation resolve package is absent"))?;
        if entry.resolve.is_some() {
            return Err(json_schema(
                "metadata observation resolve node is ambiguous",
            ));
        }
        let mut node = Budgeted::from_retained(node, node_ownership);
        edges = edges.saturating_add(normalize_resolve_node(&mut node, all_packages, allocation)?);
        let node = projection_bytes_budgeted(node, allocation)?;
        ownership = ownership.checked_add(node.ownership())?;
        entry.resolve = Some(node.into_parts().0);
    }
    if projected.len() != all_packages.len()
        || projected.values().any(|entry| entry.resolve.is_none())
    {
        return Err(json_schema(
            "metadata observation package/node set is incomplete",
        ));
    }

    let discarded = document_ownership.checked_sub(moved_ownership)?;
    drop(value);
    drop(packages_value);
    drop(nodes_value);
    drop(resolve);
    allocation.release(discarded)?;
    Ok(Budgeted::from_retained(projected, ownership))
}

fn metadata_projection_from_observation(
    observed: Budgeted<BTreeMap<Arc<str>, ProjectedPackage>>,
    all_packages: &BTreeMap<Arc<str>, ProtectedPackage>,
    reachable: &BTreeSet<Arc<str>>,
    limits: &Limits,
    allocation: &mut JsonAllocation,
) -> Result<MetadataProjection> {
    let mut protected_ownership = JsonOwnership::collection();
    allocation.acquire(protected_ownership)?;
    let mut protected = BTreeMap::new();
    for id in reachable {
        let (shared_id, package) = all_packages
            .get_key_value(id.as_ref())
            .ok_or_else(|| json_schema("unit-reachable metadata package is absent"))?;
        let entry = JsonOwnership::members(1);
        allocation.acquire(entry)?;
        protected_ownership = protected_ownership.checked_add(entry)?;
        protected.insert(Arc::clone(shared_id), package.clone());
    }
    let protected = Budgeted::from_retained(protected, protected_ownership);

    let token_ownership =
        JsonOwnership::collection().checked_add(JsonOwnership::nodes(protected.value().len()))?;
    allocation.acquire(token_ownership)?;
    let tokens = protected
        .value()
        .values()
        .map(|package| key_token_bytes(&package.key))
        .collect::<BTreeSet<_>>();
    let (mut observed, observed_ownership) = observed.into_parts();
    let parts_structure =
        JsonOwnership::collection().checked_add(JsonOwnership::nodes(protected.value().len()))?;
    allocation.acquire(parts_structure)?;
    let mut projected = Vec::<Budgeted<Box<[u8]>>>::new();
    projected
        .try_reserve_exact(protected.value().len())
        .map_err(|source| Error::Allocation {
            operation: "protected Cargo projection",
            source,
        })?;
    let mut released_buffers = JsonOwnership::empty();
    let mut edges = 0_usize;
    for (id, selected) in protected.value() {
        let ProjectedPackage {
            package: package_bytes,
            resolve: resolve_bytes,
        } = observed
            .remove(id.as_ref())
            .ok_or_else(|| json_schema("unit-reachable metadata observation is absent"))?;
        let package_buffer = JsonOwnership::buffer(package_bytes.len());
        let package = super::unit::parse_budgeted_json(
            "normalized protected metadata package",
            &package_bytes,
            limits,
            allocation,
        )?;
        drop(package_bytes);
        allocation.release(package_buffer)?;
        released_buffers = released_buffers.checked_add(package_buffer)?;

        let resolve_bytes = resolve_bytes
            .ok_or_else(|| json_schema("normalized metadata resolve row is absent"))?;
        let resolve_buffer = JsonOwnership::buffer(resolve_bytes.len());
        let mut resolve = super::unit::parse_budgeted_json(
            "normalized protected metadata resolve node",
            &resolve_bytes,
            limits,
            allocation,
        )?;
        drop(resolve_bytes);
        allocation.release(resolve_buffer)?;
        released_buffers = released_buffers.checked_add(resolve_buffer)?;
        edges = edges.saturating_add(filter_normalized_resolve_budgeted(
            &mut resolve,
            &tokens,
            allocation,
        )?);
        let package = projection_bytes_budgeted(package, allocation)?;
        let resolve = projection_bytes_budgeted(resolve, allocation)?;
        const ENTRY_PREFIX: &[u8] = b"{\"key\":\"";
        const ENTRY_PACKAGE: &[u8] = b"\",\"package\":";
        const ENTRY_RESOLVE: &[u8] = b",\"resolve\":";
        const ENTRY_SUFFIX: &[u8] = b"}";
        let total = ENTRY_PREFIX
            .len()
            .checked_add(64)
            .and_then(|total| total.checked_add(ENTRY_PACKAGE.len()))
            .and_then(|total| total.checked_add(package.value().len()))
            .and_then(|total| total.checked_add(ENTRY_RESOLVE.len()))
            .and_then(|total| total.checked_add(resolve.value().len()))
            .and_then(|total| total.checked_add(ENTRY_SUFFIX.len()))
            .ok_or_else(|| json_schema("protected metadata entry byte count overflow"))?;
        let token = key_token_bytes(&selected.key);
        let entry = allocation.retain_with(JsonOwnership::buffer(total), || {
            let mut bytes = Vec::new();
            bytes
                .try_reserve_exact(total)
                .map_err(|source| Error::Allocation {
                    operation: "protected Cargo projection",
                    source,
                })?;
            bytes.extend_from_slice(ENTRY_PREFIX);
            bytes.extend_from_slice(&token);
            bytes.extend_from_slice(ENTRY_PACKAGE);
            bytes.extend_from_slice(package.value());
            bytes.extend_from_slice(ENTRY_RESOLVE);
            bytes.extend_from_slice(resolve.value());
            bytes.extend_from_slice(ENTRY_SUFFIX);
            if bytes.len() != total {
                return Err(json_schema(
                    "protected metadata entry byte count changed during assembly",
                ));
            }
            Ok(bytes.into_boxed_slice())
        })?;
        package.drop_from(allocation)?;
        resolve.drop_from(allocation)?;
        projected.push(entry);
    }
    drop(observed);
    let dropped_ownership = observed_ownership.checked_sub(released_buffers)?;
    allocation.release(dropped_ownership)?;
    drop(tokens);
    allocation.release(token_ownership)?;

    const PREFIX: &[u8] = b"{\"packages\":[";
    const SUFFIX: &[u8] = b"]}";
    let entry_bytes = projected.iter().try_fold(0_usize, |total, entry| {
        total
            .checked_add(entry.value().len())
            .ok_or_else(|| json_schema("protected metadata projection byte count overflow"))
    })?;
    let total = PREFIX
        .len()
        .checked_add(entry_bytes)
        .and_then(|total| total.checked_add(projected.len().saturating_sub(1)))
        .and_then(|total| total.checked_add(SUFFIX.len()))
        .ok_or_else(|| json_schema("protected metadata projection byte count overflow"))?;
    let output = allocation.retain_with(JsonOwnership::buffer(total), || {
        let mut bytes = Vec::new();
        bytes
            .try_reserve_exact(total)
            .map_err(|source| Error::Allocation {
                operation: "protected Cargo projection",
                source,
            })?;
        bytes.extend_from_slice(PREFIX);
        for (index, entry) in projected.iter().enumerate() {
            if index != 0 {
                bytes.push(b',');
            }
            bytes.extend_from_slice(entry.value());
        }
        bytes.extend_from_slice(SUFFIX);
        if bytes.len() != total {
            return Err(json_schema(
                "protected metadata projection byte count changed during assembly",
            ));
        }
        Ok(bytes.into_boxed_slice())
    })?;
    for entry in projected {
        entry.drop_from(allocation)?;
    }
    allocation.release(parts_structure)?;
    Ok((output, protected))
}

fn filter_normalized_resolve_budgeted(
    value: &mut Budgeted<JsonValue>,
    protected: &BTreeSet<[u8; 64]>,
    allocation: &mut JsonAllocation,
) -> Result<usize> {
    value.transform(|value, ownership| {
        let object = json_object_mut(value, "normalized protected metadata resolve node")?;
        let mut edges = 0_usize;
        if let Some(dependencies) = object.get_mut("dependencies") {
            let dependencies = dependencies
                .as_array_mut()
                .ok_or_else(|| json_schema("normalized resolve dependencies are invalid"))?;
            release_filtered_array_entries(
                dependencies,
                |dependency| {
                    dependency
                        .as_str()
                        .and_then(|token| <&[u8; 64]>::try_from(token.as_bytes()).ok())
                        .is_some_and(|token| protected.contains(token))
                },
                allocation,
                ownership,
            )?;
            edges = edges.saturating_add(dependencies.len());
        }
        if let Some(dependencies) = object.get_mut("deps") {
            let dependencies = dependencies
                .as_array_mut()
                .ok_or_else(|| json_schema("normalized resolve dependency records are invalid"))?;
            release_filtered_array_entries(
                dependencies,
                |dependency| {
                    dependency
                        .get("pkg")
                        .and_then(JsonValue::as_str)
                        .and_then(|token| <&[u8; 64]>::try_from(token.as_bytes()).ok())
                        .is_some_and(|token| protected.contains(token))
                },
                allocation,
                ownership,
            )?;
            edges = edges.saturating_add(dependencies.len());
        }
        Ok(edges)
    })
}

struct PendingUnits {
    pending: Vec<usize>,
    discovered: BTreeSet<usize>,
    bound: usize,
    #[cfg(test)]
    peak: usize,
}

impl PendingUnits {
    fn retain(bound: usize, allocation: &mut JsonAllocation) -> Result<Budgeted<Self>> {
        let ownership = JsonOwnership::collection()
            .checked_add(JsonOwnership::nodes(bound))?
            .checked_add(JsonOwnership::collection())?;
        allocation.retain_with(ownership, || {
            let mut pending = Vec::new();
            pending
                .try_reserve_exact(bound)
                .map_err(|source| Error::Allocation {
                    operation: "protected Cargo projection",
                    source,
                })?;
            debug_assert!(pending.capacity() >= bound);
            Ok(Self {
                pending,
                discovered: BTreeSet::new(),
                bound,
                #[cfg(test)]
                peak: 0,
            })
        })
    }

    fn enqueue(
        queue: &mut Budgeted<Self>,
        index: usize,
        allocation: &mut JsonAllocation,
    ) -> Result<bool> {
        queue.transform(|queue, ownership| {
            if index >= queue.bound {
                return Err(json_schema("Cargo unit dependency index is invalid"));
            }
            if queue.discovered.contains(&index) {
                return Ok(false);
            }
            if queue.pending.len() >= queue.bound {
                return Err(json_schema("Cargo unit pending queue exceeded its bound"));
            }
            debug_assert!(queue.pending.len() < queue.pending.capacity());
            let discovered = JsonOwnership::nodes(1);
            allocation.acquire(discovered)?;
            let inserted = queue.discovered.insert(index);
            debug_assert!(inserted, "the discovery check precedes the only insertion");
            queue.pending.push(index);
            #[cfg(test)]
            {
                queue.peak = queue.peak.max(queue.pending.len());
            }
            *ownership = ownership.checked_add(discovered)?;
            Ok(true)
        })
    }

    fn pop(queue: &mut Budgeted<Self>) -> Option<usize> {
        queue.value_mut().pending.pop()
    }

    fn is_empty(queue: &Budgeted<Self>) -> bool {
        queue.value().pending.is_empty()
    }

    #[cfg(test)]
    fn peak(queue: &Budgeted<Self>) -> usize {
        queue.value().peak
    }

    fn finish(
        queue: Budgeted<Self>,
        allocation: &mut JsonAllocation,
    ) -> Result<Budgeted<BTreeSet<usize>>> {
        let (queue, ownership) = queue.into_parts();
        let discovered_ownership = JsonOwnership::collection()
            .checked_add(JsonOwnership::nodes(queue.discovered.len()))?;
        let pending_ownership = ownership.checked_sub(discovered_ownership)?;
        drop(queue.pending);
        allocation.release(pending_ownership)?;
        Ok(Budgeted::from_retained(
            queue.discovered,
            discovered_ownership,
        ))
    }
}

#[cfg(test)]
fn metadata_projection(
    value: &JsonValue,
    all_packages: &BTreeMap<Arc<str>, ProtectedPackage>,
    reachable: &BTreeSet<Arc<str>>,
) -> Result<(JsonValue, BTreeMap<Arc<str>, ProtectedPackage>)> {
    let mut allocation = JsonAllocation::empty();
    let object = json_object(value, "protected Cargo metadata root")?;
    let packages = json_array(object.get("packages"), "metadata packages")?;
    let resolve = json_object(
        object
            .get("resolve")
            .ok_or_else(|| json_schema("metadata resolve is absent"))?,
        "metadata resolve",
    )?;
    let nodes = json_array(resolve.get("nodes"), "metadata resolve nodes")?;
    let protected = reachable
        .iter()
        .map(|id| {
            all_packages
                .get(id)
                .cloned()
                .map(|package| (id.clone(), package))
                .ok_or_else(|| json_schema("unit-reachable metadata package is absent"))
        })
        .collect::<Result<BTreeMap<_, _>>>()?;
    let packages_by_id = packages
        .iter()
        .map(|package| {
            let object = json_object(package, "metadata package")?;
            Ok((json_bounded_string(object, "id")?, package))
        })
        .collect::<Result<BTreeMap<_, _>>>()?;
    let nodes_by_id = nodes
        .iter()
        .map(|node| {
            let object = json_object(node, "metadata resolve node")?;
            let id = json_bounded_string(object, "id")?;
            Ok((id, node))
        })
        .collect::<Result<BTreeMap<_, _>>>()?;

    let mut projected = Vec::new();
    projected
        .try_reserve_exact(protected.len())
        .map_err(|source| Error::Allocation {
            operation: "protected Cargo projection",
            source,
        })?;
    let mut edge_count = 0_usize;
    for (id, selected) in &protected {
        let package = (*packages_by_id
            .get(id.as_ref())
            .ok_or_else(|| json_schema("unit-reachable package document is absent"))?)
        .clone();
        let package_ownership = JsonOwnership::tree(&package)?;
        let mut package = allocation.retain_with(package_ownership, || Ok(package))?;
        normalize_metadata_package(&mut package, id, all_packages, &mut allocation)?;
        let node = nodes_by_id
            .get(id.as_ref())
            .ok_or_else(|| json_schema("protected resolve node is absent"))?;
        let node = (*node).clone();
        let node_ownership = JsonOwnership::tree(&node)?;
        let mut node = allocation.retain_with(node_ownership, || Ok(node))?;
        edge_count = edge_count.saturating_add(normalize_resolve_node(
            &mut node,
            &protected,
            &mut allocation,
        )?);
        let (package, _) = package.into_parts();
        let (node, _) = node.into_parts();
        projected.push(serde_json::json!({
            "key": key_token(&selected.key),
            "package": package,
            "resolve": node,
        }));
    }
    Ok((serde_json::json!({"packages": projected}), protected))
}

fn rewrite_json_string_from_bytes(
    value: &mut JsonValue,
    new_bytes: usize,
    allocation: &mut JsonAllocation,
    ownership: &mut JsonOwnership,
    build: impl FnOnce(&str) -> Result<Vec<u8>>,
) -> Result<()> {
    let previous = value
        .as_str()
        .ok_or_else(|| json_schema("protected replacement value is not a string"))?;
    let old_bytes = previous.len();
    let old = JsonOwnership::string_bytes(old_bytes);
    let new = JsonOwnership::string_bytes(new_bytes);
    let next_ownership = ownership.checked_sub(old)?.checked_add(new)?;
    let buffer = JsonOwnership::buffer(new_bytes);
    allocation.acquire(buffer)?;
    let replacement_bytes = match build(previous) {
        Ok(replacement) if replacement.len() == new_bytes => replacement,
        Ok(_) => {
            allocation.release(buffer)?;
            return Err(json_schema("protected replacement string length drifted"));
        }
        Err(error) => {
            allocation.release(buffer)?;
            return Err(error);
        }
    };
    let replacement = match std::str::from_utf8(&replacement_bytes) {
        Ok(replacement) => replacement,
        Err(error) => {
            allocation.release(buffer)?;
            return Err(json_schema(&error.to_string()));
        }
    };
    let growth = JsonOwnership::string_bytes(new_bytes.saturating_sub(old_bytes));
    if let Err(error) = allocation.acquire(growth) {
        allocation.release(buffer)?;
        return Err(error);
    }
    let JsonValue::String(string) = value else {
        allocation.release(growth)?;
        allocation.release(buffer)?;
        return Err(json_schema("protected replacement value changed shape"));
    };
    if string.capacity() < new_bytes
        && let Err(source) = string.try_reserve_exact(new_bytes.saturating_sub(string.len()))
    {
        allocation.release(growth)?;
        allocation.release(buffer)?;
        return Err(Error::Allocation {
            operation: "protected Cargo projection string growth",
            source,
        });
    }
    string.clear();
    string.push_str(replacement);
    drop(replacement_bytes);
    allocation.release(JsonOwnership::string_bytes(
        old_bytes.saturating_sub(new_bytes),
    ))?;
    allocation.release(buffer)?;
    *ownership = next_ownership;
    Ok(())
}

fn replace_json_key_token(
    value: &mut JsonValue,
    key: &Key,
    allocation: &mut JsonAllocation,
    ownership: &mut JsonOwnership,
) -> Result<()> {
    const TOKEN_BYTES: usize = 64;

    let old_bytes = value
        .as_str()
        .ok_or_else(|| json_schema("protected token value is not a string"))?
        .len();
    let old = JsonOwnership::string_bytes(old_bytes);
    let new = JsonOwnership::string_bytes(TOKEN_BYTES);
    let next_ownership = ownership.checked_sub(old)?.checked_add(new)?;
    let growth = JsonOwnership::string_bytes(TOKEN_BYTES.saturating_sub(old_bytes));
    allocation.acquire(growth)?;

    let token = key_token_bytes(key);
    let JsonValue::String(string) = value else {
        allocation.release(growth)?;
        return Err(json_schema("protected token value changed shape"));
    };
    if string.capacity() < TOKEN_BYTES
        && let Err(source) = string.try_reserve_exact(TOKEN_BYTES.saturating_sub(string.len()))
    {
        allocation.release(growth)?;
        return Err(Error::Allocation {
            operation: "protected Cargo projection token",
            source,
        });
    }
    string.clear();
    for byte in token {
        string.push(char::from(byte));
    }
    debug_assert_eq!(string.len(), TOKEN_BYTES);

    allocation.release(JsonOwnership::string_bytes(
        old_bytes.saturating_sub(TOKEN_BYTES),
    ))?;
    *ownership = next_ownership;
    Ok(())
}

fn replace_object_key_token(
    object: &mut JsonMap<String, JsonValue>,
    field: &'static str,
    key: &Key,
    allocation: &mut JsonAllocation,
    ownership: &mut JsonOwnership,
) -> Result<()> {
    let value = object
        .get_mut(field)
        .ok_or_else(|| json_schema(&format!("protected string `{field}` is absent")))?;
    replace_json_key_token(value, key, allocation, ownership)
}

fn insert_object_string(
    object: &mut JsonMap<String, JsonValue>,
    field: &'static str,
    value_bytes: usize,
    allocation: &mut JsonAllocation,
    ownership: &mut JsonOwnership,
    build: impl FnOnce() -> Result<String>,
) -> Result<()> {
    if object.contains_key(field) {
        return Err(json_schema(&format!(
            "protected string `{field}` is already present"
        )));
    }
    let acquired = JsonOwnership::object_string_field(field, value_bytes)?;
    allocation.acquire(acquired)?;
    let value = match build() {
        Ok(value) if value.len() == value_bytes => value,
        Ok(_) => {
            allocation.release(acquired)?;
            return Err(json_schema("protected inserted string length drifted"));
        }
        Err(error) => {
            allocation.release(acquired)?;
            return Err(error);
        }
    };
    object.insert(field.into(), JsonValue::String(value));
    *ownership = ownership.checked_add(acquired)?;
    Ok(())
}

fn remove_object_field(
    object: &mut JsonMap<String, JsonValue>,
    field: &'static str,
    allocation: &mut JsonAllocation,
    ownership: &mut JsonOwnership,
) -> Result<JsonValue> {
    let removed = object
        .remove(field)
        .ok_or_else(|| json_schema(&format!("protected field `{field}` is absent")))?;
    let released = JsonOwnership::object_field(field, &removed)?;
    allocation.release(released)?;
    *ownership = ownership.checked_sub(released)?;
    Ok(removed)
}

fn release_filtered_array_entries(
    values: &mut Vec<JsonValue>,
    mut retain: impl FnMut(&JsonValue) -> bool,
    allocation: &mut JsonAllocation,
    ownership: &mut JsonOwnership,
) -> Result<()> {
    let released = values.iter().filter(|value| !retain(value)).try_fold(
        JsonOwnership::empty(),
        |released, value| {
            released
                .checked_add(JsonOwnership::nodes(1))?
                .checked_add(JsonOwnership::tree(value)?)
        },
    )?;
    values.retain(retain);
    allocation.release(released)?;
    *ownership = ownership.checked_sub(released)?;
    Ok(())
}

fn normalize_metadata_package(
    value: &mut Budgeted<JsonValue>,
    id: &str,
    packages: &BTreeMap<Arc<str>, ProtectedPackage>,
    allocation: &mut JsonAllocation,
) -> Result<()> {
    let package = packages
        .get(id)
        .ok_or_else(|| json_schema("protected metadata package id is absent"))?;
    value.transform(|value, ownership| {
        let object = json_object_mut(value, "protected metadata package")?;
        replace_object_key_token(object, "id", &package.key, allocation, ownership)?;
        normalize_path_field(
            object,
            "manifest_path",
            package,
            packages,
            allocation,
            ownership,
        )?;
        normalize_optional_path_field(
            object,
            "license_file",
            package,
            packages,
            allocation,
            ownership,
        )?;
        normalize_optional_path_field(object, "readme", package, packages, allocation, ownership)?;
        if let Some(dependencies) = object.get_mut("dependencies") {
            let dependencies = dependencies
                .as_array_mut()
                .ok_or_else(|| json_schema("metadata package dependencies are invalid"))?;
            for dependency in dependencies {
                let dependency = json_object_mut(dependency, "metadata package dependency")?;
                normalize_optional_path_field(
                    dependency, "path", package, packages, allocation, ownership,
                )?;
            }
        }
        if let Some(targets) = object.get_mut("targets") {
            let targets = targets
                .as_array_mut()
                .ok_or_else(|| json_schema("metadata package targets are invalid"))?;
            for target in targets {
                normalize_path_field(
                    json_object_mut(target, "metadata package target")?,
                    "src_path",
                    package,
                    packages,
                    allocation,
                    ownership,
                )?;
            }
        }
        Ok(())
    })
}

fn normalize_resolve_node(
    value: &mut Budgeted<JsonValue>,
    packages: &BTreeMap<Arc<str>, ProtectedPackage>,
    allocation: &mut JsonAllocation,
) -> Result<usize> {
    value.transform(|value, ownership| {
        let object = json_object_mut(value, "protected metadata resolve node")?;
        let id = json_bounded_string(object, "id")?;
        let package = packages
            .get(id.as_ref())
            .ok_or_else(|| json_schema("protected resolve package id is absent"))?;
        replace_object_key_token(object, "id", &package.key, allocation, ownership)?;
        let mut edges = 0_usize;
        if let Some(dependencies) = object.get_mut("dependencies") {
            let dependencies = dependencies
                .as_array_mut()
                .ok_or_else(|| json_schema("resolve dependencies are invalid"))?;
            release_filtered_array_entries(
                dependencies,
                |dependency| {
                    dependency
                        .as_str()
                        .is_some_and(|id| packages.contains_key(id))
                },
                allocation,
                ownership,
            )?;
            edges = edges.saturating_add(dependencies.len());
            for dependency in dependencies {
                let id = dependency
                    .as_str()
                    .ok_or_else(|| json_schema("resolve dependency id is invalid"))?;
                let package = packages
                    .get(id)
                    .ok_or_else(|| json_schema("protected resolve edge escaped the cohort"))?;
                replace_json_key_token(dependency, &package.key, allocation, ownership)?;
            }
        }
        if let Some(dependencies) = object.get_mut("deps") {
            let dependencies = dependencies
                .as_array_mut()
                .ok_or_else(|| json_schema("resolve dependency records are invalid"))?;
            release_filtered_array_entries(
                dependencies,
                |dependency| {
                    dependency
                        .get("pkg")
                        .and_then(JsonValue::as_str)
                        .is_some_and(|id| packages.contains_key(id))
                },
                allocation,
                ownership,
            )?;
            edges = edges.saturating_add(dependencies.len());
            for dependency in dependencies {
                let dependency = json_object_mut(dependency, "resolve dependency record")?;
                let id = json_bounded_string(dependency, "pkg")?;
                let package = packages
                    .get(id.as_ref())
                    .ok_or_else(|| json_schema("protected resolve record escaped the cohort"))?;
                replace_object_key_token(dependency, "pkg", &package.key, allocation, ownership)?;
            }
        }
        Ok(edges)
    })
}

#[cfg(test)]
fn unit_reachability(
    value: &JsonValue,
    packages: &BTreeMap<Arc<str>, ProtectedPackage>,
    allocation: &mut JsonAllocation,
) -> Result<Reachability> {
    unit_reachability_rooted(value, ROOT_PACKAGE, false, packages, allocation)
}

/// The units and packages reachable from `root`'s lib unit. With
/// `unbuilt`, a graph without that unit governs nothing: the plain
/// universe's root, the `rawr` crate, is a build dependency of the harness,
/// which builds no unit of it (D609.2).
fn unit_reachability_rooted(
    value: &JsonValue,
    root: &str,
    unbuilt: bool,
    packages: &BTreeMap<Arc<str>, ProtectedPackage>,
    allocation: &mut JsonAllocation,
) -> Result<Reachability> {
    let object = json_object(value, "protected Cargo unit root")?;
    let units = json_array(object.get("units"), "Cargo units")?;
    let mut pending = PendingUnits::retain(units.len(), allocation)?;
    for (index, unit) in units.iter().enumerate() {
        let unit_object = json_object(unit, "Cargo unit")?;
        let id = json_bounded_str(unit_object, "pkg_id")?;
        let package = packages
            .get(id)
            .ok_or_else(|| json_schema("Cargo unit package is absent from metadata"))?;
        let mode = json_bounded_str(unit_object, "mode")?;
        let target = json_object(
            unit_object
                .get("target")
                .ok_or_else(|| json_schema("Cargo unit target is absent"))?,
            "Cargo unit target",
        )?;
        let kinds = json_array(target.get("kind"), "Cargo unit target kinds")?;
        if package.key.name.as_ref() == root
            && package.key.source.is_empty()
            && matches!(mode, "build" | "check")
            && kinds.iter().any(|kind| {
                kind.as_str()
                    .is_some_and(|kind| matches!(kind, "lib" | "rlib"))
            })
        {
            PendingUnits::enqueue(&mut pending, index, allocation)?;
        }
    }
    if PendingUnits::is_empty(&pending) && !unbuilt {
        return Err(json_schema("the governed root's target unit is absent"));
    }
    let mut edges = 0_usize;
    while let Some(index) = PendingUnits::pop(&mut pending) {
        let dependencies = json_object(&units[index], "Cargo unit")?
            .get("dependencies")
            .and_then(JsonValue::as_array)
            .ok_or_else(|| json_schema("Cargo unit dependencies are absent"))?;
        edges = edges.saturating_add(dependencies.len());
        for dependency in dependencies {
            let target = dependency
                .get("index")
                .and_then(JsonValue::as_u64)
                .and_then(|index| usize::try_from(index).ok())
                .ok_or_else(|| json_schema("Cargo unit dependency index is invalid"))?;
            PendingUnits::enqueue(&mut pending, target, allocation)?;
        }
    }
    let reachable_units = PendingUnits::finish(pending, allocation)?;

    let mut reachable_ownership = JsonOwnership::collection();
    allocation.acquire(reachable_ownership)?;
    let mut reachable_packages = BTreeSet::new();
    for index in reachable_units.value() {
        let unit = units
            .get(*index)
            .ok_or_else(|| json_schema("reachable Cargo unit index is invalid"))?;
        let id = json_bounded_str(json_object(unit, "Cargo unit")?, "pkg_id")?;
        if !reachable_packages.contains(id) {
            let (shared_id, _) = packages
                .get_key_value(id)
                .ok_or_else(|| json_schema("reachable Cargo unit package is absent"))?;
            let entry = JsonOwnership::nodes(1);
            allocation.acquire(entry)?;
            reachable_ownership = reachable_ownership.checked_add(entry)?;
            reachable_packages.insert(Arc::clone(shared_id));
        }
    }
    Ok((
        reachable_units,
        Budgeted::from_retained(reachable_packages, reachable_ownership),
    ))
}

fn unit_projection(
    value: &JsonValue,
    packages: &BTreeMap<Arc<str>, ProtectedPackage>,
    reachable_units: &BTreeSet<usize>,
    allocation: &mut JsonAllocation,
) -> Result<Budgeted<Box<[u8]>>> {
    allocation.transform_owned(JsonOwnership::default(), |allocation| {
        unit_projection_owned(value, packages, reachable_units, allocation)
    })
}

fn unit_projection_owned(
    value: &JsonValue,
    packages: &BTreeMap<Arc<str>, ProtectedPackage>,
    reachable_units: &BTreeSet<usize>,
    allocation: &mut JsonAllocation,
) -> Result<Budgeted<Box<[u8]>>> {
    let object = json_object(value, "protected Cargo unit root")?;
    let units = json_array(object.get("units"), "Cargo units")?;
    let vector_scratch = JsonOwnership::collection()
        .checked_add(JsonOwnership::collection())?
        .checked_add(JsonOwnership::collection())?
        .checked_add(JsonOwnership::nodes(units.len().saturating_mul(2)))?;
    allocation.acquire(vector_scratch)?;
    let mut scratch = vector_scratch;
    let mut unit_packages = Vec::new();
    let mut unit_keys = Vec::new();
    unit_packages
        .try_reserve_exact(units.len())
        .map_err(|source| Error::Allocation {
            operation: "protected Cargo projection",
            source,
        })?;
    unit_keys
        .try_reserve_exact(units.len())
        .map_err(|source| Error::Allocation {
            operation: "protected Cargo projection",
            source,
        })?;
    let mut protected_keys = BTreeSet::new();
    for (index, unit) in units.iter().enumerate() {
        let unit_object = json_object(unit, "Cargo unit")?;
        let id = json_bounded_str(unit_object, "pkg_id")?;
        let package = packages
            .get(id)
            .filter(|_| reachable_units.contains(&index));
        unit_packages.push(package);
        unit_keys.push(None);
        let Some(package) = package else {
            continue;
        };
        let mut normalized = allocation.clone_tree(unit)?;
        if let Some(removed) = json_object_mut(normalized.value_mut(), "protected Cargo unit key")?
            .remove("dependencies")
        {
            let removed = JsonOwnership::tree(&removed)?
                .checked_add(JsonOwnership::members(1))?
                .checked_add(JsonOwnership::object_key("dependencies"))?;
            allocation.release(removed)?;
            normalized.transform(|_, ownership| {
                *ownership = ownership.checked_sub(removed)?;
                Ok(())
            })?;
        }
        normalize_unit_base(&mut normalized, package, packages, allocation)?;
        let digest = projection_digest_budgeted(
            b"rawr.import.cargo.protected-unit-key/v1",
            normalized,
            allocation,
        )?;
        unit_keys[index] = Some(digest);
    }
    // A unit's key names its dependencies' keys, as Cargo's `-C metadata`
    // does, so a crate over two variants of one dependency (a host and a
    // target build over two `libc` feature sets) is two units without
    // `--target`.
    let mut local_keys = unit_keys.clone();
    for index in super::unit::dependency_order(units)? {
        let Some(local) = local_keys[index].take() else {
            continue;
        };
        let mut bytes = local.as_bytes().to_vec();
        let mut dependencies = Vec::new();
        for dependency in json_object(&units[index], "Cargo unit")?
            .get("dependencies")
            .and_then(JsonValue::as_array)
            .ok_or_else(|| json_schema("Cargo unit dependencies are absent"))?
        {
            let target = dependency
                .get("index")
                .and_then(JsonValue::as_u64)
                .and_then(|index| usize::try_from(index).ok())
                .filter(|index| *index < units.len())
                .ok_or_else(|| json_schema("Cargo unit dependency index is invalid"))?;
            let key = unit_keys[target]
                .ok_or_else(|| json_schema("protected Cargo unit edge escaped the cohort"))?;
            dependencies.push(key);
        }
        dependencies.sort_unstable();
        for dependency in dependencies {
            bytes.extend_from_slice(dependency.as_bytes());
        }
        let digest = projection_digest_bytes(b"rawr.import.cargo.protected-unit-key/v2", &bytes);
        if protected_keys.contains(&digest) {
            return Err(json_schema("Cargo unit identity is ambiguous"));
        }
        let set_node = JsonOwnership::nodes(1);
        allocation.acquire(set_node)?;
        scratch = scratch.checked_add(set_node)?;
        protected_keys.insert(digest);
        unit_keys[index] = Some(digest);
    }

    let parts_structure =
        JsonOwnership::collection().checked_add(JsonOwnership::nodes(reachable_units.len()))?;
    allocation.acquire(parts_structure)?;
    let mut projected = Vec::new();
    projected
        .try_reserve_exact(reachable_units.len())
        .map_err(|source| Error::Allocation {
            operation: "protected Cargo projection",
            source,
        })?;
    for (index, unit) in units.iter().enumerate() {
        if !reachable_units.contains(&index) {
            continue;
        }
        let package = unit_packages[index]
            .ok_or_else(|| json_schema("protected Cargo unit package escaped reachability"))?;
        let mut unit = allocation.clone_tree(unit)?;
        normalize_unit_base(&mut unit, package, packages, allocation)?;
        unit.transform(|unit, ownership| {
            let object = json_object_mut(unit, "protected Cargo unit")?;
            let dependencies = object
                .get_mut("dependencies")
                .and_then(JsonValue::as_array_mut)
                .ok_or_else(|| json_schema("Cargo unit dependencies are absent"))?;
            for dependency in dependencies {
                let dependency = json_object_mut(dependency, "Cargo unit dependency")?;
                let target = dependency
                    .get("index")
                    .and_then(JsonValue::as_u64)
                    .and_then(|index| usize::try_from(index).ok())
                    .filter(|index| *index < units.len())
                    .ok_or_else(|| json_schema("Cargo unit dependency index is invalid"))?;
                let target = unit_keys[target]
                    .as_ref()
                    .ok_or_else(|| json_schema("protected Cargo unit edge escaped the cohort"))?;
                insert_object_string(dependency, "unit", 64, allocation, ownership, || {
                    Ok(target.to_string())
                })?;
                drop(remove_object_field(
                    dependency, "index", allocation, ownership,
                )?);
            }
            let key = unit_keys[index]
                .as_ref()
                .ok_or_else(|| json_schema("protected Cargo unit key is absent"))?;
            insert_object_string(object, "unit", 64, allocation, ownership, || {
                Ok(key.to_string())
            })
        })?;
        projected.push(projection_bytes_budgeted(unit, allocation)?);
    }
    let version = object.get("version").unwrap_or(&JsonValue::Null);
    let version = projection_bytes_budgeted(allocation.clone_tree(version)?, allocation)?;
    drop(unit_packages);
    drop(unit_keys);
    drop(protected_keys);
    allocation.release(scratch)?;

    const PREFIX: &[u8] = b"{\"units\":[";
    const BETWEEN: &[u8] = b"],\"version\":";
    const SUFFIX: &[u8] = b"}";
    let unit_bytes = projected.iter().try_fold(0_usize, |total, unit| {
        total
            .checked_add(unit.value().len())
            .ok_or_else(|| json_schema("protected Cargo unit projection byte count overflow"))
    })?;
    let comma_bytes = projected.len().saturating_sub(1);
    let total = PREFIX
        .len()
        .checked_add(unit_bytes)
        .and_then(|total| total.checked_add(comma_bytes))
        .and_then(|total| total.checked_add(BETWEEN.len()))
        .and_then(|total| total.checked_add(version.value().len()))
        .and_then(|total| total.checked_add(SUFFIX.len()))
        .ok_or_else(|| json_schema("protected Cargo unit projection byte count overflow"))?;
    let output = allocation.retain_with(JsonOwnership::buffer(total), || {
        let mut bytes = Vec::new();
        bytes
            .try_reserve_exact(total)
            .map_err(|source| Error::Allocation {
                operation: "protected Cargo projection",
                source,
            })?;
        bytes.extend_from_slice(PREFIX);
        for (index, unit) in projected.iter().enumerate() {
            if index != 0 {
                bytes.push(b',');
            }
            bytes.extend_from_slice(unit.value());
        }
        bytes.extend_from_slice(BETWEEN);
        bytes.extend_from_slice(version.value());
        bytes.extend_from_slice(SUFFIX);
        if bytes.len() != total {
            return Err(json_schema(
                "protected Cargo unit projection byte count changed during assembly",
            ));
        }
        Ok(bytes.into_boxed_slice())
    })?;
    for unit in projected {
        unit.drop_from(allocation)?;
    }
    version.drop_from(allocation)?;
    allocation.release(parts_structure)?;
    Ok(output)
}

fn normalize_unit_base(
    value: &mut Budgeted<JsonValue>,
    package: &ProtectedPackage,
    packages: &BTreeMap<Arc<str>, ProtectedPackage>,
    allocation: &mut JsonAllocation,
) -> Result<()> {
    value.transform(|value, ownership| {
        let object = json_object_mut(value, "protected Cargo unit")?;
        replace_object_key_token(object, "pkg_id", &package.key, allocation, ownership)?;
        match object.get("is_std") {
            None => {
                let inserted = JsonValue::Bool(false);
                let acquired = JsonOwnership::object_field("is_std", &inserted)?;
                allocation.acquire(acquired)?;
                object.insert("is_std".into(), inserted);
                *ownership = ownership.checked_add(acquired)?;
            }
            Some(JsonValue::Bool(false)) => {}
            Some(JsonValue::Bool(true)) => {
                return Err(json_schema(
                    "protected Cargo unit entered build-std/sysroot",
                ));
            }
            Some(_) => return Err(json_schema("Cargo unit is_std is invalid")),
        }
        let target = object
            .get_mut("target")
            .ok_or_else(|| json_schema("Cargo unit target is absent"))?;
        normalize_path_field(
            json_object_mut(target, "Cargo unit target")?,
            "src_path",
            package,
            packages,
            allocation,
            ownership,
        )?;
        Ok(())
    })
}

#[allow(clippy::too_many_arguments)]
fn normalize_path_field(
    object: &mut JsonMap<String, JsonValue>,
    field: &'static str,
    package: &ProtectedPackage,
    packages: &BTreeMap<Arc<str>, ProtectedPackage>,
    allocation: &mut JsonAllocation,
    ownership: &mut JsonOwnership,
) -> Result<()> {
    let value = object
        .get_mut(field)
        .ok_or_else(|| json_schema(&format!("protected path `{field}` is absent")))?;
    let path = value
        .as_str()
        .ok_or_else(|| json_schema(&format!("protected path `{field}` is invalid")))?;
    let normalized_bytes = normalized_path_len(path, package, packages)?;
    rewrite_json_string_from_bytes(value, normalized_bytes, allocation, ownership, |path| {
        normalized_path_bytes(path, package, packages, normalized_bytes)
    })
}

#[allow(clippy::too_many_arguments)]
fn normalize_optional_path_field(
    object: &mut JsonMap<String, JsonValue>,
    field: &'static str,
    package: &ProtectedPackage,
    packages: &BTreeMap<Arc<str>, ProtectedPackage>,
    allocation: &mut JsonAllocation,
    ownership: &mut JsonOwnership,
) -> Result<()> {
    match object.get(field) {
        None | Some(JsonValue::Null) => Ok(()),
        Some(JsonValue::String(_)) => {
            normalize_path_field(object, field, package, packages, allocation, ownership)
        }
        Some(_) => Err(json_schema(&format!("optional path `{field}` is invalid"))),
    }
}

fn normalized_path_len(
    value: &str,
    owner: &ProtectedPackage,
    packages: &BTreeMap<Arc<str>, ProtectedPackage>,
) -> Result<usize> {
    let path = Path::new(value);
    let (label_bytes, relative) = if path.is_relative() {
        ("$SELF".len(), path)
    } else if let Ok(relative) = path.strip_prefix(&owner.root) {
        ("$SELF".len(), relative)
    } else {
        let (_, relative) = packages
            .values()
            .find_map(|package| {
                path.strip_prefix(&package.root)
                    .ok()
                    .map(|relative| (package, relative))
            })
            .ok_or_else(|| {
                json_schema(&format!(
                    "protected Cargo path escaped the protected cohort: {value}"
                ))
            })?;
        ("$PACKAGE/".len() + 64, relative)
    };
    relative
        .components()
        .try_fold(label_bytes, |bytes, component| {
            let component = match component {
                std::path::Component::Normal(component) => component,
                std::path::Component::CurDir => return Ok(bytes),
                std::path::Component::Prefix(_)
                | std::path::Component::RootDir
                | std::path::Component::ParentDir => {
                    return Err(json_schema(&format!(
                        "protected Cargo path is not normalized: {value}"
                    )));
                }
            };
            let component = component
                .to_str()
                .ok_or_else(|| json_schema("protected Cargo path is not UTF-8"))?;
            bytes
                .checked_add(1_usize.saturating_add(component.len()))
                .ok_or_else(|| json_schema("protected Cargo normalized path length overflow"))
        })
}

fn normalized_path_bytes(
    value: &str,
    owner: &ProtectedPackage,
    packages: &BTreeMap<Arc<str>, ProtectedPackage>,
    expected_bytes: usize,
) -> Result<Vec<u8>> {
    let path = Path::new(value);
    let (package, relative) = if path.is_relative() {
        (None, path)
    } else if let Ok(relative) = path.strip_prefix(&owner.root) {
        (None, relative)
    } else {
        let (package, relative) = packages
            .values()
            .find_map(|package| {
                path.strip_prefix(&package.root)
                    .ok()
                    .map(|relative| (package, relative))
            })
            .ok_or_else(|| {
                json_schema(&format!(
                    "protected Cargo path escaped the protected cohort: {value}"
                ))
            })?;
        (Some(package), relative)
    };
    let mut normalized = Vec::new();
    normalized
        .try_reserve_exact(expected_bytes)
        .map_err(|source| Error::Allocation {
            operation: "protected Cargo projection",
            source,
        })?;
    match package {
        None => normalized.extend_from_slice(b"$SELF"),
        Some(package) => {
            normalized.extend_from_slice(b"$PACKAGE/");
            normalized.extend_from_slice(&key_token_bytes(&package.key));
        }
    }
    for component in relative.components() {
        let component = match component {
            std::path::Component::Normal(component) => component,
            std::path::Component::CurDir => continue,
            std::path::Component::Prefix(_)
            | std::path::Component::RootDir
            | std::path::Component::ParentDir => {
                return Err(json_schema(&format!(
                    "protected Cargo path is not normalized: {value}"
                )));
            }
        };
        normalized.push(b'/');
        normalized.extend_from_slice(
            component
                .to_str()
                .ok_or_else(|| json_schema("protected Cargo path is not UTF-8"))?
                .as_bytes(),
        );
    }
    if normalized.len() != expected_bytes {
        return Err(json_schema(
            "protected Cargo normalized path length drifted",
        ));
    }
    Ok(normalized)
}

const PACKAGE_KEY_DOMAIN: &[u8] = b"rawr.import.cargo.protected-package-key/v1";

fn key_token_bytes(key: &Key) -> [u8; 64] {
    let mut digest = Sha256::new();
    digest.update(PACKAGE_KEY_DOMAIN);
    for value in [key.name.as_ref(), key.version.as_ref(), key.source.as_ref()] {
        digest.update((value.len() as u64).to_le_bytes());
        digest.update(value.as_bytes());
    }
    let digest = digest.finalize();
    let mut encoded = [0_u8; 64];
    const HEX: &[u8; 16] = b"0123456789abcdef";
    for (index, byte) in digest.iter().copied().enumerate() {
        encoded[index.saturating_mul(2)] = HEX[usize::from(byte >> 4)];
        encoded[index.saturating_mul(2).saturating_add(1)] = HEX[usize::from(byte & 0x0f)];
    }
    encoded
}

fn key_token(key: &Key) -> String {
    key_token_bytes(key).into_iter().map(char::from).collect()
}

fn projection_digest_budgeted(
    domain: &[u8],
    value: Budgeted<JsonValue>,
    allocation: &mut JsonAllocation,
) -> Result<Digest> {
    let bytes = projection_bytes_budgeted(value, allocation)?;
    let digest = projection_digest_bytes(domain, bytes.value());
    bytes.drop_from(allocation)?;
    Ok(digest)
}

fn projection_bytes_budgeted(
    value: Budgeted<JsonValue>,
    allocation: &mut JsonAllocation,
) -> Result<Budgeted<Box<[u8]>>> {
    let consumed = value.ownership();
    allocation.transform_owned(consumed, |allocation| {
        let mut value = value;
        canonicalize_json(value.value_mut(), allocation)?;
        struct Counter(usize);
        impl std::io::Write for Counter {
            fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
                self.0 = self
                    .0
                    .checked_add(bytes.len())
                    .ok_or_else(|| std::io::Error::other("projection JSON byte count overflow"))?;
                Ok(bytes.len())
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }
        let mut counter = Counter(0);
        serde_json::to_writer(&mut counter, value.value()).map_err(|error| Error::Schema {
            document: "protected Cargo projection",
            reason: error.to_string().into_boxed_str(),
        })?;
        let ownership = JsonOwnership::buffer(counter.0);
        let bytes = allocation.retain_with(ownership, || {
            let mut bytes = Vec::new();
            bytes
                .try_reserve_exact(counter.0)
                .map_err(|source| Error::Allocation {
                    operation: "protected Cargo projection",
                    source,
                })?;
            serde_json::to_writer(&mut bytes, value.value()).map_err(|error| Error::Schema {
                document: "protected Cargo projection",
                reason: error.to_string().into_boxed_str(),
            })?;
            Ok(bytes.into_boxed_slice())
        })?;
        value.drop_from(allocation)?;
        Ok(bytes)
    })
}

#[cfg(test)]
fn projection_bytes(value: JsonValue) -> Result<Box<[u8]>> {
    let mut allocation = JsonAllocation::empty();
    let ownership = JsonOwnership::tree(&value)?;
    let value = allocation.retain_with(ownership, || Ok(value))?;
    let bytes = projection_bytes_budgeted(value, &mut allocation)?;
    let (bytes, _) = bytes.into_parts();
    Ok(bytes)
}

fn projection_digest_bytes(domain: &[u8], bytes: &[u8]) -> Digest {
    let mut digest = Sha256::new();
    digest.update(domain);
    digest.update((bytes.len() as u64).to_le_bytes());
    digest.update(bytes);
    Digest(digest.finalize().into())
}

fn canonicalize_json(value: &mut JsonValue, allocation: &mut JsonAllocation) -> Result<()> {
    match value {
        JsonValue::Array(values) => {
            for value in values.iter_mut() {
                canonicalize_json(value, allocation)?;
            }
            let scratch = values.iter().try_fold(
                JsonOwnership::collection().checked_add(JsonOwnership::nodes(values.len()))?,
                |ownership, value| {
                    ownership.checked_add(JsonOwnership::buffer(canonical_json_len(value)?))
                },
            )?;
            let sorted = allocation.with_scratch(scratch, || {
                let mut keyed = Vec::new();
                keyed
                    .try_reserve_exact(values.len())
                    .map_err(|source| Error::Allocation {
                        operation: "protected Cargo projection",
                        source,
                    })?;
                for value in std::mem::take(values) {
                    let mut key = Vec::new();
                    let length = canonical_json_len(&value)?;
                    key.try_reserve_exact(length)
                        .map_err(|source| Error::Allocation {
                            operation: "protected Cargo projection",
                            source,
                        })?;
                    serde_json::to_writer(&mut key, &value).map_err(|error| Error::Schema {
                        document: "protected Cargo projection",
                        reason: error.to_string().into_boxed_str(),
                    })?;
                    keyed.push((key.into_boxed_slice(), value));
                }
                keyed.sort_unstable_by(|left, right| left.0.cmp(&right.0));
                Ok(keyed
                    .into_iter()
                    .map(|(_, value)| value)
                    .collect::<Vec<_>>())
            })?;
            *values = sorted;
        }
        JsonValue::Object(object) => {
            for value in object.values_mut() {
                canonicalize_json(value, allocation)?;
            }
            let scratch = JsonOwnership::collection()
                .checked_add(JsonOwnership::collection())?
                .checked_add(JsonOwnership::nodes(object.len()))?;
            let sorted = allocation.with_scratch(scratch, || {
                let retained = std::mem::take(object);
                let mut sorted = Vec::new();
                sorted
                    .try_reserve_exact(retained.len())
                    .map_err(|source| Error::Allocation {
                        operation: "protected Cargo projection",
                        source,
                    })?;
                sorted.extend(retained);
                sorted.sort_unstable_by(|left, right| left.0.cmp(&right.0));
                let mut object = JsonMap::new();
                object.extend(sorted);
                Ok(object)
            })?;
            *object = sorted;
        }
        JsonValue::Null | JsonValue::Bool(_) | JsonValue::Number(_) | JsonValue::String(_) => {}
    }
    Ok(())
}

fn canonical_json_len(value: &JsonValue) -> Result<usize> {
    struct Counter(usize);
    impl std::io::Write for Counter {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.0 = self
                .0
                .checked_add(bytes.len())
                .ok_or_else(|| std::io::Error::other("projection sort-key size overflow"))?;
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let mut counter = Counter(0);
    serde_json::to_writer(&mut counter, value).map_err(|error| Error::Schema {
        document: "protected Cargo projection",
        reason: error.to_string().into_boxed_str(),
    })?;
    Ok(counter.0)
}

fn json_object<'a>(value: &'a JsonValue, field: &str) -> Result<&'a JsonMap<String, JsonValue>> {
    value
        .as_object()
        .ok_or_else(|| json_schema(&format!("{field} is not an object")))
}

fn json_object_mut<'a>(
    value: &'a mut JsonValue,
    field: &str,
) -> Result<&'a mut JsonMap<String, JsonValue>> {
    value
        .as_object_mut()
        .ok_or_else(|| json_schema(&format!("{field} is not an object")))
}

fn json_array<'a>(value: Option<&'a JsonValue>, field: &str) -> Result<&'a [JsonValue]> {
    value
        .and_then(JsonValue::as_array)
        .map(Vec::as_slice)
        .ok_or_else(|| json_schema(&format!("{field} is not an array")))
}

fn json_bounded_string(
    object: &JsonMap<String, JsonValue>,
    field: &'static str,
) -> Result<Box<str>> {
    Ok(json_bounded_str(object, field)?.into())
}

/// Read one string field. The string stays owned by the JSON epoch that
/// admitted the document.
fn json_bounded_str<'a>(
    object: &'a JsonMap<String, JsonValue>,
    field: &'static str,
) -> Result<&'a str> {
    let value = object
        .get(field)
        .and_then(JsonValue::as_str)
        .ok_or_else(|| json_schema(&format!("`{field}` is not a string")))?;
    Ok(value)
}

fn json_schema(reason: &str) -> Error {
    Error::Schema {
        document: "protected Cargo universe facts",
        reason: reason.into(),
    }
}

impl Closure {
    #[cfg(test)]
    fn parse(bytes: &[u8]) -> Result<Self> {
        Self::parse_rooted(bytes, ROOT_PACKAGE)
    }

    fn parse_rooted(bytes: &[u8], root: &str) -> Result<Self> {
        let text = std::str::from_utf8(bytes).map_err(|error| Error::Schema {
            document: "resolution universe lock",
            reason: error.to_string().into_boxed_str(),
        })?;
        let document = text.parse::<DocumentMut>().map_err(|error| Error::Schema {
            document: "resolution universe lock",
            reason: error.to_string().into_boxed_str(),
        })?;
        let tables = document
            .get("package")
            .and_then(Item::as_array_of_tables)
            .ok_or_else(|| Error::Schema {
                document: "resolution universe lock",
                reason: "package array is absent".into(),
            })?;
        let mut raw = BTreeMap::new();
        for table in tables {
            let key = key(table)?;
            if raw.insert(key, table).is_some() {
                return Err(schema("duplicate package identity"));
            }
        }
        let dependency_index = dependency_index(raw.keys())?;
        let mut edges = 0_usize;
        for table in raw.values() {
            let count = table
                .get("dependencies")
                .and_then(Item::as_array)
                .map_or(0, toml_edit::Array::len);
            edges = edges.saturating_add(count);
        }
        let mut roots = raw
            .keys()
            .filter(|key| key.name.as_ref() == root && key.source.is_empty());
        let root = roots
            .next()
            .cloned()
            .ok_or_else(|| schema("profile root package is absent"))?;
        if roots.next().is_some() {
            return Err(schema("profile root package is ambiguous"));
        }
        let mut packages = Vec::new();
        packages
            .try_reserve_exact(raw.len())
            .map_err(|source| Error::Allocation {
                operation: "resolution universe lock packages",
                source,
            })?;
        for (key, table) in &raw {
            let dependencies = dependencies(table, &dependency_index)?;
            packages.push(Package {
                name: key.name.as_ref().into(),
                version: key.version.as_ref().into(),
                source: key.source.as_ref().into(),
                checksum: bounded_string(string(table, "checksum").unwrap_or_default())?,
                dependencies: dependencies.into_boxed_slice(),
            });
        }
        packages.sort_unstable();
        let _ = root;
        let mut digest = Sha256::new();
        digest.update(b"rawr.import.cargo.resolution-universe-closure/v1");
        for package in &packages {
            for value in [
                &package.name,
                &package.version,
                &package.source,
                &package.checksum,
            ] {
                digest.update((value.len() as u64).to_le_bytes());
                digest.update(value.as_bytes());
            }
            digest.update((package.dependencies.len() as u64).to_le_bytes());
            for dependency in &package.dependencies {
                for value in [
                    dependency.name.as_ref(),
                    dependency.version.as_ref(),
                    dependency.source.as_ref(),
                ] {
                    digest.update((value.len() as u64).to_le_bytes());
                    digest.update(value.as_bytes());
                }
            }
        }
        Ok(Self {
            packages: packages.into_boxed_slice(),
            digest: Digest(digest.finalize().into()),
        })
    }

    /// The one fact the facade fixes is the framework crate it was
    /// compiled against. Every `iced` package in the project's lock must be
    /// that exact package (version and source); every other package, edge and
    /// checksum is the project's own Cargo resolution. A seed without the
    /// framework package (test fixtures) fixes nothing.
    fn require_framework(&self, framework: Option<&str>, actual: &Self) -> Result<()> {
        let Some(framework) = framework else {
            return Ok(());
        };
        let Some(expected) = self
            .packages
            .iter()
            .find(|package| package.name.as_ref() == framework)
        else {
            return Ok(());
        };
        let mismatch = actual.packages.iter().find(|package| {
            package.name.as_ref() == framework
                && (package.version.as_ref(), package.source.as_ref())
                    != (expected.version.as_ref(), expected.source.as_ref())
        });
        match mismatch {
            None => Ok(()),
            Some(actual) => Err(Error::Request {
                reason: format!(
                    "this rawr supports {framework} {}; the project uses {framework} {}",
                    describe_framework(expected),
                    describe_framework(actual),
                )
                .into_boxed_str(),
            }),
        }
    }
}

/// `at rev <commit> (<repository>)` for a git source, `<version> (<source>)`
/// otherwise.
fn describe_framework(package: &Package) -> String {
    let source = package.source.as_ref();
    match source.strip_prefix("git+") {
        Some(git) => {
            let (location, commit) = git.split_once('#').unwrap_or((git, ""));
            let repository = location.split_once('?').map_or(location, |(url, _)| url);
            format!("at rev {commit} ({repository})")
        }
        None if source.is_empty() => format!("{} (path)", package.version),
        None => format!("{} ({source})", package.version),
    }
}

fn key(table: &Table) -> Result<Key> {
    Ok(Key {
        name: bounded_string(required(table, "name")?)?.into(),
        version: bounded_string(required(table, "version")?)?.into(),
        source: bounded_string(string(table, "source").unwrap_or_default())?.into(),
    })
}

/// Retain one lock string.
fn bounded_string(value: &str) -> Result<Box<str>> {
    Ok(value.into())
}

fn dependencies(table: &Table, index: &DependencyIndex) -> Result<Vec<Key>> {
    let Some(values) = table.get("dependencies").and_then(Item::as_array) else {
        return Ok(Vec::new());
    };
    let mut resolved = Vec::new();
    resolved
        .try_reserve_exact(values.len())
        .map_err(|source| Error::Allocation {
            operation: "resolution universe dependencies",
            source,
        })?;
    for value in values {
        let dependency = value
            .as_str()
            .ok_or_else(|| schema("dependency identity is not a string"))?;
        let key = index
            .get(dependency)
            .ok_or_else(|| schema("dependency identity is absent"))?
            .clone()
            .ok_or_else(|| schema("dependency identity is ambiguous"))?;
        resolved.push(key);
    }
    resolved.sort_unstable();
    Ok(resolved)
}

/// Each spelling a lock's `dependencies` entry may use, and the package it
/// names. Cargo writes the shortest spelling no other package shares, and a
/// package with no source (a path package) can only be written without one.
/// So when a sourced package and a path package share a name and version,
/// the bare `name version` names the path package, and the sourced one is
/// always written with its `(source)`. A spelling two path packages share,
/// or two sourced packages and no path package, names none.
fn dependency_index<'a>(keys: impl Iterator<Item = &'a Key> + Clone) -> Result<DependencyIndex> {
    let mut index = DependencyIndex::new();
    for key in keys.clone().filter(|key| !key.source.is_empty()) {
        for spelling in dependency_spellings(key)? {
            index
                .entry(spelling)
                .and_modify(|named| *named = None)
                .or_insert_with(|| Some(key.clone()));
        }
    }
    let mut by_path = BTreeSet::new();
    for key in keys.filter(|key| key.source.is_empty()) {
        for spelling in dependency_spellings(key)? {
            let named = if by_path.insert(spelling.clone()) {
                Some(key.clone())
            } else {
                None
            };
            index.insert(spelling, named);
        }
    }
    Ok(index)
}

fn dependency_spellings(key: &Key) -> Result<Vec<Box<str>>> {
    let mut spellings = Vec::with_capacity(if key.source.is_empty() { 2 } else { 3 });
    spellings.push(key.name.as_ref().into());
    let versioned = format!("{} {}", key.name, key.version);
    spellings.push(versioned.clone().into_boxed_str());
    if !key.source.is_empty() {
        // Cargo writes the source without its precise part: a git source's
        // `#<commit>` is left out of the dependency spelling.
        let source = key.source.as_ref();
        let source = if source.starts_with("git+") {
            source.rsplit_once('#').map_or(source, |(source, _)| source)
        } else {
            source
        };
        spellings.push(format!("{versioned} ({source})").into_boxed_str());
    }
    Ok(spellings)
}

fn required<'a>(table: &'a Table, field: &'static str) -> Result<&'a str> {
    string(table, field).ok_or_else(|| schema(field))
}

fn string<'a>(table: &'a Table, field: &str) -> Option<&'a str> {
    table.get(field).and_then(Item::as_str)
}

fn schema(reason: &str) -> Error {
    Error::Schema {
        document: "resolution universe lock",
        reason: reason.into(),
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn staged_candidate_names_only_its_exact_bytes_and_universe() {
        let definition = definition();
        let candidate = definition
            .candidate(SEED.as_bytes().into())
            .expect("seed candidate parses");
        let length = SEED.len() as u64;
        let digest: [u8; 32] = Sha256::digest(SEED.as_bytes()).into();
        assert!(candidate.names(&definition, (length, digest)));
        let mut changed = digest;
        changed[0] ^= 1;
        assert!(!candidate.names(&definition, (length, changed)));
        assert!(!candidate.names(&definition, (length + 1, digest)));
        let mut foreign = definition.clone();
        foreign.identity = super::Digest([7; 32]);
        assert!(!candidate.names(&foreign, (length, digest)));
        let shared = candidate.clone();
        assert!(
            std::ptr::eq(shared.bytes(), candidate.bytes()),
            "clones share one parse"
        );
    }

    #[test]
    fn malformed_candidate_lock_refuses_as_a_parse_error() {
        let Err(error) = definition().candidate(b"[".as_slice().into()) else {
            panic!("malformed TOML refuses");
        };
        assert!(!matches!(error, crate::Error::Limit { .. }));
    }

    use super::Definition;
    use crate::cargo::Limits;
    use sha2::{Digest as _, Sha256};
    use std::{path::PathBuf, sync::Arc};

    const SEED: &str = "version = 4\n\n[[package]]\nname = \"rawr-iced\"\nversion = \"1.0.0\"\ndependencies = [\"shared\"]\n\n[[package]]\nname = \"shared\"\nversion = \"1.2.3\"\nsource = \"registry+exact\"\nchecksum = \"aa\"\n";
    const AUTHENTICATED_METADATA: &str = r#"{
        "packages": [{
            "id": "path+file:///fixture/rawr-iced#1.0.0",
            "name": "rawr-iced",
            "version": "1.0.0",
            "source": null,
            "manifest_path": "/fixture/rawr-iced/Cargo.toml",
            "dependencies": [],
            "targets": [{
                "kind": ["lib"],
                "src_path": "/fixture/rawr-iced/src/lib.rs"
            }]
        }],
        "resolve": {"nodes": [{
            "id": "path+file:///fixture/rawr-iced#1.0.0",
            "dependencies": [],
            "deps": [],
            "features": ["base"]
        }]}
    }"#;
    const AUTHENTICATED_UNIT: &str = r#"{
        "version": 1,
        "units": [{
            "pkg_id": "path+file:///fixture/rawr-iced#1.0.0",
            "target": {
                "kind": ["lib"],
                "crate_types": ["rlib"],
                "src_path": "/fixture/rawr-iced/src/lib.rs"
            },
            "mode": "build",
            "features": ["base"],
            "dependencies": [],
            "is_std": false
        }]
    }"#;

    fn definition() -> Definition {
        Definition::new(
            super::ROOT_PACKAGE,
            Some(super::FRAMEWORK_PACKAGE),
            "fixture",
            1,
            1,
            [7; 32],
            SEED.as_bytes(),
            "aarch64-apple-darwin",
            "dev",
            "rustc",
            "cargo",
        )
        .expect("fixture universe seals")
    }

    fn bind(lock: &super::FinalLock, context: u8, metadata: u8, unit: u8) -> super::Proof {
        let universe = lock.0.definition.identity;
        lock.bind(
            super::ContextProof {
                universe,
                digest: super::Digest([context; 32]),
            },
            &super::MetadataProof {
                universe,
                digest: super::Digest([metadata; 32]),
                packages: Default::default(),
                _ownership: super::JsonOwnership::empty(),
            },
            super::UnitProof {
                universe,
                digest: super::Digest([unit; 32]),
            },
            super::BuildProof {
                universe,
                projection: super::Digest([5; 32]),
                input: super::Digest([6; 32]),
            },
        )
        .expect("same-universe facts bind")
    }

    fn metadata_proof(definition: &Definition) -> super::MetadataProof {
        let universe = definition.identity;
        let packages = definition
            .protected
            .packages
            .iter()
            .enumerate()
            .map(|(index, package)| {
                (
                    Arc::<str>::from(format!("package-{index}")),
                    super::ProtectedPackage {
                        key: super::Key {
                            name: package.name.as_ref().into(),
                            version: package.version.as_ref().into(),
                            source: package.source.as_ref().into(),
                        },
                        root: PathBuf::new().into(),
                    },
                )
            })
            .collect();
        super::MetadataProof {
            universe,
            digest: super::Digest([0; 32]),
            packages,
            _ownership: super::JsonOwnership::empty(),
        }
    }

    fn admit(definition: Definition, bytes: Box<[u8]>) -> crate::Result<super::FinalLock> {
        let metadata = metadata_proof(&definition);
        definition.candidate(bytes)?.admit(&metadata)
    }

    fn pending(definition: Definition) -> super::Pending {
        let metadata = metadata_proof(&definition);
        let universe = definition.identity;
        let lock = definition
            .candidate(SEED.as_bytes().into())
            .expect("fixture candidate parses")
            .admit(&metadata)
            .expect("fixture candidate admits");
        super::Pending::transform(
            super::Digest([1; 32]),
            lock,
            super::ContextProof {
                universe,
                digest: super::Digest([2; 32]),
            },
            metadata,
            super::UnitProof {
                universe,
                digest: super::Digest([3; 32]),
            },
            None,
        )
    }

    #[test]
    fn typed_metadata_shares_package_authority_with_protected_and_reachable_maps() {
        let root_id = "path+file:///fixture/rawr-iced#1.0.0";
        let dependency_id = "registry+https://example.invalid/index#shared@1.2.3";
        let registry_source: Arc<str> = "registry+https://example.invalid/index".into();
        let metadata = super::super::unit::Metadata::fixture_packages([
            super::super::unit::Package::fixture(
                root_id,
                "rawr-iced",
                "1.0.0",
                PathBuf::from("/fixture/rawr-iced"),
                super::super::unit::PackageSource::Path {
                    cargo_id: root_id.into(),
                },
            ),
            super::super::unit::Package::fixture(
                dependency_id,
                "shared",
                "1.2.3",
                PathBuf::from("/fixture/shared"),
                super::super::unit::PackageSource::Registry {
                    cargo_id: Arc::clone(&registry_source),
                    checksum: None,
                },
            ),
        ]);
        let (typed_root_id, typed_root) = metadata
            .package_entries()
            .find(|(id, _)| id.as_ref() == root_id)
            .expect("typed root package exists");
        let typed_root_name = typed_root.shared_name();
        let typed_root_version = typed_root.shared_version();
        let typed_root_path = typed_root.shared_root();
        let (typed_dependency_id, typed_dependency) = metadata
            .package_entries()
            .find(|(id, _)| id.as_ref() == dependency_id)
            .expect("typed dependency package exists");
        let typed_dependency_source = typed_dependency
            .source()
            .shared_universe_source()
            .expect("registry package has a universe source");

        let mut allocation = super::JsonAllocation::empty();
        let packages = super::metadata_packages(&metadata, &mut allocation)
            .expect("typed package authority projects");
        assert_eq!(
            packages.ownership().usage(),
            (3, 0, 0, 1),
            "the protected map owns only its collection and two member nodes",
        );
        let (protected_root_id, protected_root) = packages
            .value()
            .get_key_value(root_id)
            .expect("protected root package exists");
        assert!(Arc::ptr_eq(typed_root_id, protected_root_id));
        assert!(Arc::ptr_eq(&typed_root_name, &protected_root.key.name));
        assert!(Arc::ptr_eq(
            &typed_root_version,
            &protected_root.key.version
        ));
        assert!(Arc::ptr_eq(&typed_root_path, &protected_root.root));
        assert!(matches!(protected_root.key.source, super::KeySource::Path));
        assert_eq!(
            super::key_token_bytes(&protected_root.key),
            super::key_token_bytes(&super::Key {
                name: "rawr-iced".into(),
                version: "1.0.0".into(),
                source: "".into(),
            }),
            "the typed path-source state preserves the prior null-source package key",
        );
        let (protected_dependency_id, protected_dependency) = packages
            .value()
            .get_key_value(dependency_id)
            .expect("protected dependency package exists");
        assert!(Arc::ptr_eq(typed_dependency_id, protected_dependency_id));
        let super::KeySource::Cargo(protected_dependency_source) = &protected_dependency.key.source
        else {
            panic!("registry package retains its Cargo source");
        };
        assert!(Arc::ptr_eq(
            &typed_dependency_source,
            protected_dependency_source,
        ));
        assert!(Arc::ptr_eq(&registry_source, protected_dependency_source));

        let units = serde_json::json!({
            "units": [{
                "pkg_id": root_id,
                "mode": "build",
                "target": {"kind": ["lib"]},
                "dependencies": [{"index": 1}]
            }, {
                "pkg_id": dependency_id,
                "mode": "build",
                "target": {"kind": ["lib"]},
                "dependencies": []
            }]
        });
        let strings_before = allocation.full_usage().2;
        let (reachable_units, reachable_packages) =
            super::unit_reachability(&units, packages.value(), &mut allocation)
                .expect("both package identities are reachable");
        assert_eq!(reachable_packages.ownership().usage(), (3, 0, 0, 1));
        assert_eq!(allocation.full_usage().2, strings_before);
        assert!(Arc::ptr_eq(
            reachable_packages
                .value()
                .get(root_id)
                .expect("root identity is reachable"),
            protected_root_id,
        ));
        assert!(Arc::ptr_eq(
            reachable_packages
                .value()
                .get(dependency_id)
                .expect("dependency identity is reachable"),
            protected_dependency_id,
        ));

        reachable_units
            .drop_from(&mut allocation)
            .expect("reachable unit ownership releases");
        reachable_packages
            .drop_from(&mut allocation)
            .expect("reachable package ownership releases");
        packages
            .drop_from(&mut allocation)
            .expect("protected package ownership releases");
        assert_eq!(allocation.full_usage(), (0, 0, 0, 0));
    }

    #[test]
    fn fresh_and_exact_persisted_locks_admit_without_fallback() {
        admit(definition(), SEED.as_bytes().into()).expect("authenticated persisted lock admits");
    }

    #[test]
    fn unrelated_candidate_packages_are_admitted() {
        let augmented = format!(
            "{SEED}\n[[package]]\nname = \"project-only\"\nversion = \"9.0.0\"\nsource = \"registry+exact\"\nchecksum = \"bb\"\n"
        );
        admit(definition(), augmented.into_bytes().into_boxed_slice())
            .expect("unrelated project closure is allowed");
    }

    /// The project's own resolution of packages the profile reaches
    /// (versions, checksums, edges, extra packages) is admitted as Cargo made it.
    #[test]
    fn project_resolution_of_profile_packages_is_admitted() {
        let expanded = SEED
            .replace(
                "dependencies = [\"shared\"]",
                "dependencies = [\"profile-extra\", \"shared\"]",
            )
            + "\n[[package]]\nname = \"profile-extra\"\nversion = \"4.0.0\"\nsource = \"registry+exact\"\nchecksum = \"cc\"\n";
        for lock in [
            SEED.replace("version = \"1.2.3\"", "version = \"1.2.4\""),
            SEED.replace("checksum = \"aa\"", "checksum = \"bb\""),
            SEED.replace("dependencies = [\"shared\"]", "dependencies = []"),
            expanded,
        ] {
            admit(definition(), lock.into_bytes().into_boxed_slice())
                .expect("the project's own resolution is admitted");
        }
    }

    #[test]
    fn ambiguous_profile_root_rejects() {
        let lock = format!("{SEED}\n[[package]]\nname = \"rawr-iced\"\nversion = \"2.0.0\"\n");
        assert!(admit(definition(), lock.into_bytes().into_boxed_slice()).is_err());
    }

    #[test]
    fn dangling_lock_dependency_rejects() {
        let dangling = SEED.replace("dependencies = [\"shared\"]", "dependencies = [\"absent\"]");
        let error = super::Closure::parse(dangling.as_bytes())
            .expect_err("a dangling dependency identity refuses");
        assert!(!matches!(error, crate::Error::Limit { .. }));
        assert!(format!("{error}").contains("absent"));
    }

    #[test]
    fn ambiguous_bounded_dependency_lookup_rejects() {
        let ambiguous = format!(
            "{SEED}\n[[package]]\nname = \"shared\"\nversion = \"9.9.9\"\nsource = \"registry+other\"\nchecksum = \"bb\"\n"
        );
        let error = super::Closure::parse(ambiguous.as_bytes())
            .expect_err("name-only dependency cannot select an ambiguous indexed package");
        assert!(format!("{error}").contains("ambiguous"));
    }

    /// A path package and a git package of one name and version, as a
    /// `[patch]` of a git source leaves them: Cargo writes the path one bare
    /// and the git one with its source.
    #[test]
    fn a_bare_name_and_version_shared_with_a_sourced_package_names_the_path_package() {
        let git = "git+https://example.com/emporium.git?branch=master#82d4c650";
        let lock = format!(
            "version = 4\n\n\
             [[package]]\nname = \"rawr-iced\"\nversion = \"1.0.0\"\n\
             dependencies = [\"types 0.0.1\", \"shop\"]\n\n\
             [[package]]\nname = \"types\"\nversion = \"0.0.1\"\n\n\
             [[package]]\nname = \"types\"\nversion = \"0.0.1\"\nsource = \"{git}\"\n\n\
             [[package]]\nname = \"shop\"\nversion = \"0.1.0\"\nsource = \"{git}\"\n\
             dependencies = [\"types 0.0.1 (git+https://example.com/emporium.git?branch=master)\"]\n"
        );
        super::Closure::parse(lock.as_bytes()).expect("each spelling names one package");
    }

    #[test]
    fn context_and_whole_lock_change_complete_identity() {
        let first_lock = admit(definition(), SEED.as_bytes().into()).unwrap();
        let first = bind(&first_lock, 0, 1, 2);
        let augmented = format!(
            "{SEED}\n[[package]]\nname = \"project-only\"\nversion = \"9.0.0\"\nsource = \"registry+exact\"\nchecksum = \"bb\"\n"
        );
        let second_lock = admit(definition(), augmented.into_bytes().into_boxed_slice()).unwrap();
        let second = bind(&second_lock, 0, 1, 2);
        assert_eq!(first.base(), second.base());
        assert_ne!(first.complete(), second.complete());
    }

    #[test]
    fn compiler_proof_cohort_and_endpoint_table_change_universe_identity() {
        let first = definition();
        for (version, endpoint_table) in [(2, 1), (1, 2)] {
            let changed = Definition::new(
                super::ROOT_PACKAGE,
                Some(super::FRAMEWORK_PACKAGE),
                "fixture",
                version,
                endpoint_table,
                [7; 32],
                SEED.as_bytes(),
                "aarch64-apple-darwin",
                "dev",
                "rustc",
                "cargo",
            )
            .expect("changed compiler-proof cohort seals");

            assert_ne!(first.identity(), changed.identity());
        }
    }

    #[test]
    fn protected_closure_and_toolchain_facts_change_universe_identity() {
        const CHANGED_SEED: &str = "version = 4\n\n[[package]]\nname = \"rawr-iced\"\nversion = \"1.0.0\"\ndependencies = [\"shared\"]\n\n[[package]]\nname = \"shared\"\nversion = \"1.2.4\"\nsource = \"registry+exact\"\nchecksum = \"bb\"\n";

        let first = definition();
        let changed_closure = Definition::new(
            super::ROOT_PACKAGE,
            Some(super::FRAMEWORK_PACKAGE),
            "fixture",
            1,
            1,
            [7; 32],
            CHANGED_SEED.as_bytes(),
            "aarch64-apple-darwin",
            "dev",
            "rustc",
            "cargo",
        )
        .expect("changed protected closure seals");
        let changed_toolchain = Definition::new(
            super::ROOT_PACKAGE,
            Some(super::FRAMEWORK_PACKAGE),
            "fixture",
            1,
            1,
            [7; 32],
            SEED.as_bytes(),
            "aarch64-apple-darwin",
            "dev",
            "different-rustc",
            "cargo",
        )
        .expect("changed toolchain seals");

        assert_ne!(first.identity(), changed_closure.identity());
        assert_ne!(first.identity(), changed_toolchain.identity());
    }

    #[test]
    fn feature_and_unit_role_facts_change_completed_proof() {
        let final_lock =
            admit(definition(), SEED.as_bytes().into()).expect("authenticated lock admits");
        let first = bind(&final_lock, 0, 1, 2);
        let changed_features = bind(&final_lock, 0, 3, 2);
        let changed_roles = bind(&final_lock, 0, 1, 4);

        assert_eq!(first.base(), changed_features.base());
        assert_eq!(first.base(), changed_roles.base());
        assert_ne!(first.complete(), changed_features.complete());
        assert_ne!(first.complete(), changed_roles.complete());
    }

    #[test]
    fn accounted_bind_admits_exact_hash_bytes_after_prior_usage() {
        let lock = admit(definition(), SEED.as_bytes().into()).expect("fixture lock admits");
        let universe = lock.0.definition.identity;
        let metadata = metadata_proof(&lock.0.definition);
        let context = super::ContextProof {
            universe,
            digest: super::Digest([1; 32]),
        };
        let unit = super::UnitProof {
            universe,
            digest: super::Digest([2; 32]),
        };
        let build = super::BuildProof {
            universe,
            projection: super::Digest([3; 32]),
            input: super::Digest([4; 32]),
        };
        let domain = b"rawr.import.cargo.resolution-universe-proof/v1";
        let bytes = domain.len() + 7 * 32;
        let prior = 47;
        for limit in [prior + bytes, prior + bytes - 1] {
            let mut calls = 0;
            let mut used = prior;
            let result = lock.bind_accounted(context, &metadata, unit, build, |actual| {
                calls += 1;
                assert_eq!(actual, bytes);
                let next = used + actual;
                if next > limit {
                    return Err(crate::Error::Limit {
                        resource: "fixture completion hash bytes",
                        limit,
                        actual: next,
                    });
                }
                used = next;
                Ok(())
            });
            assert_eq!(calls, 1);
            if limit == prior + bytes {
                let proof = result.expect("the exact remaining hash budget admits");
                assert_eq!(used, limit);
                let legacy = lock
                    .bind(context, &metadata, unit, build)
                    .expect("legacy completion retains its proof bytes");
                assert_eq!(proof.complete(), legacy.complete());
                let mut expected = Sha256::new();
                expected.update(domain);
                for digest in [
                    universe,
                    lock.digest(),
                    context.digest,
                    metadata.digest,
                    unit.digest,
                    build.projection,
                    build.input,
                ] {
                    expected.update(digest.as_bytes());
                }
                assert_eq!(proof.complete(), super::Digest(expected.finalize().into()));
            } else {
                assert!(matches!(result, Err(crate::Error::Limit {
                    resource: "fixture completion hash bytes", actual, ..
                }) if actual == prior + bytes));
                assert_eq!(used, prior);
            }
        }
    }

    #[test]
    fn accounted_bind_rejects_each_foreign_identity_before_admission() {
        let lock = admit(definition(), SEED.as_bytes().into()).expect("fixture lock admits");
        let universe = lock.0.definition.identity;
        let foreign = super::Digest([0; 32]);
        assert_ne!(universe, foreign);
        for (index, field) in [
            "Cargo context proof",
            "protected metadata proof",
            "protected unit proof",
            "protected source/build proof",
        ]
        .into_iter()
        .enumerate()
        {
            let identity = |position| if position == index { foreign } else { universe };
            let mut metadata = metadata_proof(&lock.0.definition);
            metadata.universe = identity(1);
            let mut called = false;
            let result = lock.bind_accounted(
                super::ContextProof {
                    universe: identity(0),
                    digest: super::Digest([1; 32]),
                },
                &metadata,
                super::UnitProof {
                    universe: identity(2),
                    digest: super::Digest([2; 32]),
                },
                super::BuildProof {
                    universe: identity(3),
                    projection: super::Digest([3; 32]),
                    input: super::Digest([4; 32]),
                },
                |_| {
                    called = true;
                    Err(crate::Error::Limit {
                        resource: "unexpected hash admission",
                        limit: 0,
                        actual: 1,
                    })
                },
            );
            assert!(!called);
            assert!(
                matches!(result, Err(crate::Error::ResolutionDrift { field: actual, .. })
                if actual == field)
            );
        }
    }

    #[test]
    fn streamed_unit_projection_preserves_the_exact_canonical_schema() {
        let limits = Limits::standard();
        let metadata = super::super::unit::bounded_json(
            "streamed projection metadata fixture",
            AUTHENTICATED_METADATA.as_bytes(),
            &limits,
        )
        .expect("metadata fixture parses");
        let unit = super::super::unit::bounded_json(
            "streamed projection unit fixture",
            AUTHENTICATED_UNIT.as_bytes(),
            &limits,
        )
        .expect("unit fixture parses");
        let mut allocation = super::JsonAllocation::empty();
        allocation
            .charge_tree(&metadata)
            .expect("metadata DOM charges");
        let packages = super::metadata_packages_from_value(&metadata, &mut allocation)
            .expect("package facts derive");
        allocation.charge_tree(&unit).expect("unit DOM charges");
        let (reachable_units, _) =
            super::unit_reachability(&unit, packages.value(), &mut allocation)
                .expect("unit reachability derives");
        let projection = super::unit_projection(
            &unit,
            packages.value(),
            reachable_units.value(),
            &mut allocation,
        )
        .expect("streamed unit projection derives");
        assert_eq!(
            std::str::from_utf8(projection.value()).expect("projection is UTF-8 JSON"),
            "{\"units\":[{\"dependencies\":[],\"features\":[\"base\"],\"is_std\":false,\"mode\":\"build\",\"pkg_id\":\"886d85edf17bc9a89402bfd45334a90963f8179902804ceba5d77b7a7a98d5e2\",\"target\":{\"crate_types\":[\"rlib\"],\"kind\":[\"lib\"],\"src_path\":\"$SELF/src/lib.rs\"},\"unit\":\"fa330279f54c2e2424a0729ab5dc194c4619f9c19ca6bb2fed134b889a1a4cfa\"}],\"version\":1}"
        );
    }

    #[test]
    fn streamed_unit_projection_does_not_retain_every_normalized_unit_tree() {
        const COUNT: usize = 24;
        const LONG: usize = 4_096;
        let standard = Limits::standard();
        let metadata = super::super::unit::bounded_json(
            "streamed projection scale metadata",
            AUTHENTICATED_METADATA.as_bytes(),
            &standard,
        )
        .expect("metadata fixture parses");
        let long = "x".repeat(LONG);
        let units = (0..COUNT)
            .map(|index| {
                serde_json::json!({
                    "pkg_id": "path+file:///fixture/rawr-iced#1.0.0",
                    "target": {
                        "kind": ["lib"],
                        "crate_types": ["rlib"],
                        "src_path": "/fixture/rawr-iced/src/lib.rs",
                        "name": format!("{long}{index}")
                    },
                    "mode": "build",
                    "features": ["base"],
                    "dependencies": [],
                    "platform": format!("platform-{index}"),
                    "is_std": false
                })
            })
            .collect::<Vec<_>>();
        let unit = serde_json::json!({"version": 1, "units": units});

        let exercise = || {
            let mut allocation = super::JsonAllocation::empty();
            allocation.charge_tree(&metadata)?;
            let packages = super::metadata_packages_from_value(&metadata, &mut allocation)?;
            allocation.charge_tree(&unit)?;
            let (reachable_units, reachable_packages) =
                super::unit_reachability(&unit, packages.value(), &mut allocation)?;
            reachable_packages.drop_from(&mut allocation)?;
            let before = allocation.full_usage();
            let projection = super::unit_projection(
                &unit,
                packages.value(),
                reachable_units.value(),
                &mut allocation,
            )?;
            let peak = allocation.peak_usage();
            Ok::<_, crate::Error>((projection.value().to_vec(), before, peak))
        };

        let (canonical, before, peak) = exercise().expect("streamed scale projection seals");
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(&canonical)
                .expect("projection remains canonical JSON")["units"]
                .as_array()
                .expect("projected units remain an array")
                .len(),
            COUNT
        );
        let additional_strings = peak.2.saturating_sub(before.2);
        assert!(additional_strings < LONG.saturating_mul(4));
        assert!(additional_strings < LONG.saturating_mul(COUNT / 2));
    }

    #[test]
    fn proof_projection_shares_one_exact_epoch_with_both_owned_cargo_doms() {
        let standard = Limits::standard();
        let definition = definition();
        let metadata = super::super::unit::bounded_json(
            "already-owned metadata fixture",
            AUTHENTICATED_METADATA.as_bytes(),
            &standard,
        )
        .expect("metadata fixture parses once");
        let unit = super::super::unit::bounded_json(
            "already-owned unit fixture",
            AUTHENTICATED_UNIT.as_bytes(),
            &standard,
        )
        .expect("unit fixture parses once");

        let exercise = || {
            let mut shared = super::super::unit::JsonAllocation::empty();
            let metadata_ownership = super::JsonOwnership::tree(&metadata)?;
            shared.acquire(metadata_ownership)?;
            let metadata_dom = shared.full_usage();
            let observation = definition.observe_metadata_value(
                super::Budgeted::from_retained(metadata.clone(), metadata_ownership),
                None,
                &standard,
                &mut shared,
            )?;
            assert_eq!(
                shared.full_usage(),
                observation.live_ownership()?.usage(),
                "only the moved observation remains after the source DOM is consumed"
            );
            assert_ne!(shared.full_usage(), metadata_dom);
            shared.charge_tree(&unit)?;
            let unit_and_metadata_doms = shared.full_usage();
            let (_metadata, _unit) =
                definition.authenticate_value(&unit, observation, &standard, &mut shared)?;
            assert!(shared.peak_usage().0 > unit_and_metadata_doms.0);
            Ok::<_, crate::Error>(shared)
        };

        let measured = exercise().expect("standard epoch measures all live ownership");
        let peak = measured.peak_usage();
        assert!(peak.0 > 0 && peak.1 > 0 && peak.2 > 0 && peak.3 > 0);
    }

    #[test]
    fn owned_metadata_move_preserves_the_reference_projection_bytes() {
        let limits = Limits::standard();
        let reference_document = super::super::unit::bounded_json(
            "reference metadata fixture",
            AUTHENTICATED_METADATA.as_bytes(),
            &limits,
        )
        .expect("reference metadata parses");
        let mut reference_allocation = super::JsonAllocation::empty();
        let reference_packages =
            super::metadata_packages_from_value(&reference_document, &mut reference_allocation)
                .expect("reference packages derive");
        let reference_reachable = reference_packages
            .value()
            .keys()
            .cloned()
            .collect::<std::collections::BTreeSet<_>>();
        let (reference, _) = super::metadata_projection(
            &reference_document,
            reference_packages.value(),
            &reference_reachable,
        )
        .expect("clone-shaped reference projection derives");
        let reference =
            super::projection_bytes(reference).expect("reference projection serializes");

        for compact in [false, true] {
            let document = super::super::unit::bounded_json(
                "owned metadata fixture",
                AUTHENTICATED_METADATA.as_bytes(),
                &limits,
            )
            .expect("owned metadata parses");
            let mut allocation = super::JsonAllocation::empty();
            let document_ownership =
                super::JsonOwnership::tree(&document).expect("owned metadata measures");
            allocation
                .acquire(document_ownership)
                .expect("owned metadata charges once");
            let packages = super::metadata_packages_from_value(&document, &mut allocation)
                .expect("owned packages derive");
            let mut document = super::Budgeted::from_retained(document, document_ownership);
            let records = if compact {
                super::super::unit::compact_metadata_packages(&mut document, true, &mut allocation)
                    .expect("complete package records compact")
            } else {
                None
            };
            let observed = super::metadata_observation(
                document,
                records,
                packages.value(),
                &limits,
                &mut allocation,
            )
            .expect("owned metadata moves into the observation");
            let reachable_ownership = packages
                .value()
                .keys()
                .try_fold(super::JsonOwnership::collection(), |ownership, id| {
                    let _ = id;
                    ownership.checked_add(super::JsonOwnership::nodes(1))
                })
                .expect("reachable ownership measures");
            let reachable = allocation
                .retain_with(reachable_ownership, || {
                    Ok(packages
                        .value()
                        .keys()
                        .cloned()
                        .collect::<std::collections::BTreeSet<_>>())
                })
                .expect("reachable package ids retain");
            let (bytes, protected) = super::metadata_projection_from_observation(
                observed,
                packages.value(),
                reachable.value(),
                &limits,
                &mut allocation,
            )
            .expect("moved metadata projects");
            reachable
                .drop_from(&mut allocation)
                .expect("reachable ids release");
            packages
                .drop_from(&mut allocation)
                .expect("all-package authority releases");
            protected
                .drop_from(&mut allocation)
                .expect("protected-package authority releases");
            assert_eq!(bytes.value().as_ref(), reference.as_ref());
            bytes
                .drop_from(&mut allocation)
                .expect("projection bytes release");
            assert_eq!(allocation.full_usage(), (0, 0, 0, 0));
        }
    }

    #[test]
    fn unreachable_metadata_packages_release_before_the_projection_is_retained() {
        let limits = Limits::standard();
        let rawr_id = "path+file:///fixture/rawr-iced#1.0.0";
        let project_id = "path+file:///fixture/project-only#9.0.0";
        let document = serde_json::json!({
            "packages": [{
                "id": rawr_id,
                "name": "rawr-iced",
                "version": "1.0.0",
                "source": null,
                "manifest_path": "/fixture/rawr-iced/Cargo.toml",
                "dependencies": [],
                "targets": [{"kind": ["lib"], "src_path": "/fixture/rawr-iced/src/lib.rs"}]
            }, {
                "id": project_id,
                "name": "project-only",
                "version": "9.0.0",
                "source": null,
                "manifest_path": "/fixture/project-only/Cargo.toml",
                "description": "x".repeat(4_096),
                "dependencies": [],
                "targets": [{"kind": ["lib"], "src_path": "/fixture/project-only/src/lib.rs"}]
            }],
            "resolve": {"nodes": [{
                "id": rawr_id,
                "dependencies": [],
                "deps": [],
                "features": ["base"]
            }, {
                "id": project_id,
                "dependencies": [],
                "deps": [],
                "features": []
            }]}
        });
        let mut allocation = super::JsonAllocation::empty();
        let document_ownership =
            super::JsonOwnership::tree(&document).expect("two-package DOM measures");
        allocation
            .acquire(document_ownership)
            .expect("two-package DOM charges once");
        let packages = super::metadata_packages_from_value(&document, &mut allocation)
            .expect("two-package authority derives");
        let shared_rawr_id = Arc::clone(
            packages
                .value()
                .get_key_value(rawr_id)
                .expect("rawr package is retained")
                .0,
        );
        let reference_reachable = std::collections::BTreeSet::from([Arc::clone(&shared_rawr_id)]);
        let (reference, _) =
            super::metadata_projection(&document, packages.value(), &reference_reachable)
                .expect("reachable-only reference metadata projects");
        let reference = super::projection_bytes(reference)
            .expect("reachable-only reference metadata serializes");
        let observed = super::metadata_observation(
            super::Budgeted::from_retained(document, document_ownership),
            None,
            packages.value(),
            &limits,
            &mut allocation,
        )
        .expect("two-package observation derives");
        let before = allocation.full_usage();
        let reachable_ownership = super::JsonOwnership::collection()
            .checked_add(super::JsonOwnership::nodes(1))
            .expect("one reachable id measures");
        let reachable = allocation
            .retain_with(reachable_ownership, || {
                Ok(std::collections::BTreeSet::from([shared_rawr_id]))
            })
            .expect("one reachable id retains");
        let (projection, protected) = super::metadata_projection_from_observation(
            observed,
            packages.value(),
            reachable.value(),
            &limits,
            &mut allocation,
        )
        .expect("only reachable metadata projects");
        let exact_live = packages
            .ownership()
            .checked_add(reachable.ownership())
            .and_then(|ownership| ownership.checked_add(projection.ownership()))
            .and_then(|ownership| ownership.checked_add(protected.ownership()))
            .expect("retained projection ownership sums")
            .usage();
        assert_eq!(allocation.full_usage(), exact_live);
        assert_eq!(projection.value().as_ref(), reference.as_ref());
        assert!(
            allocation.full_usage().1 + 4_096 < before.1,
            "the unreachable project-only encoded row is no longer retained"
        );
        reachable
            .drop_from(&mut allocation)
            .expect("reachable ids release");
        packages
            .drop_from(&mut allocation)
            .expect("all-package authority releases");
        protected
            .drop_from(&mut allocation)
            .expect("protected-package authority releases");
        projection
            .drop_from(&mut allocation)
            .expect("metadata projection releases");
        assert_eq!(allocation.full_usage(), (0, 0, 0, 0));
    }

    #[test]
    fn canonical_projection_precharges_sort_keys_and_buffer() {
        let value = serde_json::json!([
            {"b": "second", "a": [2, 1]},
            {"b": "first", "a": [1, 2]}
        ]);
        let mut allocation = super::JsonAllocation::empty();
        let ownership = super::JsonOwnership::tree(&value).expect("tree ownership measures");
        let value = allocation
            .retain_with(ownership, || Ok(value))
            .expect("the tree retains");
        let bytes = super::projection_bytes_budgeted(value, &mut allocation)
            .expect("the canonical projection derives");
        assert_eq!(allocation.full_usage(), bytes.ownership().usage());
        assert!(
            allocation.peak_usage().0 > bytes.ownership().usage().0,
            "the sort-key scratch is charged before it is released"
        );
        let (bytes, ownership) = bytes.into_parts();
        allocation.release(ownership).expect("the output releases");
        assert_eq!(allocation.full_usage(), (0, 0, 0, 0));
        assert_eq!(
            bytes.as_ref(),
            br#"[{"a":[1,2],"b":"first"},{"a":[1,2],"b":"second"}]"#
        );
    }

    #[test]
    fn package_token_replacement_reuses_string_ownership() {
        let key = super::Key {
            name: "fixture".into(),
            version: "1.0.0".into(),
            source: "registry+https://example.invalid/index".into(),
        };
        let expected = super::key_token(&key);
        let mut value = serde_json::Value::String("x".repeat(128));
        let mut ownership = super::JsonOwnership::tree(&value).expect("old string measures");
        let mut allocation = super::JsonAllocation::empty();
        allocation
            .acquire(ownership)
            .expect("old string is charged");

        super::replace_json_key_token(&mut value, &key, &mut allocation, &mut ownership)
            .expect("shorter token reuses the existing string ownership");
        assert_eq!(value.as_str(), Some(expected.as_str()));
        assert_eq!(allocation.full_usage(), ownership.usage());
        assert_eq!(allocation.peak_usage().2, 128);

        let mut short = serde_json::Value::String("x".repeat(63));
        let mut short_ownership =
            super::JsonOwnership::tree(&short).expect("short string measures");
        let mut growing = super::JsonAllocation::empty();
        growing
            .acquire(short_ownership)
            .expect("short string is charged");
        super::replace_json_key_token(&mut short, &key, &mut growing, &mut short_ownership)
            .expect("the longer token charges its growth");
        assert_eq!(short.as_str(), Some(expected.as_str()));
        assert_eq!(growing.full_usage(), short_ownership.usage());
        assert_eq!(growing.full_usage().2, 64);
    }

    #[test]
    fn normalized_path_replacement_uses_bounded_byte_scratch_not_a_second_string() {
        let package = super::ProtectedPackage {
            key: super::Key {
                name: "fixture".into(),
                version: "1.0.0".into(),
                source: "".into(),
            },
            root: std::path::PathBuf::from("/fixture/package").into(),
        };
        let packages = std::collections::BTreeMap::from([(
            std::sync::Arc::<str>::from("fixture"),
            package.clone(),
        )]);
        let original = "/fixture/package/src/lib.rs";
        let expected = "$SELF/src/lib.rs";
        let mut value = serde_json::Value::String(original.into());
        let mut ownership = super::JsonOwnership::tree(&value).expect("old path measures");
        let mut allocation = super::JsonAllocation::empty();
        allocation.acquire(ownership).expect("old path is charged");

        super::rewrite_json_string_from_bytes(
            &mut value,
            expected.len(),
            &mut allocation,
            &mut ownership,
            |path| super::normalized_path_bytes(path, &package, &packages, expected.len()),
        )
        .expect("shorter normalized path reuses string ownership");
        assert_eq!(value.as_str(), Some(expected));
        assert_eq!(allocation.full_usage(), ownership.usage());
        assert_eq!(allocation.peak_usage().2, original.len());

        let mut short = serde_json::Value::String("src".into());
        let mut short_ownership =
            super::JsonOwnership::tree(&short).expect("relative path measures");
        let mut growing = super::JsonAllocation::empty();
        growing
            .acquire(short_ownership)
            .expect("raw relative path is charged");
        super::rewrite_json_string_from_bytes(
            &mut short,
            "$SELF/src".len(),
            &mut growing,
            &mut short_ownership,
            |path| super::normalized_path_bytes(path, &package, &packages, "$SELF/src".len()),
        )
        .expect("the longer normalized path charges its growth");
        assert_eq!(short.as_str(), Some("$SELF/src"));
        assert_eq!(growing.full_usage(), short_ownership.usage());
        assert_eq!(growing.full_usage().2, "$SELF/src".len());
    }

    #[test]
    fn budgeted_move_transfers_existing_ownership_without_a_second_charge() {
        let value = serde_json::json!({"owned": ["once"]});
        let ownership = super::JsonOwnership::tree(&value).expect("tree ownership measures");
        let mut allocation = super::JsonAllocation::empty();
        let budgeted = allocation
            .retain_with(ownership, || Ok(value))
            .expect("tree ownership retains");
        let before = allocation.full_usage();
        let (value, transferred) = budgeted.into_parts();
        let budgeted = super::Budgeted::from_retained(value, transferred);
        assert_eq!(allocation.full_usage(), before);
        budgeted
            .drop_from(&mut allocation)
            .expect("transferred ownership releases once");
        assert_eq!(allocation.full_usage(), (0, 0, 0, 0));
    }

    #[test]
    fn failed_observation_releases_only_its_owned_transformation() {
        let limits = Limits::standard();
        let definition = definition();

        let incomplete = serde_json::json!({
            "packages": [{
                "id": "path+file:///fixture/rawr-iced#1.0.0",
                "name": "rawr-iced",
                "version": "1.0.0",
                "source": null,
                "manifest_path": "/fixture/rawr-iced/Cargo.toml"
            }]
        });
        let mut allocation = super::JsonAllocation::empty();
        let incomplete_ownership =
            super::JsonOwnership::tree(&incomplete).expect("incomplete DOM ownership measures");
        allocation
            .acquire(incomplete_ownership)
            .expect("owned incomplete DOM charges once");
        assert!(definition
            .observe_metadata_value(
                super::Budgeted::from_retained(incomplete, incomplete_ownership),
                None,
                &limits,
                &mut allocation,
            )
            .is_err());
        assert_eq!(allocation.full_usage(), (0, 0, 0, 0));
    }

    #[test]
    fn duplicate_edges_discover_each_pending_unit_once() {
        let standard = Limits::standard();
        let metadata = super::super::unit::bounded_json(
            "duplicate-edge metadata fixture",
            AUTHENTICATED_METADATA.as_bytes(),
            &standard,
        )
        .expect("metadata fixture parses");
        let unit = serde_json::json!({
            "version": 1,
            "units": [{
                "pkg_id": "path+file:///fixture/rawr-iced#1.0.0",
                "target": {
                    "kind": ["lib"],
                    "crate_types": ["rlib"],
                    "src_path": "/fixture/rawr-iced/src/lib.rs"
                },
                "mode": "build",
                "features": ["base"],
                "dependencies": [
                    {"index": 0}, {"index": 0}, {"index": 0},
                    {"index": 0}, {"index": 0}, {"index": 0}
                ],
                "is_std": false
            }]
        });
        assert!(
            unit["units"][0]["dependencies"]
                .as_array()
                .expect("dependencies are an array")
                .len()
                > unit["units"].as_array().expect("units are an array").len()
        );

        let exercise = || {
            let mut allocation = super::JsonAllocation::empty();
            allocation.charge_tree(&metadata)?;
            let packages = super::metadata_packages_from_value(&metadata, &mut allocation)?;
            allocation.charge_tree(&unit)?;
            let (reachable_units, reachable_packages) =
                super::unit_reachability(&unit, packages.value(), &mut allocation)?;
            let projection = super::unit_projection(
                &unit,
                packages.value(),
                reachable_units.value(),
                &mut allocation,
            )?;
            let bytes = projection;
            reachable_units.drop_from(&mut allocation)?;
            reachable_packages.drop_from(&mut allocation)?;
            packages.drop_from(&mut allocation)?;
            let peak = allocation.peak_usage();
            let (bytes, ownership) = bytes.into_parts();
            allocation.release(ownership)?;
            Ok::<_, crate::Error>((bytes, peak))
        };

        let (canonical, peak) = exercise().expect("standard duplicate graph measures");
        assert_eq!(
            canonical
                .windows(b"\"unit\"".len())
                .filter(|window| *window == b"\"unit\"")
                .count(),
            7
        );
        assert!(peak.0 > 0, "the queue and projection were charged");

        let mut allocation = super::JsonAllocation::empty();
        let mut pending =
            super::PendingUnits::retain(1, &mut allocation).expect("one-unit queue reserves once");
        for _ in 0..16 {
            super::PendingUnits::enqueue(&mut pending, 0, &mut allocation)
                .expect("duplicate discovery is infallible");
        }
        assert_eq!(super::PendingUnits::peak(&pending), 1);
        assert_eq!(pending.value().bound, 1);
        assert_eq!(pending.value().pending.len(), 1);
        assert!(super::PendingUnits::peak(&pending) <= pending.value().bound);
        let discovered = super::PendingUnits::finish(pending, &mut allocation)
            .expect("queue transfers its discovered set");
        assert_eq!(discovered.value().len(), 1);
        discovered
            .drop_from(&mut allocation)
            .expect("discovered queue ownership releases");
        assert_eq!(allocation.full_usage(), (0, 0, 0, 0));
    }

    #[test]
    fn placeholder_observations_and_self_attested_receipts_cannot_mint_authority() {
        let limits = Limits::standard();
        let definition = definition();
        assert!(definition.observe_metadata(b"{}", &limits).is_err());

        let lock = admit(definition, SEED.as_bytes().into())
            .expect("authenticated lock admits for receipt fixture");
        let proof = bind(&lock, 1, 2, 3);
        let build = super::BuildProof {
            universe: lock.0.definition.identity,
            projection: super::Digest([9; 32]),
            input: super::Digest([10; 32]),
        };
        let mut receipt = super::Receipt::transform(
            super::Digest([4; 32]),
            &lock,
            super::Digest([5; 32]),
            super::Digest([6; 32]),
            super::Digest([7; 32]),
            super::Digest([8; 32]),
            build,
            proof,
        );
        receipt.metadata = Sha256::digest(b"self-attested metadata").into();
        receipt.unit = Sha256::digest(b"self-attested unit").into();
        receipt.complete = Sha256::digest(b"self-attested completion").into();
        let bytes = receipt
            .encode()
            .expect("forged receipt remains canonical JSON");
        let receipt = super::Receipt::parse(&bytes, &lock.0.definition)
            .expect("receipt parsing grants no final authority");

        assert!(receipt
            .revalidate(
                &lock,
                super::Digest([7; 32]),
                super::Digest([8; 32]),
                build,
                proof,
            )
            .is_err());
    }

    #[test]
    fn a_changed_post_build_projection_is_a_new_key_not_a_refusal() {
        let complete = |bytes: &[u8]| {
            let input = crate::input::Baseline::fixture_universe(bytes);
            let projection = input
                .universe_build()
                .expect("the fixture has a projection");
            let digest = *projection.digest().as_bytes();
            let completion = pending(definition()).complete(
                super::Digest([4; 32]),
                super::Digest([5; 32]),
                super::Digest([6; 32]),
                super::Digest([7; 32]),
                &input,
                &Limits::standard(),
            );
            (completion, digest)
        };
        let (first, first_digest) = complete(b"[4,[]]\n");
        let (second, second_digest) = complete(b"[4,[]]\nchanged");
        let (first, second) = (first.unwrap(), second.unwrap());
        assert_ne!(first.proof().complete(), second.proof().complete());
        let (first, second) = (first.receipt().unwrap(), second.receipt().unwrap());
        assert_eq!(first.build, first_digest);
        assert_eq!(second.build, second_digest);
        assert_ne!(first.build, second.build);
        assert_eq!(first.input, [0x51; 32]);
    }

    /// A runner with no host profile governs the `rawr` crate's closure
    /// and fixes no framework: a lock with any iced, or none, is admitted.
    #[test]
    fn the_plain_universe_governs_rawr_and_fixes_no_framework() {
        let plain = super::Catalog::plain().unwrap();
        assert!(plain.names_package("rawr"));
        assert!(!plain.names_package("iced"));
        let project = "version = 4\n\n[[package]]\nname = \"rawr\"\nversion = \"1.25.1\"\n\
                       dependencies = [\"itoa\"]\n\n[[package]]\nname = \"itoa\"\nversion = \"1.0.18\"\n\
                       source = \"registry+https://github.com/rust-lang/crates.io-index\"\n";
        admit(plain.current.clone(), project.as_bytes().into())
            .expect("a project's own resolution around rawr is admitted");
        let without_root = "version = 4\n\n[[package]]\nname = \"itoa\"\nversion = \"1.0.18\"\n";
        assert!(admit(plain.current.clone(), without_root.as_bytes().into()).is_err());
    }
}
