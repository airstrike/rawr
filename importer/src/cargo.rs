//! Final-form Cargo resolution under one sealed project/config authority.

use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsString;
use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, TryLockError};

use sha2::Digest as _;
use sha2::Sha256;
use toml_edit::{value, Array, DocumentMut, InlineTable, Item, Table, Value};

use crate::process::{self, Native, ProductionReplay, Program, ToolPaths};
use crate::{Error, Result};

pub mod config;
pub(crate) mod execution;
pub(crate) mod internal;
pub mod resolution;
pub mod toolchain;
pub mod unit;
#[doc(hidden)]
pub mod universe;
#[cfg(test)]
mod workspace_tests;

const HARNESS_DIRECTORY: &str = "resolution";
const PROVISIONAL_DIRECTORY: &str = "provisional";
const CANDIDATE_DIRECTORY: &str = "candidates";
const UNIVERSE_RECEIPT: &str = "resolution-universe.json";
const HARNESS_MANIFEST: &str = "Cargo.toml";
const HARNESS_SOURCE: &str = "#![forbid(unsafe_code)]\n\
    /// Keeps the final-form resolution package a normal library target.\n\
    pub fn rawr_resolution_harness() {}\n";
const HARNESS_PACKAGE: &str = "rawr-resolution-harness";
const HARNESS_VERSION: &str = "0.0.0";
const HARNESS_EDITION: &str = "2024";
/// The harness table that holds Rawr's own crate, which the generated
/// runner moves to its `[dependencies]`.
pub(crate) const BUILD_DEPENDENCIES: &str = "build-dependencies";
const PROFILE_DEFAULT: &str = "dev";
/// The standard Cargo JSON figure. It is distinct from the physical child
/// capture bound `process::CAPTURE_BYTES`, which refuses.
const JSON_BYTES: usize = 32 * 1024 * 1024;
const _: () = assert!(JSON_BYTES <= process::CAPTURE_BYTES);
const MANIFEST_BYTES: usize = 1024 * 1024;
const PACKAGES: usize = 4_096;
/// Physical bound on the directory entries swept when retiring stale
/// temporaries from a Rawr-owned resolution harness. Fixed at the 4,096 figure
/// (the standard package envelope) this sweep has always refused at; it is a
/// finite-work ceiling over a directory Rawr owns, not an input measure, so it
/// never follows the `packages` figure.
const HARNESS_ENTRIES: usize = 4_096;
const UNITS: usize = 500_000;
const EDGES: usize = 1_000_000;
const ITEMS: usize = 2_500_000;
const CANONICAL_ITEMS: usize = 19_000_000;
/// The canonical bytes figure the driver's plan carries; an accounting
/// figure, never a bound.
const CANONICAL_BYTES: usize = 1_207_959_552;
const INPUT_PROOF_BYTES: usize = 512 * 1024 * 1024;

const RETAINED_GRAPH_ITEMS: usize = 6_500_000;
const RETAINED_GRAPH_BYTES: usize = 512 * 1024 * 1024;
const CLASSIFIED_CONTRACT_ITEMS: usize = 6_500_000;
const CLASSIFIED_CONTRACT_BYTES: usize = 640 * 1024 * 1024;
const CONTAINERS: usize = 1_500_000;
const STRING_BYTES: usize = 1024 * 1024;
const DEPTH: usize = 256;
const LEASE_FILE: &str = ".resolution.lock";
const AUTHORITATIVE_TARGET_DIRECTORY: &str = "authoritative-target";
// 2: the build runs in the shared build directory and relocates what it reads.
const FINAL_COMMAND_SCHEMA: u32 = 2;

fn versioned_target_directory(harness_root: &Path) -> PathBuf {
    harness_root.join(AUTHORITATIVE_TARGET_DIRECTORY)
}

static TEMPORARY_ID: AtomicU64 = AtomicU64::new(1);
static HEAVY_OWNER: Mutex<()> = Mutex::new(());

/// Sole in-process authority for one heavyweight native Cargo operation.
pub struct Owner {
    _guard: std::sync::MutexGuard<'static, ()>,
}

#[cfg(test)]
std::thread_local! {
    static OWNED_AUTHORITIES: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// A SHA-256 content identity retained without exposing mutable bytes.
#[derive(Clone, Copy, Eq, Ord, PartialEq, PartialOrd)]
pub struct Digest([u8; 32]);

/// Frozen allocation and subprocess limits required by Cargo resolution.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limits {
    pub(crate) json_bytes: usize,
    pub(crate) manifest_bytes: usize,
    pub(crate) packages: usize,
    pub(crate) units: usize,
    pub(crate) edges: usize,
    pub(crate) items: usize,
    pub(crate) canonical_items: usize,
    pub(crate) canonical_bytes: usize,
    pub(crate) input_proof_bytes: usize,
    pub(crate) retained_graph_items: usize,
    pub(crate) retained_graph_bytes: usize,
    pub(crate) classified_contract_items: usize,
    pub(crate) classified_contract_bytes: usize,
    pub(crate) containers: usize,
    pub(crate) string_bytes: usize,
    pub(crate) depth: usize,
    process: process::Limits,
}

impl Limits {
    /// Return the aggregate retained-item ceiling for generated consumers.
    #[doc(hidden)]
    #[must_use]
    pub const fn item_limit(&self) -> usize {
        self.items
    }

    /// Return the aggregate retained-byte ceiling for generated consumers.
    #[doc(hidden)]
    #[must_use]
    pub const fn byte_limit(&self) -> usize {
        self.json_bytes
    }
}

impl Owner {
    /// Try to acquire the one native Cargo lane without waiting.
    pub fn acquire() -> Result<Self> {
        acquire_process_owner().map(|guard| Self { _guard: guard })
    }
}

/// A program's library closure for intrinsic inventories ([`Request::closure`]).
#[cfg(feature = "rustc-driver")]
#[derive(Default)]
pub struct Closure {
    /// Every library crate, declared by its crate name, and by name and
    /// version where two versions of one crate share the name.
    pub dependencies: Vec<config::Dependency>,
    /// Each alias the program declares, with the crate name it names.
    pub aliases: Vec<(Box<str>, Box<str>)>,
    /// Each crate's name, compiler unit and native input witness.
    pub units: Vec<(Box<str>, Box<str>, [u8; 32])>,
    /// Each compiler unit's machine-wide key, for a crate whose whole
    /// closure comes from registries and git commits (`extract::shared`).
    pub shared: BTreeMap<Box<str>, Box<str>>,
    /// The packages that run a build script in a shared unit's closure,
    /// for a unit with any: its entry is shared only where the project's
    /// build gives the outputs the nightly's did (#403).
    pub scripts: BTreeMap<Box<str>, Box<[Box<str>]>>,
    /// Where machine-wide entries live, under rawr's home.
    pub home: PathBuf,
}

/// The name each library of a closure is declared by, keyed by its package
/// id, from each library's crate name, package id and version. A crate name
/// no other library shares is its own name. Libraries that share one, as
/// rustc-hash 1 and 2 under iced do, are named by name and version, and
/// numbered after that where one version comes from two sources. No two
/// libraries get one name.
#[cfg(feature = "rustc-driver")]
fn closure_aliases<'a>(
    libraries: impl IntoIterator<Item = (&'a str, &'a str, &'a str)>,
) -> BTreeMap<&'a str, String> {
    let mut by_name = BTreeMap::<&str, Vec<(&str, &str)>>::new();
    for (name, package_id, version) in libraries {
        by_name.entry(name).or_default().push((package_id, version));
    }
    let mut taken = by_name
        .iter()
        .filter(|(_, packages)| packages.len() == 1)
        .map(|(name, _)| (*name).to_owned())
        .collect::<BTreeSet<_>>();
    let mut names = BTreeMap::new();
    for (name, mut packages) in by_name {
        if let [(package_id, _)] = packages.as_slice() {
            names.insert(*package_id, name.to_owned());
            continue;
        }
        packages.sort_unstable();
        for (package_id, version) in packages {
            let base = format!("{name}_{}", version.replace(['.', '-', '+'], "_"));
            let mut candidate = base.clone();
            let mut count = 1;
            while !taken.insert(candidate.clone()) {
                count += 1;
                candidate = format!("{base}_{count}");
            }
            names.insert(package_id, candidate);
        }
    }
    names
}

#[cfg(all(test, feature = "rustc-driver"))]
mod closure_alias_tests {
    use super::closure_aliases;

    /// iced's closure has two versions of rustc-hash, and a version can
    /// come from two sources; each library still gets its own name, and
    /// a name no other library shares stays the crate name.
    #[test]
    fn libraries_sharing_a_crate_name_get_distinct_names() {
        let names = closure_aliases([
            ("rustc_hash", "registry#rustc-hash@1.1.0", "1.1.0"),
            ("rustc_hash", "registry#rustc-hash@2.1.3", "2.1.3"),
            ("iced_core", "git#iced_core@0.14.0", "0.14.0"),
            ("lyon", "registry#lyon@1.0.1", "1.0.1"),
            ("lyon", "git#lyon@1.0.1", "1.0.1"),
            ("lyon_1_0_1", "registry#lyon_1_0_1@0.1.0", "0.1.0"),
        ]);
        assert_eq!(names["registry#rustc-hash@1.1.0"], "rustc_hash_1_1_0");
        assert_eq!(names["registry#rustc-hash@2.1.3"], "rustc_hash_2_1_3");
        assert_eq!(names["git#iced_core@0.14.0"], "iced_core");
        assert_eq!(names["git#lyon@1.0.1"], "lyon_1_0_1_2");
        assert_eq!(names["registry#lyon@1.0.1"], "lyon_1_0_1_3");
        assert_eq!(names["registry#lyon_1_0_1@0.1.0"], "lyon_1_0_1");
        let mut distinct = names.values().collect::<Vec<_>>();
        distinct.sort_unstable();
        distinct.dedup();
        assert_eq!(distinct.len(), names.len());
    }
}

/// Staged inputs that can only become a `Request` after canonical sealing.
pub struct Builder {
    inventory: bool,
    limits: Limits,
    project_root: PathBuf,
    cache_root: PathBuf,
    environment: Option<config::Environment>,
    cargo_authority: Option<PathBuf>,
    target: Option<Box<str>>,
    profile: Box<str>,
    dependencies: Vec<config::Dependency>,
    policy: Option<crate::extract::Policy>,
    universe: Option<universe::Catalog>,
    demand: std::collections::BTreeSet<DemandPath>,
}

/// One Rust path Rawr names, root alias first.
pub(crate) type DemandPath = Box<[Box<str>]>;

/// A canonical root/config/dependency/target/toolchain request proof.
pub struct Request {
    inventory: bool,
    #[cfg(feature = "rustc-driver")]
    inputs_only: bool,
    project_root: PathBuf,
    cache_root: PathBuf,
    authority: Authority,
    inputs: Box<[Input]>,
    config: config::Provenance,
    toolchain: toolchain::Requirement,
    target: Box<str>,
    profile: Box<str>,
    dependencies: Box<[config::Selected]>,
    policy: crate::extract::Policy,
    universe: Option<universe::Catalog>,
    parallelism: process::parallelism::Proof,
    internal: internal::Seed,
    /// The Rust paths Rawr names; a dependency they name is
    /// extracted from them instead of from every public path.
    demand: Box<[DemandPath]>,
}

/// A currently validated semantic resolution owner ready for API publication.
#[cfg(feature = "rustc-driver")]
pub struct Prepared {
    owner: resolution::owner::Resolved,
}

#[cfg(feature = "rustc-driver")]
impl Prepared {
    /// The shared tables this preparation used or extracted, to settle
    /// once the runner builds (`crate::bindings`).
    pub fn settlement(&mut self) -> crate::bindings::Settlement {
        match &mut self.owner {
            resolution::owner::Resolved::Owner(owner) => owner.settlement(),
            resolution::owner::Resolved::Shared(shared) => shared.settlement(),
        }
    }

    /// What a derive's expansion runs against in this resolution's build
    /// (#593): its target, its dependencies' directories and the selected
    /// crates' artifacts. `None` when shared bindings answered it, so no
    /// nightly build ran, or it selected no crate.
    pub fn expansion_basis(&self) -> Option<crate::expand::Basis> {
        match &self.owner {
            resolution::owner::Resolved::Owner(owner) => {
                Some(crate::expand::Basis::of(owner.expansion()))
                    .filter(|basis| !basis.externs.is_empty())
            }
            resolution::owner::Resolved::Shared(_) => None,
        }
    }

    /// Whether every selected crate replayed shared bindings, so no nightly
    /// build or probe ran.
    pub fn is_shared(&self) -> bool {
        matches!(self.owner, resolution::owner::Resolved::Shared(_))
    }

    /// Publish this currently validated owner through the extraction API node.
    pub fn publish(self, limits: &Limits) -> Result<crate::api::Graph<crate::api::Extracted>> {
        match self.owner {
            resolution::owner::Resolved::Owner(owner) => owner.publish(limits),
            resolution::owner::Resolved::Shared(_) => {
                unreachable!("shared bindings publish through the retained API, never `publish`")
            }
        }
    }

    /// Retain API data for audited classification before its current consumer union.
    pub fn retain_api(self, limits: &Limits) -> Result<crate::api::Continuation> {
        match self.owner {
            resolution::owner::Resolved::Owner(owner) => owner.retain_api(limits),
            resolution::owner::Resolved::Shared(shared) => (*shared).retain_api(limits),
        }
    }
}

/// The immutable final-form manifest, lock, config, metadata, and unit proof.
pub struct Resolution {
    context: Arc<crate::context::Context>,
    project_root: PathBuf,
    cache_root: PathBuf,
    retained_inputs: Box<[Input]>,
    retained_config: config::Provenance,
    harness_root: PathBuf,
    manifest: Digest,
    source: Digest,
    lock: Digest,
    authority: Digest,
    config: Digest,
    metadata: unit::Metadata,
    cargo_allocation: unit::Allocation,
    selected: Box<[Selection]>,
    build_unit: unit::BuildGraph,
    toolchain: toolchain::Proof,
    target: Box<str>,
    profile: Box<str>,
    policy: crate::extract::Policy,
    input: crate::input::Baseline,
    parallelism: process::parallelism::Proof,
    internal: internal::Recipe,
    #[cfg(test)]
    observations: Option<Retained>,
    universe: Option<universe::Proof>,
    #[cfg(feature = "rustc-driver")]
    dag: Option<DagResolution>,
    #[cfg(feature = "rustc-driver")]
    capture: Option<crate::extract::plan::Capture>,
    /// Each library unit's machine-wide key, for a closure probe: set only
    /// for a unit whose whole closure is immutable (`extract::shared`), with
    /// the packages in it that run a build script.
    #[cfg(feature = "rustc-driver")]
    closure_keys: BTreeMap<Box<str>, ClosureKey>,
    /// The nightly build's build-script outputs by package, for a capture
    /// of intrinsic facts: an entry whose closure runs a build script is
    /// shared only where the project's build gives the same (#403).
    #[cfg(feature = "rustc-driver")]
    nightly_scripts: crate::applicability::store::Scripts,
}

/// A unit's machine-wide key and the packages in its closure that run a
/// build script.
#[cfg(feature = "rustc-driver")]
type ClosureKey = (Box<str>, Box<[Box<str>]>);

/// Completed observations without consuming the request that must validate reuse.
struct CompletedHarness {
    context: Arc<crate::context::Context>,
    harness_root: PathBuf,
    manifest: Digest,
    source: Digest,
    lock: Digest,
    metadata: unit::Metadata,
    cargo_allocation: unit::Allocation,
    selected: Box<[Selection]>,
    build_unit: unit::BuildGraph,
    toolchain: toolchain::Proof,
    input: crate::input::Baseline,
    internal: internal::Recipe,
    observations: Option<Retained>,
    universe: Option<universe::Proof>,
    #[cfg(feature = "rustc-driver")]
    capture: Option<crate::extract::plan::Capture>,
    /// The shared tables this extraction leaves to publish.
    #[cfg(feature = "rustc-driver")]
    settlement: Vec<crate::bindings::Settled>,
    #[cfg(feature = "rustc-driver")]
    closure_units: Option<Box<[u8]>>,
    #[cfg(feature = "rustc-driver")]
    nightly_scripts: crate::applicability::store::Scripts,
}

impl CompletedHarness {
    fn into_resolution(self, request: Request) -> Resolution {
        #[cfg(feature = "rustc-driver")]
        let closure_keys = match &self.closure_units {
            Some(units) => {
                let selected = self
                    .build_unit
                    .artifacts()
                    .iter()
                    .filter_map(|output| {
                        Some(crate::extract::shared::Selected {
                            unit_key: output.semantic_key(),
                            package_id: output.package_id(),
                            target: output.target().to_str()?,
                            features: output.features(),
                        })
                    })
                    .collect::<Vec<_>>();
                crate::extract::shared::keys(
                    units,
                    &self.metadata,
                    &selected,
                    self.toolchain.rustc_commit(),
                    self.toolchain.host(),
                    &request.target,
                )
                .into_iter()
                // A closure that runs a build script may give the nightly
                // other outputs than the project's own build: its entry is
                // shared only where they agree (#399, #403).
                .map(|(unit, shared)| (unit, (shared.key, shared.scripts)))
                .collect()
            }
            _ => BTreeMap::new(),
        };
        Resolution {
            context: self.context,
            project_root: request.project_root,
            cache_root: request.cache_root,
            retained_inputs: request.inputs,
            authority: request.authority.digest,
            config: request.config.digest(),
            retained_config: request.config,
            harness_root: self.harness_root,
            manifest: self.manifest,
            source: self.source,
            lock: self.lock,
            selected: self.selected,
            metadata: self.metadata,
            cargo_allocation: self.cargo_allocation,
            build_unit: self.build_unit,
            toolchain: self.toolchain,
            target: request.target,
            profile: request.profile,
            policy: request.policy,
            input: self.input,
            parallelism: request.parallelism,
            internal: self.internal,
            #[cfg(test)]
            observations: self.observations,
            universe: self.universe,
            #[cfg(feature = "rustc-driver")]
            dag: None,
            #[cfg(feature = "rustc-driver")]
            capture: self.capture,
            #[cfg(feature = "rustc-driver")]
            closure_keys,
            #[cfg(feature = "rustc-driver")]
            nightly_scripts: self.nightly_scripts,
        }
    }
}

struct Harness {
    context: Arc<crate::context::Context>,
    harness_root: PathBuf,
    manifest: Digest,
    source: Digest,
    lock: Digest,
    metadata: unit::Metadata,
    workspace: crate::input::workspace::Image,
    cargo_allocation: unit::Allocation,
    selected: Box<[Selection]>,
    toolchain: toolchain::Proof,
    internal: internal::Recipe,
    documents: Option<Documents>,
    universe: Option<universe::Pending>,
    initial: unit::BuildGraph,
    command_target: PathBuf,
    #[cfg(feature = "rustc-driver")]
    dag: Option<DagHarness>,
    #[cfg(feature = "rustc-driver")]
    extraction_plan: crate::extract::plan::Plan,
    /// A closure probe's unit graph, for its crates' machine-wide keys.
    #[cfg(feature = "rustc-driver")]
    closure_units: Option<Box<[u8]>>,
}

/// A prepared harness whose ordinary Build succeeded.
#[cfg(feature = "rustc-driver")]
struct Built {
    harness: Harness,
    build: crate::input::build::Build,
}

struct Documents {
    metadata: unit::Transport,
    units: unit::Transport,
}

/// A borrowed pair from one cold observation bundle or one framed historical record.
struct DocumentBytes<'bytes> {
    metadata: &'bytes [u8],
    units: &'bytes [u8],
}

impl<'bytes> DocumentBytes<'bytes> {
    fn from_record(record: &'bytes crate::input::stored::Record<'_>) -> Self {
        Self {
            metadata: record.metadata(),
            units: record.units(),
        }
    }
}

impl Documents {
    fn new(metadata: unit::Transport, units: unit::Transport) -> Self {
        Self { metadata, units }
    }
}

/// One complete cold observation bundle; none of its fields is a current receipt.
struct Retained {
    documents: Documents,
    build: crate::input::build::Retained,
}

struct CachedToolchain {
    context: crate::context::Context,
    attestation: std::sync::Arc<crate::node::Validated<toolchain::Attestation>>,
}

#[cfg(feature = "rustc-driver")]
struct DagHarness {
    context: crate::context::Context,
    request: Digest,
    unit: crate::node::Validated<crate::cargo::resolution::UnitGraph>,
}

#[cfg(feature = "rustc-driver")]
pub(crate) struct DagResolution {
    pub(crate) context: crate::context::Context,
    pub(crate) request: Digest,
    pub(crate) build: crate::node::Validated<crate::cargo::resolution::BuildProduct>,
}

struct Placement {
    root: PathBuf,
    request: Digest,
    state: PlacementState,
    exclusive: bool,
}

// Persisted is the usual state; boxing it would allocate per placement to shrink a local.
#[allow(clippy::large_enum_variant)]
enum PlacementState {
    Fresh,
    Persisted {
        lock: universe::Candidate,
        receipt: universe::Receipt,
    },
}

/// The three observations required while materializing a fresh versioned harness.
enum MetadataPhase {
    Provisional,
    Candidate,
}

struct Materialized {
    bytes: Box<[u8]>,
    internal: internal::Recipe,
    lock: universe::Candidate,
}

/// One exact alias-to-package identity and its canonical final-runner roles.
#[derive(Clone, Eq, PartialEq)]
pub struct Selection {
    alias: Box<str>,
    package_id: Box<str>,
    roles: config::Roles,
}

#[derive(Debug)]
struct Authority {
    resolver: Box<str>,
    retained: BTreeMap<Box<str>, Item>,
    digest: Digest,
    inputs: Box<[Input]>,
    /// The project's own `Cargo.lock`, which starts the harness resolution
    /// so Rawr's units are the ones `cargo build` already compiled.
    lock: Option<Box<[u8]>>,
}

#[derive(Debug)]
pub(crate) struct Input {
    path: PathBuf,
    digest: Option<Digest>,
    role: Role,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
enum Role {
    Manifest,
    Lock,
}

impl Input {
    pub(crate) fn path(&self) -> &Path {
        &self.path
    }

    pub(crate) fn digest(&self) -> Option<&[u8; 32]> {
        self.digest.as_ref().map(Digest::as_bytes)
    }

    pub(crate) fn manifest(&self) -> Option<&Path> {
        (self.role == Role::Manifest).then_some(self.path())
    }
}

impl Digest {
    pub(crate) const fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    /// Borrow the exact 32-byte content identity.
    pub const fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

impl fmt::Debug for Request {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        struct Aliases<'request>(&'request [config::Selected]);
        impl fmt::Debug for Aliases<'_> {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter
                    .debug_list()
                    .entries(self.0.iter().map(config::Selected::alias))
                    .finish()
            }
        }
        formatter
            .debug_struct("Request")
            .field("project_root", &self.project_root)
            .field("cache_root", &self.cache_root)
            .field("authority", &self.authority.digest)
            .field("config", &self.config.digest())
            .field("target", &self.target)
            .field("profile", &self.profile)
            .field("dependency_aliases", &Aliases(&self.dependencies))
            .finish()
    }
}

impl fmt::Debug for Resolution {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Resolution")
            .field("harness_root", &self.harness_root)
            .field("manifest", &self.manifest)
            .field("source", &self.source)
            .field("lock", &self.lock)
            .field("authority", &self.authority)
            .field("config", &self.config)
            .field("metadata", &self.metadata.digest())
            .field("selected_count", &self.selected.len())
            .field("build_unit", &self.build_unit.digest())
            .field("toolchain", &self.toolchain)
            .field("target", &self.target)
            .field("profile", &self.profile)
            .finish()
    }
}

impl fmt::Debug for Digest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, formatter)
    }
}

impl fmt::Display for Digest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        for byte in self.0 {
            write!(formatter, "{byte:02x}")?;
        }
        Ok(())
    }
}

impl Limits {
    /// The frozen Phase-1 resolution envelope.
    pub const fn standard() -> Self {
        Self {
            json_bytes: JSON_BYTES,
            manifest_bytes: MANIFEST_BYTES,
            packages: PACKAGES,
            units: UNITS,
            edges: EDGES,
            items: ITEMS,
            canonical_items: CANONICAL_ITEMS,
            canonical_bytes: CANONICAL_BYTES,
            input_proof_bytes: INPUT_PROOF_BYTES,
            retained_graph_items: RETAINED_GRAPH_ITEMS,
            retained_graph_bytes: RETAINED_GRAPH_BYTES,
            classified_contract_items: CLASSIFIED_CONTRACT_ITEMS,
            classified_contract_bytes: CLASSIFIED_CONTRACT_BYTES,
            containers: CONTAINERS,
            string_bytes: STRING_BYTES,
            depth: DEPTH,
            process: process::Limits::capture(),
        }
    }

    pub(crate) const fn process_limits(self) -> process::Limits {
        self.process
    }

    pub(crate) fn bind_node_context(&self, digest: &mut Sha256) {
        // Only the physical bounds that still refuse bind node identity.
        digest.update(b"rawr.import.cargo.limits/v3");
        for value in [
            self.depth as u64,
            self.process.stdout() as u64,
            self.process.stderr() as u64,
        ] {
            digest.update(value.to_le_bytes());
        }
    }

    #[cfg(test)]
    pub(crate) const fn with_depth(self, depth: usize) -> Self {
        Self { depth, ..self }
    }

    #[cfg(test)]
    pub(crate) const fn fixture(json_bytes: usize, items: usize, string_bytes: usize) -> Self {
        Self {
            json_bytes,
            items,
            string_bytes,
            ..Self::standard()
        }
    }
}

impl Default for Limits {
    fn default() -> Self {
        Self::standard()
    }
}

/// Format into a string reserved at its exact length; a failed reservation
/// is an allocation failure.
pub(crate) fn formatted(arguments: fmt::Arguments<'_>) -> Result<String> {
    struct Counter(usize);
    impl fmt::Write for Counter {
        fn write_str(&mut self, value: &str) -> fmt::Result {
            self.0 = self.0.checked_add(value.len()).ok_or(fmt::Error)?;
            Ok(())
        }
    }
    let mut counter = Counter(0);
    fmt::write(&mut counter, arguments).map_err(|_| Error::Node {
        operation: "request formatted length",
        source: Box::new(crate::node::Error::Allocation),
    })?;
    let mut value = String::new();
    value
        .try_reserve_exact(counter.0)
        .map_err(|source| Error::Allocation {
            operation: "request formatted string",
            source,
        })?;
    fmt::write(&mut value, arguments).expect("formatting into a `String` never fails");
    Ok(value)
}

impl Request {
    /// Begin a request whose loose fields must still be promoted by `build`.
    pub fn builder(
        project_root: &Path,
        project_cache_root: &Path,
        limits: &Limits,
    ) -> Result<Builder> {
        Ok(Builder {
            inventory: false,
            limits: *limits,
            project_root: project_root.to_path_buf(),
            cache_root: project_cache_root.to_path_buf(),
            environment: None,
            cargo_authority: None,
            target: None,
            profile: PROFILE_DEFAULT.into(),
            dependencies: Vec::new(),
            policy: None,
            universe: None,
            demand: std::collections::BTreeSet::new(),
        })
    }

    /// Consume the sealed request into one exact Cargo resolution proof.
    pub fn resolve(self, native: &Native, limits: &Limits) -> Result<Resolution> {
        self.resolve_with(native, limits, None)
    }

    /// Capture only normal-build input authority for a consuming application.
    /// No metadata probe is scheduled and this result cannot publish an API.
    #[cfg(feature = "rustc-driver")]
    pub(crate) fn native_inputs(mut self, native: &Native, limits: &Limits) -> Result<Resolution> {
        assert!(
            !self.inventory && self.demand.is_empty(),
            "native input capture never also captures API facts"
        );
        self.inputs_only = true;
        self.resolve(native, limits)
    }

    /// Every library crate this request's build reaches, as an exact
    /// dependency at its resolved source, version and features, declared by
    /// its crate name; the alias each declared dependency had; and each
    /// crate's native input witness. An intrinsic inventory declares the
    /// whole closure, so each crate that can implement a trait has its own
    /// complete partition (D650.5), and a crate's witness keys its shared
    /// entry before any nightly build (D609.3). This runs the normal
    /// resolution only: no metadata probe.
    #[cfg(feature = "rustc-driver")]
    pub fn closure(mut self, native: &Native, limits: &Limits) -> Result<Closure> {
        self.inventory = false;
        self.demand = Box::default();
        self.inputs_only = true;
        let resolution = self.resolve(native, limits)?;
        let metadata = &resolution.metadata;
        let declared = metadata.declared_packages();
        let root = metadata.root_package_id();
        let mut seen = BTreeSet::new();
        let mut closure = Closure::default();
        // Each library built for the program's target. A library only a
        // build script or proc macro links is built for the host, and
        // declaring it would build it a second time with the same key.
        let libraries = resolution.build_unit.artifacts().iter().filter(|output| {
            let kinds = output.target_kinds();
            output.platform().is_some()
                && kinds
                    .iter()
                    .any(|kind| matches!(kind.as_ref(), "lib" | "rlib"))
                && !kinds.iter().any(|kind| kind.as_ref() == "proc-macro")
                && Some(output.package_id()) != root
        });
        let mut units = Vec::new();
        for output in libraries {
            let package_id = output.package_id();
            if !seen.insert(package_id) {
                continue;
            }
            let package = metadata
                .package(package_id)
                .expect("each unit Cargo builds belongs to a package of its metadata");
            units.push((output, package));
        }
        let aliases = closure_aliases(units.iter().map(|(output, package)| {
            (output.crate_name(), output.package_id(), package.version())
        }));
        for (output, package) in units {
            let package_id = output.package_id();
            let name = output.crate_name();
            let alias = aliases[package_id].as_str();
            let dependency = match package.source() {
                unit::PackageSource::Path { .. } => {
                    config::Dependency::path(alias, package.root())?
                }
                unit::PackageSource::Registry { cargo_id, .. }
                    if cargo_id
                        .starts_with("registry+https://github.com/rust-lang/crates.io-index")
                        || cargo_id.starts_with("sparse+https://index.crates.io/") =>
                {
                    config::Dependency::registry(alias, &format!("={}", package.version()))?
                }
                unit::PackageSource::Git { cargo_id, .. } => {
                    // The same source id as the project's, so the project's
                    // `Cargo.lock` pins the same commit.
                    let (url, reference) = config::GitReference::parse_source(cargo_id)
                        .expect("Cargo names each git source `git+<url>`");
                    config::Dependency::git(alias, url, reference)?
                }
                unit::PackageSource::Registry { cargo_id, .. } => {
                    return Err(Error::Request {
                        reason: format!(
                            "the crate `{} v{}` comes from the registry `{cargo_id}`, and rawr \
                             reads crates from crates.io, git and paths only",
                            package.name(),
                            package.version(),
                        )
                        .into_boxed_str(),
                    });
                }
            }
            .package(package.name())?
            .features(output.features().iter().map(AsRef::as_ref), false)?;
            closure.dependencies.push(dependency);
            for (alias, declared) in &declared {
                if *declared == package_id {
                    closure.aliases.push(((*alias).into(), name.into()));
                }
            }
            let witness = resolution
                .input
                .partition(output.semantic_key())
                .expect("an input-only resolution witnesses each library it builds");
            closure
                .units
                .push((name.into(), output.semantic_key().into(), witness));
        }
        closure.aliases.sort_unstable();
        closure.units.sort_unstable();
        for (unit, (key, scripts)) in &resolution.closure_keys {
            if !closure
                .units
                .iter()
                .any(|(_, candidate, _)| candidate == unit)
            {
                continue;
            }
            closure.shared.insert(unit.clone(), key.clone());
            if !scripts.is_empty() {
                closure.scripts.insert(unit.clone(), scripts.clone());
            }
        }
        closure.home = crate::store::home(resolution.context.cargo_home()).join("inventories");
        Ok(closure)
    }

    /// Prepare the complete semantic owner before extraction, reusing an
    /// attested bundle when all current inputs still agree.
    #[cfg(feature = "rustc-driver")]
    pub fn prepare_cached(self, limits: &Limits) -> Result<Prepared> {
        self.prepare_cached_in(
            limits,
            &crate::supervisor::snapshot::attempt::Scope::begin(),
        )
    }

    /// Prepare or fall back within the original reload attempt's cumulative
    /// validation budget, including all prior resident snapshot checks.
    #[cfg(feature = "rustc-driver")]
    pub fn prepare_cached_in(
        self,
        limits: &Limits,
        scope: &crate::supervisor::snapshot::attempt::Scope,
    ) -> Result<Prepared> {
        assert!(
            !self.inventory,
            "an intrinsic acquisition resolves uncached; demand replay is not complete"
        );
        resolution::owner::prepare_cached_in(self, limits, scope, &crate::metrics::Current)
            .map(|owner| Prepared { owner })
            .map_err(|source| Error::Node {
                operation: "complete resolution owner",
                source: Box::new(source),
            })
    }

    /// Resolve through the shared toolchain node before any tool discovery or
    /// version child is launched. Evidence-only callers retain [`Self::resolve`].
    pub fn resolve_cached(self, limits: &Limits) -> Result<Resolution> {
        assert!(
            !self.inventory,
            "an intrinsic acquisition resolves uncached; demand replay is not complete"
        );
        if self.universe.is_some() {
            let provisional_manifest = render_manifest(&self, None)?;
            if let Some(catalog) = self.universe.as_ref() {
                let _placement = select_placement(&self, catalog, &provisional_manifest)?;
            }
        }
        let context = crate::context::Context::capture_config(
            &self.project_root,
            &self.config,
            &self.target,
            limits,
        )
        .map_err(|error| Error::Request {
            reason: error.to_string().into_boxed_str(),
        })?;
        let attestation =
            toolchain::cached(&context, &crate::metrics::Current).map_err(|source| {
                Error::Node {
                    operation: "pinned toolchain",
                    source: Box::new(source),
                }
            })?;
        let native = Native::from_attestation(attestation.as_ref());
        let cached = CachedToolchain {
            context,
            attestation,
        };
        self.resolve_with(&native, limits, Some(&cached))
    }

    /// The pinned native toolchain this request's project uses, through the
    /// shared toolchain node: no discovery when its attestation is current.
    #[cfg(feature = "rustc-driver")]
    pub fn pinned_native(&self, limits: &Limits) -> Result<Native> {
        let context = crate::context::Context::capture_config(
            &self.project_root,
            &self.config,
            &self.target,
            limits,
        )
        .map_err(|error| Error::Request {
            reason: error.to_string().into_boxed_str(),
        })?;
        let attestation =
            toolchain::cached(&context, &crate::metrics::Current).map_err(|source| {
                Error::Node {
                    operation: "pinned toolchain",
                    source: Box::new(source),
                }
            })?;
        Ok(Native::from_attestation(attestation.as_ref()))
    }

    fn resolve_with(
        self,
        native: &Native,
        limits: &Limits,
        cached: Option<&CachedToolchain>,
    ) -> Result<Resolution> {
        let native = native.production();
        let _process_owner = acquire_process_owner()?;
        let mut lease = open_lease(&self)?;
        let _lease_owner = acquire_lease(
            &mut lease,
            &self.cache_root.join(LEASE_FILE),
            "acquire resolution cache lease",
        )?;
        let harness = self.prepare_harness(&native, limits, limits, cached)?;
        self.resolve_under_epoch(&native, harness, limits)
    }

    /// Enter the private nonpublishing lane while retaining production authority.
    #[cfg(feature = "rustc-driver")]
    pub(crate) fn resolve_evidence(
        self,
        native: &Native,
        limits: &Limits,
        evidence: &process::EvidenceSession,
    ) -> Result<Resolution> {
        let native = native.evidence(evidence);
        let _process_owner = acquire_process_owner()?;
        let mut lease = open_lease(&self)?;
        let _lease_owner = acquire_lease(
            &mut lease,
            &self.cache_root.join(LEASE_FILE),
            "acquire resolution cache lease",
        )?;
        let harness = self.prepare_harness(&native, limits, limits, None)?;
        self.resolve_under_epoch(&native, harness, limits)
    }

    fn prepare_harness<N: ToolPaths>(
        &self,
        native: &N,
        limits: &Limits,
        cargo_limits: &Limits,
        cached: Option<&CachedToolchain>,
    ) -> Result<Harness> {
        let mut cargo_allocation = unit::Allocation::begin(cargo_limits);
        let node_context = Arc::new(match cached {
            Some(cached) => cached.context.clone(),
            None => crate::context::Context::capture_config(
                &self.project_root,
                &self.config,
                &self.target,
                limits,
            )
            .map_err(|error| Error::Request {
                reason: error.to_string().into_boxed_str(),
            })?,
        });
        let versioned = self
            .universe
            .clone()
            .map(|catalog| {
                let provisional_manifest = render_manifest(self, None)?;
                let placement = select_placement(self, &catalog, &provisional_manifest)?;
                Ok::<_, Error>((catalog, placement))
            })
            .transpose()?;
        let toolchain = match cached {
            Some(cached) => cached.attestation.value().proof().clone(),
            None => {
                let environment = self.config.command_environment(
                    &self.cache_root.join(HARNESS_DIRECTORY).join("target"),
                    native,
                )?;
                execution::Scope::bind(
                    &node_context,
                    execution::Operation::Toolchain,
                    self.config.digest().0,
                )
                .run(|| {
                    self.toolchain.verify(
                        native,
                        &self.project_root,
                        environment,
                        limits.process,
                        &self.target,
                    )
                })?
            }
        };
        self.revalidate(limits, "toolchain verification")?;

        if let Some((catalog, placement)) = versioned {
            let definition = catalog.current().clone();
            let context = definition.validate_context(
                &self.target,
                &self.profile,
                toolchain.rustc_commit(),
                toolchain.cargo_commit(),
                self.config.projection_schema(),
                self.toolchain.schemas().graph(),
                self.toolchain.schemas().unit(),
                final_command_cohort(
                    self.toolchain.schemas().graph(),
                    &self.target,
                    &self.profile,
                    self.parallelism,
                    self.config.explicit_configs(),
                )?,
                self.config.universe_context(),
                self.config.digest(),
            )?;
            return self.prepare_versioned_harness(
                native,
                limits,
                cargo_limits,
                cargo_allocation,
                toolchain,
                catalog,
                context,
                placement,
                node_context,
                cached.map(|cached| cached.attestation.as_ref()),
            );
        }

        execution::Scope::bind(
            &node_context,
            execution::Operation::Harness,
            digest(render_manifest(self, None)?.as_bytes()).0,
        )
        .run(|| {
            let harness_root = prepare_harness(self)?;
            let target_dir = harness_root.join("target");

            let manifest_path = harness_root.join(HARNESS_MANIFEST);
            let metadata_bytes =
                cargo_metadata(native, self, &manifest_path, &target_dir, false, limits)?;
            self.revalidate(limits, "initial metadata")?;
            let provisional_metadata = unit::Metadata::parse(
                &metadata_bytes,
                &self.dependencies,
                cargo_limits,
                self.toolchain.schemas().graph(),
                &target_dir,
                &mut cargo_allocation,
            )?;
            let provisional_selected =
                provisional_metadata.selections(&self.dependencies, &mut cargo_allocation)?;
            let internal = self
                .internal
                .seal(&provisional_metadata, &provisional_selected)?;
            finalize_harness(self, &harness_root, &internal)?;
            drop(provisional_selected);
            drop(provisional_metadata);
            // The final metadata and the unit graph name the Build's target
            // directory, where its artifacts land.
            #[cfg(feature = "rustc-driver")]
            let initial_target = harness_root.join(AUTHORITATIVE_TARGET_DIRECTORY);
            #[cfg(not(feature = "rustc-driver"))]
            let initial_target = target_dir.clone();
            let metadata_bytes =
                cargo_metadata(native, self, &manifest_path, &initial_target, false, limits)?;
            self.revalidate(limits, "final metadata")?;
            let metadata = unit::Metadata::parse(
                &metadata_bytes,
                &self.dependencies,
                cargo_limits,
                self.toolchain.schemas().graph(),
                &initial_target,
                &mut cargo_allocation,
            )?;
            let lock_path = harness_root.join("Cargo.lock");
            let lock_bytes = read_bounded("harness lock", &lock_path, process::CAPTURE_BYTES)?;
            let metadata = metadata.bind_lock(&lock_bytes, cargo_limits, &mut cargo_allocation)?;
            let selected = metadata.selections(&self.dependencies, &mut cargo_allocation)?;
            internal.revalidate(&metadata, &selected)?;
            let lock = digest(&lock_bytes);
            // File reads are I/O retention bounded by a physical
            // constant. Manifest-class reads
            // (Cargo.toml, Rawr manifests, Cargo config, harness sources) bind
            // to the diagnostic tail figure `TAIL_BYTES`, which preserves the
            // retention the 1 MiB manifest target used to impose; lock- and
            // JSON-class reads (locks, receipts, sidecars, plan files,
            // compiler artifacts, IPC frames) bind to `CAPTURE_BYTES`.
            let manifest_bytes =
                read_bounded("harness manifest", &manifest_path, process::TAIL_BYTES)?;
            let manifest = digest(&manifest_bytes);
            let source_path = harness_root.join("src/lib.rs");
            let source = digest(&read_bounded(
                "harness source",
                &source_path,
                process::TAIL_BYTES,
            )?);

            let workspace = self.capture_workspace(&metadata, limits)?;

            let initial_bytes = cargo_units(native, self, &manifest_path, &initial_target, limits)?;
            let initial = unit::BuildGraph::parse(
                &initial_bytes,
                &metadata,
                cargo_limits,
                self.toolchain.schemas().unit(),
                self.config.no_build_std(),
                &self.profile,
                &self.target,
                &mut cargo_allocation,
            )?;
            #[cfg(feature = "rustc-driver")]
            let mut extraction_plan = crate::extract::plan::Plan::prepare(
                &harness_root,
                &initial,
                &toolchain,
                &self.target,
                limits,
            )?
            .demand(&self.demand)?
            .inventory(self.inventory);
            #[cfg(feature = "rustc-driver")]
            let command_target = extraction_plan.command_target()?;
            #[cfg(not(feature = "rustc-driver"))]
            let command_target = target_dir.clone();
            Ok(Harness {
                context: Arc::clone(&node_context),
                harness_root,
                manifest,
                source,
                lock,
                selected,
                metadata,
                workspace,
                cargo_allocation,
                toolchain,
                internal,
                documents: None,
                universe: None,
                initial,
                command_target,
                #[cfg(feature = "rustc-driver")]
                dag: None,
                #[cfg(feature = "rustc-driver")]
                extraction_plan,
                #[cfg(feature = "rustc-driver")]
                closure_units: self
                    .inputs_only
                    .then(|| initial_bytes.to_vec().into_boxed_slice()),
            })
        })
    }

    #[allow(
        clippy::too_many_arguments,
        reason = "the versioned harness transforms one request, toolchain, allocation epoch, and universe together"
    )]
    fn prepare_versioned_harness<N: ToolPaths>(
        &self,
        native: &N,
        limits: &Limits,
        cargo_limits: &Limits,
        mut cargo_allocation: unit::Allocation,
        toolchain: toolchain::Proof,
        catalog: universe::Catalog,
        context: universe::ContextProof,
        placement: Placement,
        node_context: Arc<crate::context::Context>,
        toolchain_parent: Option<&crate::node::Validated<toolchain::Attestation>>,
    ) -> Result<Harness> {
        let Placement {
            root: harness_root,
            request,
            state,
            exclusive,
        } = placement;
        let definition = catalog.current().clone();
        let manifest_path = harness_root.join(HARNESS_MANIFEST);
        let source_path = harness_root.join("src/lib.rs");
        let lock_path = harness_root.join("Cargo.lock");
        let (
            metadata,
            selected,
            internal,
            candidate_lock,
            receipt,
            metadata_observation,
            metadata_parent,
        ) = match state {
            PlacementState::Persisted { lock, receipt } => {
                let target_dir = versioned_target_directory(&harness_root);
                let authority =
                    locked_metadata_node_authority(request, &toolchain, self, &manifest_path)?;
                let metadata_parent = match toolchain_parent {
                    Some(parent) => crate::cargo::resolution::cached_locked_with_toolchain(
                        &node_context,
                        parent,
                        &authority,
                        &manifest_path,
                        &lock_path,
                        &target_dir,
                        &crate::metrics::Current,
                        || cargo_metadata(native, self, &manifest_path, &target_dir, true, limits),
                    ),
                    None => crate::cargo::resolution::cached_locked(
                        &node_context,
                        &authority,
                        &manifest_path,
                        &lock_path,
                        &target_dir,
                        &crate::metrics::Current,
                        || cargo_metadata(native, self, &manifest_path, &target_dir, true, limits),
                    ),
                }
                .map_err(|source| Error::Node {
                    operation: "Cargo resolution metadata",
                    source: Box::new(source),
                })?;
                let metadata_bytes = metadata_parent.value().raw().to_vec().into_boxed_slice();
                self.revalidate(limits, "locked persisted metadata")?;
                let (metadata, metadata_observation) = unit::Metadata::parse_versioned(
                    &metadata_bytes,
                    &self.dependencies,
                    cargo_limits,
                    self.toolchain.schemas().graph(),
                    &target_dir,
                    &mut cargo_allocation,
                    &definition,
                )?;
                let metadata =
                    metadata.bind_lock(lock.bytes(), cargo_limits, &mut cargo_allocation)?;
                let selected = metadata.selections(&self.dependencies, &mut cargo_allocation)?;
                let internal = self.internal.seal(&metadata, &selected)?;
                let manifest = render_manifest(self, Some(&internal))?;
                require_exact_file(
                    "persisted final-form harness manifest",
                    &manifest_path,
                    manifest.as_bytes(),
                    process::TAIL_BYTES,
                )?;
                require_exact_file(
                    "persisted generated harness source",
                    &source_path,
                    HARNESS_SOURCE.as_bytes(),
                    process::TAIL_BYTES,
                )?;
                (
                    metadata,
                    selected,
                    internal,
                    lock,
                    Some(receipt),
                    metadata_observation,
                    metadata_parent,
                )
            }
            PlacementState::Fresh => {
                let target_dir = versioned_target_directory(&harness_root);
                let authority =
                    locked_metadata_node_authority(request, &toolchain, self, &manifest_path)?;
                let mut materialized = None;
                let execute = || {
                    let provisional_root = prepare_versioned_roots(self, &harness_root, exclusive)?;
                    let provisional_path = provisional_root.join(HARNESS_MANIFEST);
                    let provisional_target = provisional_root.join("target");
                    let fresh = self.materialize_fresh(
                        limits,
                        &mut cargo_allocation,
                        &definition,
                        (&manifest_path, &source_path, &lock_path),
                        |phase| match phase {
                            MetadataPhase::Provisional => cargo_metadata(
                                native,
                                self,
                                &provisional_path,
                                &provisional_target,
                                false,
                                limits,
                            ),
                            MetadataPhase::Candidate => cargo_metadata(
                                native,
                                self,
                                &manifest_path,
                                &target_dir,
                                false,
                                limits,
                            ),
                        },
                        |bytes, allocation| {
                            unit::Metadata::parse(
                                bytes,
                                &self.dependencies,
                                cargo_limits,
                                self.toolchain.schemas().graph(),
                                &provisional_target,
                                allocation,
                            )
                        },
                    )?;
                    materialized = Some((fresh.internal, fresh.lock));
                    Ok(fresh.bytes)
                };
                let metadata_parent = match toolchain_parent {
                    Some(parent) => crate::cargo::resolution::cached_locked_with_toolchain(
                        &node_context,
                        parent,
                        &authority,
                        &manifest_path,
                        &lock_path,
                        &target_dir,
                        &crate::metrics::Current,
                        execute,
                    ),
                    None => crate::cargo::resolution::cached_locked(
                        &node_context,
                        &authority,
                        &manifest_path,
                        &lock_path,
                        &target_dir,
                        &crate::metrics::Current,
                        execute,
                    ),
                }
                .map_err(|source| Error::Node {
                    operation: "publish fresh Cargo resolution metadata",
                    source: Box::new(source),
                })?;
                let (metadata, metadata_observation) = unit::Metadata::parse_versioned(
                    metadata_parent.value().raw(),
                    &self.dependencies,
                    cargo_limits,
                    self.toolchain.schemas().graph(),
                    &target_dir,
                    &mut cargo_allocation,
                    &definition,
                )?;
                let (internal, lock) = match materialized {
                    Some((internal, lock)) => (Some(internal), lock),
                    None => (
                        None,
                        definition.candidate(
                            read_bounded(
                                "reused candidate resolution lock",
                                &lock_path,
                                process::CAPTURE_BYTES,
                            )?
                            .into_boxed_slice(),
                        )?,
                    ),
                };
                let (metadata, selected, internal) = self.bind_fresh_metadata(
                    metadata,
                    internal,
                    &lock,
                    (&manifest_path, &source_path),
                    cargo_limits,
                    &mut cargo_allocation,
                )?;
                (
                    metadata,
                    selected,
                    internal,
                    lock,
                    None,
                    metadata_observation,
                    metadata_parent,
                )
            }
        };

        let manifest_bytes = read_bounded(
            "final-form harness manifest",
            &manifest_path,
            process::TAIL_BYTES,
        )?;
        let manifest = digest(&manifest_bytes);
        let source = digest(&read_bounded(
            "generated harness source",
            &source_path,
            process::TAIL_BYTES,
        )?);
        let lock = candidate_lock.digest();

        let workspace = self.capture_workspace(&metadata, limits)?;

        #[cfg(feature = "rustc-driver")]
        let initial_target = harness_root.join(AUTHORITATIVE_TARGET_DIRECTORY);
        #[cfg(not(feature = "rustc-driver"))]
        let initial_target = harness_root.join("target");
        let authority = unit_graph_node_authority(request, &toolchain, self, &manifest_path)?;
        let unit_parent = crate::cargo::resolution::cached_unit_graph(
            &node_context,
            &metadata_parent,
            &authority,
            &crate::metrics::Current,
            || cargo_units(native, self, &manifest_path, &initial_target, limits),
        )
        .map_err(|source| Error::Node {
            operation: "Cargo resolution unit graph",
            source: Box::new(source),
        })?;
        let initial_bytes = unit_parent.value().raw().to_vec().into_boxed_slice();
        let (initial, universe) = self.bind_versioned_units(
            &initial_bytes,
            &metadata,
            cargo_limits,
            &mut cargo_allocation,
            &definition,
            metadata_observation,
            candidate_lock,
            request,
            context,
            receipt,
        )?;
        let universe = Some(universe);
        #[cfg(feature = "rustc-driver")]
        let mut extraction_plan = crate::extract::plan::Plan::prepare(
            &harness_root,
            &initial,
            &toolchain,
            &self.target,
            limits,
        )?
        .demand(&self.demand)?
        .inventory(self.inventory);
        #[cfg(feature = "rustc-driver")]
        let command_target = extraction_plan.command_target()?;
        #[cfg(not(feature = "rustc-driver"))]
        let command_target = harness_root.join("target");
        Ok(Harness {
            context: Arc::clone(&node_context),
            harness_root,
            manifest,
            source,
            lock,
            metadata,
            workspace,
            cargo_allocation,
            selected,
            toolchain,
            internal,
            documents: None,
            universe,
            initial,
            command_target,
            #[cfg(feature = "rustc-driver")]
            dag: Some(DagHarness {
                context: node_context.as_ref().clone(),
                request,
                unit: unit_parent,
            }),
            #[cfg(feature = "rustc-driver")]
            extraction_plan,
            #[cfg(feature = "rustc-driver")]
            closure_units: self.inputs_only.then(|| initial_bytes.clone()),
        })
    }

    /// The lock a fresh resolution starts from: the project's own
    /// `Cargo.lock` when it has one, so every package it already locks keeps
    /// its version and Rawr's runner reuses what `cargo build` compiled, and
    /// the universe's seed otherwise. Admission still fixes only iced (D402).
    pub(crate) fn starting_lock<'a>(&'a self, definition: &'a universe::Definition) -> &'a [u8] {
        self.authority.lock.as_deref().unwrap_or(definition.seed())
    }

    fn materialize_fresh(
        &self,
        limits: &Limits,
        cargo_allocation: &mut unit::Allocation,
        definition: &universe::Definition,
        paths: (&Path, &Path, &Path),
        mut observe: impl FnMut(MetadataPhase) -> Result<Box<[u8]>>,
        parse: impl FnOnce(&[u8], &mut unit::Allocation) -> Result<unit::Metadata>,
    ) -> Result<Materialized> {
        let mut provisional_scope = cargo_allocation.begin_provisional()?;
        let provisional = (|| {
            let bytes = observe(MetadataPhase::Provisional)?;
            self.revalidate(limits, "provisional metadata")?;
            let metadata = parse(&bytes, cargo_allocation)?;
            let selected = metadata.selections(&self.dependencies, cargo_allocation)?;
            let internal = self.internal.seal(&metadata, &selected)?;
            Ok::<_, Error>((internal, selected, metadata))
        })();
        let internal = match provisional {
            Ok((internal, selected, metadata)) => {
                drop(selected);
                drop(metadata);
                cargo_allocation.release_provisional(&mut provisional_scope)?;
                internal
            }
            Err(error) => {
                cargo_allocation.release_provisional(&mut provisional_scope)?;
                return Err(error);
            }
        };
        let (manifest_path, source_path, lock_path) = paths;
        let manifest = render_manifest(self, Some(&internal))?;
        write_atomic(manifest_path, manifest.as_bytes())?;
        write_atomic(source_path, HARNESS_SOURCE.as_bytes())?;
        write_atomic(lock_path, self.starting_lock(definition))?;
        // The candidate's metadata is the final metadata: Cargo wrote the lock
        // it resolved, and the `--locked` unit graph that follows refuses a
        // lock it would change. Cargo's bytes can differ when it reads that
        // lock back (it may list a dependency kind once instead of twice), so
        // nothing compares them; canonicalization makes both forms hash alike.
        let bytes = observe(MetadataPhase::Candidate)?;
        self.revalidate(limits, "resolution candidate materialization")?;
        let lock_bytes = read_bounded(
            "candidate resolution lock",
            lock_path,
            process::CAPTURE_BYTES,
        )?;
        let lock = definition.candidate(lock_bytes.into_boxed_slice())?;
        Ok(Materialized {
            bytes,
            internal,
            lock,
        })
    }

    #[allow(
        clippy::type_complexity,
        reason = "the shared transformation binds final metadata, selections and the internal recipe"
    )]
    fn bind_fresh_metadata(
        &self,
        metadata: unit::Metadata,
        internal: Option<internal::Recipe>,
        lock: &universe::Candidate,
        paths: (&Path, &Path),
        cargo_limits: &Limits,
        allocation: &mut unit::Allocation,
    ) -> Result<(unit::Metadata, Box<[Selection]>, internal::Recipe)> {
        let metadata = metadata.bind_lock(lock.bytes(), cargo_limits, allocation)?;
        let selected = metadata.selections(&self.dependencies, allocation)?;
        let internal = match internal {
            Some(internal) => internal,
            None => {
                let internal = self.internal.seal(&metadata, &selected)?;
                require_exact_file(
                    "reused final-form harness manifest",
                    paths.0,
                    render_manifest(self, Some(&internal))?.as_bytes(),
                    process::TAIL_BYTES,
                )?;
                require_exact_file(
                    "reused generated harness source",
                    paths.1,
                    HARNESS_SOURCE.as_bytes(),
                    process::TAIL_BYTES,
                )?;
                internal
            }
        };
        internal.revalidate(&metadata, &selected)?;
        Ok((metadata, selected, internal))
    }

    #[allow(
        clippy::too_many_arguments,
        reason = "initial units complete the existing versioned metadata and lock proof chain"
    )]
    fn bind_versioned_units(
        &self,
        bytes: &[u8],
        metadata: &unit::Metadata,
        cargo_limits: &Limits,
        allocation: &mut unit::Allocation,
        definition: &universe::Definition,
        observation: universe::MetadataObservation,
        lock: universe::Candidate,
        request: Digest,
        context: universe::ContextProof,
        receipt: Option<universe::Receipt>,
    ) -> Result<(unit::BuildGraph, universe::Pending)> {
        let (initial, metadata_proof, unit_proof) = unit::BuildGraph::parse_versioned(
            bytes,
            metadata,
            cargo_limits,
            self.toolchain.schemas().unit(),
            self.config.no_build_std(),
            &self.profile,
            &self.target,
            allocation,
            definition,
            observation,
        )?;
        let final_lock = lock.admit(&metadata_proof)?;
        Ok((
            initial,
            universe::Pending::transform(
                request,
                final_lock,
                context,
                metadata_proof,
                unit_proof,
                receipt,
            ),
        ))
    }

    fn revalidate(&self, limits: &Limits, command: &'static str) -> Result<()> {
        self.parallelism.revalidate(command)?;
        self.config.revalidate(limits, command)?;
        for input in &self.inputs {
            if authority_input_digest(&input.path, process::TAIL_BYTES)? != input.digest {
                return Err(Error::ResolutionDrift {
                    command,
                    field: "Cargo manifest/lock authority",
                });
            }
        }
        Ok(())
    }
}

impl Request {
    fn capture_workspace(
        &self,
        metadata: &unit::Metadata,
        limits: &Limits,
    ) -> Result<crate::input::workspace::Image> {
        let mut account = crate::input::workspace::Account::bounded(limits);
        let roots = crate::input::workspace::roots(&self.inputs, metadata, &mut account)?;
        crate::input::workspace::capture(roots, &mut account)
    }

    #[allow(
        clippy::too_many_arguments,
        reason = "one Build transformation retains the original harness and Cargo allocation lineage"
    )]
    fn check_build(
        &self,
        bytes: &[u8],
        environment: Box<[(OsString, OsString)]>,
        mut initial: unit::BuildGraph,
        cargo_allocation: &unit::Allocation,
        toolchain: &toolchain::Proof,
        files: [(&Path, Digest); 3],
        limits: &Limits,
        cargo_limits: &Limits,
        workspace: &crate::input::workspace::Image,
    ) -> Result<(crate::input::build::Build, unit::BuildGraph)> {
        let build = crate::input::build::Build::parse(
            bytes,
            environment,
            toolchain.cargo_commit(),
            limits,
        )?;
        let revalidate = || {
            revalidate_harness(
                self, files[0].0, files[0].1, files[1].0, files[1].1, files[2].0, files[2].1,
                limits, "build",
            )?;
            workspace.require(&mut crate::input::workspace::Account::bounded(limits))
        };
        revalidate()?;
        initial.bind_reachability(cargo_allocation, cargo_limits)?;
        revalidate()?;
        Ok((build, initial))
    }

    fn resolve_under_epoch<N: ProductionReplay>(
        self,
        native: &N,
        harness: Harness,
        limits: &Limits,
    ) -> Result<Resolution> {
        let Harness {
            context: node_context,
            harness_root,
            manifest,
            source,
            lock,
            metadata,
            workspace,
            cargo_allocation,
            selected,
            toolchain,
            internal,
            documents,
            universe,
            initial,
            command_target,
            #[cfg(feature = "rustc-driver")]
            dag,
            #[cfg(feature = "rustc-driver")]
            extraction_plan,
            #[cfg(feature = "rustc-driver")]
            closure_units,
        } = harness;
        workspace.require(&mut crate::input::workspace::Account::bounded(limits))?;
        let manifest_path = harness_root.join(HARNESS_MANIFEST);
        let source_path = harness_root.join("src/lib.rs");
        let lock_path = harness_root.join("Cargo.lock");
        let mut execution_identity = Sha256::new();
        for authority in [
            manifest,
            source,
            lock,
            self.authority.digest,
            self.config.digest(),
        ] {
            execution_identity.update(authority.0);
        }
        execution_identity.update(toolchain.node_digest());
        execution_identity.update(self.profile.as_bytes());
        let execution_authority: [u8; 32] = execution_identity.finalize().into();
        let uncached =
            |operation| execution::Scope::bind(&node_context, operation, execution_authority);
        #[cfg(feature = "rustc-driver")]
        let environment = cargo_authoritative_environment(native, &self, &extraction_plan)?;
        #[cfg(feature = "rustc-driver")]
        let (bytes, dag_build) = if let Some(dag) = dag {
            let DagHarness {
                context,
                request,
                unit,
            } = dag;
            let authority =
                authoritative_build_node_authority(request, &toolchain, &self, &manifest_path)?;
            let authority = workspace_node_authority(&authority, &workspace)?;
            let execution_environment = environment.clone();
            let build = crate::cargo::resolution::cached_build(
                &context,
                &unit,
                &authority,
                extraction_plan.target_directory(),
                extraction_plan.protocol_directory(),
                &crate::metrics::Current,
                || {
                    workspace.require(&mut crate::input::workspace::Account::bounded(limits))?;
                    let bytes = cargo_execute_authoritative_build(
                        native,
                        &self,
                        &manifest_path,
                        extraction_plan.target_directory(),
                        execution_environment,
                        limits,
                    )?;
                    workspace.require(&mut crate::input::workspace::Account::bounded(limits))?;
                    Ok(bytes)
                },
            )
            .map_err(|source| Error::Node {
                operation: "authoritative Cargo build product",
                source: Box::new(source),
            })?;
            let bytes = build.value().raw().to_vec().into_boxed_slice();
            (bytes, Some((context, request, build)))
        } else {
            (
                uncached(execution::Operation::Build).run(|| {
                    cargo_execute_authoritative_build(
                        native,
                        &self,
                        &manifest_path,
                        extraction_plan.target_directory(),
                        environment.clone(),
                        limits,
                    )
                })?,
                None,
            )
        };
        #[cfg(not(feature = "rustc-driver"))]
        let (bytes, environment) = uncached(execution::Operation::Build)
            .run(|| cargo_execute_build(native, &self, &manifest_path, &command_target, limits))?;
        let (build, build_unit) = self.check_build(
            &bytes,
            environment,
            initial,
            &cargo_allocation,
            &toolchain,
            [
                (&manifest_path, manifest),
                (&source_path, source),
                (&lock_path, lock),
            ],
            limits,
            limits,
            &workspace,
        )?;
        #[cfg(feature = "rustc-driver")]
        let probe_scope = uncached(execution::Operation::Probe);
        let receipt_path = harness_root.join(UNIVERSE_RECEIPT);
        let harness = Harness {
            context: node_context,
            harness_root,
            manifest,
            source,
            lock,
            metadata,
            workspace,
            cargo_allocation,
            selected,
            toolchain,
            internal,
            documents,
            universe,
            initial: build_unit,
            command_target,
            #[cfg(feature = "rustc-driver")]
            dag: None,
            #[cfg(feature = "rustc-driver")]
            extraction_plan,
            #[cfg(feature = "rustc-driver")]
            closure_units,
        };
        #[allow(unused_mut)]
        let mut resolution = self.complete_harness(
            native,
            harness,
            build,
            limits,
            [manifest_path, source_path, lock_path],
            &receipt_path,
            #[cfg(feature = "rustc-driver")]
            Some(&probe_scope),
            #[cfg(feature = "rustc-driver")]
            None,
        )?;
        #[cfg(feature = "rustc-driver")]
        {
            resolution.dag = dag_build.map(|(context, request, build)| DagResolution {
                context,
                request,
                build,
            });
        }
        Ok(resolution)
    }

    #[allow(
        clippy::too_many_arguments,
        reason = "completion retains the prepared harness, original account, fixed paths and existing execution ownership"
    )]
    fn complete_harness(
        self,
        native: &impl ProductionReplay,
        harness: Harness,
        build: crate::input::build::Build,
        limits: &Limits,
        files: [PathBuf; 3],
        receipt_path: &Path,
        #[cfg(feature = "rustc-driver")] probe_scope: Option<&execution::Scope>,
        #[cfg(feature = "rustc-driver")] reserved_layout: Option<&resolution::layout::Layout>,
    ) -> Result<Resolution> {
        let complete = self.complete_harness_parts(
            native,
            harness,
            build,
            limits,
            files,
            receipt_path,
            #[cfg(feature = "rustc-driver")]
            probe_scope,
            #[cfg(feature = "rustc-driver")]
            reserved_layout,
        )?;
        Ok(complete.into_resolution(self))
    }

    #[allow(
        clippy::too_many_arguments,
        reason = "completion preserves the request and all original execution and allocation accounts"
    )]
    fn complete_harness_parts(
        &self,
        #[cfg_attr(
            not(feature = "rustc-driver"),
            expect(
                unused_variables,
                reason = "only the extraction plan launches from completion"
            )
        )]
        native: &impl ProductionReplay,
        harness: Harness,
        mut build: crate::input::build::Build,
        limits: &Limits,
        files: [PathBuf; 3],
        receipt_path: &Path,
        #[cfg(feature = "rustc-driver")] probe_scope: Option<&execution::Scope>,
        #[cfg(feature = "rustc-driver")] reserved_layout: Option<&resolution::layout::Layout>,
    ) -> Result<CompletedHarness> {
        let Harness {
            context: node_context,
            harness_root,
            manifest,
            source,
            lock,
            metadata,
            workspace,
            cargo_allocation,
            selected,
            toolchain,
            internal,
            documents,
            universe,
            initial: build_unit,
            command_target,
            #[cfg(feature = "rustc-driver")]
                dag: _,
            #[cfg(feature = "rustc-driver")]
            extraction_plan,
            #[cfg(feature = "rustc-driver")]
            closure_units,
        } = harness;
        let fallback_packages = build.fallback_packages(toolchain.cargo_commit(), limits)?;
        for (package_id, package_manifest) in fallback_packages {
            let package = metadata.package(&package_id).ok_or_else(|| Error::Input {
                code: "missing-fallback-package-authority",
                reason: package_id.clone(),
            })?;
            let files = fallback_files(
                self,
                package,
                &package_manifest,
                toolchain.cargo_commit(),
                build.usage(),
                limits,
            )?;
            build.bind_package_files(&package_id, files, limits)?;
        }
        #[cfg(feature = "rustc-driver")]
        let mut capture = if self.inputs_only {
            None
        } else {
            Some(match probe_scope {
                Some(scope) => scope
                    .run(|| extraction_plan.seal(native, &build, &build_unit, &metadata, limits))?,
                None => extraction_plan.seal(native, &build, &build_unit, &metadata, limits)?,
            })
        };
        #[cfg(feature = "rustc-driver")]
        let mut settlement = capture
            .as_mut()
            .map_or_else(Vec::new, |capture| capture.settlement(&build));
        #[cfg(feature = "rustc-driver")]
        let staged = capture.as_ref().and_then(|capture| {
            let keys = capture
                .shared_keys()
                .into_iter()
                .map(str::to_owned)
                .collect::<Vec<_>>();
            Some((capture.staged()?.to_path_buf(), keys))
        });
        #[cfg(feature = "rustc-driver")]
        let probe_artifacts = capture
            .as_mut()
            .map(|capture| capture.artifact_inputs())
            .transpose()?
            .unwrap_or_default();
        #[cfg(not(feature = "rustc-driver"))]
        let probe_artifacts = Vec::new();
        #[cfg(feature = "rustc-driver")]
        let command_build = reserved_layout.map_or(command_target.as_path(), |layout| {
            layout.final_form().build()
        });
        #[cfg(not(feature = "rustc-driver"))]
        let command_build = command_target.as_path();
        let complete = build.correlate(
            &build_unit,
            limits,
            node_context.graph_budget(),
            command_build,
            &self.target,
        )?;
        #[cfg(feature = "rustc-driver")]
        let partitioned = self.inventory || self.inputs_only;
        let (input, retained_build) = crate::input::Baseline::capture_retaining(
            complete,
            &self.inputs,
            workspace,
            &probe_artifacts,
            self.config.input_config()?,
            files,
            &toolchain,
            &internal,
            universe.as_ref().map(universe::Pending::metadata),
            &self.target,
            &self.profile,
            &node_context,
            &build_unit,
            command_build,
            #[cfg(feature = "rustc-driver")]
            partitioned,
            limits,
        )?;
        #[cfg(feature = "rustc-driver")]
        if let Some(current) =
            crate::input::bindings::Current::capture(&self.project_root, &self.config, limits)
        {
            for settled in &mut settlement {
                if let crate::bindings::Settled::Extracted {
                    inputs_packages,
                    publish,
                    ..
                } = settled
                {
                    publish.meta.inputs = crate::input::bindings::Inputs::capture(
                        &input,
                        retained_build.build(),
                        inputs_packages,
                        &current,
                    );
                }
            }
            // The next demand of this resolution replays these tables
            // instead of building and probing again.
            if let Some((staged, keys)) = &staged {
                let entries = settlement
                    .iter()
                    .filter_map(|settled| {
                        let crate::bindings::Settled::Extracted {
                            inputs_packages,
                            publish,
                            ..
                        } = settled
                        else {
                            return None;
                        };
                        let inputs = publish.meta.inputs.clone().or_else(|| {
                            crate::input::bindings::Inputs::capture_staged(
                                &input,
                                retained_build.build(),
                                inputs_packages,
                                &current,
                            )
                        })?;
                        Some((settled, inputs))
                    })
                    .collect::<Vec<_>>();
                let keys = keys.iter().map(String::as_str).collect::<Vec<_>>();
                crate::bindings::stage(staged, &keys, &entries);
            }
        }
        let universe = universe
            .map(|pending| {
                pending.complete(
                    manifest,
                    source,
                    metadata.digest(),
                    build_unit.digest(),
                    &input,
                    limits,
                )
            })
            .transpose();
        let universe = universe?;

        let universe = if let Some(completion) = universe {
            let proof = completion.proof();
            if let Some(receipt) = completion.receipt() {
                let receipt = receipt.encode()?;
                write_atomic(receipt_path, &receipt)?;
            }
            Some(proof)
        } else {
            None
        };
        // A capture of intrinsic facts keeps what the nightly's build
        // scripts gave, for its entries to be compared with the project's.
        #[cfg(feature = "rustc-driver")]
        let nightly_scripts = if self.inventory && !self.inputs_only {
            crate::applicability::store::Scripts::observed(retained_build.build())
        } else {
            crate::applicability::store::Scripts::default()
        };
        let observations = documents.map(|documents| Retained {
            documents,
            build: retained_build,
        });
        Ok(CompletedHarness {
            context: node_context,
            harness_root,
            manifest,
            source,
            lock,
            selected,
            metadata,
            cargo_allocation,
            build_unit,
            toolchain,
            input,
            internal,
            observations,
            universe,
            #[cfg(feature = "rustc-driver")]
            capture,
            #[cfg(feature = "rustc-driver")]
            settlement,
            #[cfg(feature = "rustc-driver")]
            closure_units,
            #[cfg(feature = "rustc-driver")]
            nightly_scripts,
        })
    }
}

impl Builder {
    /// Acquire complete intrinsic facts without consumer method demand.
    /// This separate acquisition is not served by legacy demand tables.
    pub fn inventory(mut self) -> Self {
        self.inventory = true;
        self
    }
    /// Seal the exact environment replayed for Cargo and toolchain commands.
    pub fn environment(mut self, environment: config::Environment) -> Self {
        self.environment = Some(environment);
        self
    }

    /// Select the one explicit Cargo manifest authority.
    pub fn cargo_authority(mut self, manifest: &Path) -> Self {
        self.cargo_authority = Some(manifest.to_path_buf());
        self
    }

    /// Select a supported target triple instead of the pinned host.
    pub fn target(mut self, target: &str) -> Self {
        self.target = Some(target.into());
        self
    }

    /// Select one Cargo profile from the authority copied into the harness.
    pub fn profile(mut self, profile: &str) -> Self {
        let valid = profile.bytes().enumerate().all(|(index, byte)| {
            byte.is_ascii_alphanumeric() || byte == b'_' || index > 0 && byte == b'-'
        });
        assert!(
            !profile.is_empty() && valid,
            "`{profile}` is not a Cargo profile name"
        );
        self.profile = profile.into();
        self
    }

    /// Add one dependency with a final graph role.
    pub fn dependency(mut self, dependency: config::Dependency) -> Self {
        self.dependencies.push(dependency);
        self
    }

    /// Bind one preselected framework resolution universe to this request.
    #[doc(hidden)]
    pub fn resolution_universe(mut self, universe: universe::Catalog) -> Self {
        assert!(
            self.universe.is_none(),
            "a request selects one resolution universe"
        );
        self.universe = Some(universe);
        self
    }

    /// Demand one Rust path Rawr names, root alias first. A
    /// dependency with no demanded path is extracted whole; one with
    /// demand is seeded from its demanded paths only: each item a path names
    /// brings its closure (signatures, public fields, inherent and trait impl
    /// tables), by the same probe against the existing metadata. Paths
    /// through wholesale crate re-exports (`pub use other_crate;`) resolve
    /// the same way. Order and duplicates do not matter.
    pub fn demand<'a>(mut self, path: impl IntoIterator<Item = &'a str>) -> Self {
        let mut segments = Vec::new();
        for segment in path {
            assert!(
                !segment.is_empty(),
                "a demanded Rust path has no empty segment"
            );
            segments.push(Box::<str>::from(segment));
        }
        assert!(
            segments.len() >= 2,
            "a demanded Rust path names an item below a crate"
        );
        self.demand.insert(segments.into_boxed_slice());
        self
    }

    /// Bind the exact Rawr-manifest exclusion policy to this request.
    pub fn policy(mut self, policy: crate::extract::Policy) -> Self {
        self.policy = Some(policy);
        self
    }

    /// Promote loose request fields into canonical path/config/authority proof.
    pub fn build(self) -> Result<Request> {
        assert!(
            !self.inventory || self.demand.is_empty(),
            "an intrinsic acquisition carries no program demand"
        );
        let limits = &self.limits;
        // Cargo follows a symlinked project; rawr works in the directory it
        // names. The cache may be spelled through either path.
        let project_root = resolved_directory("project root", &self.project_root)?;
        let expected_cache = project_root.join("target/rawr/cache");
        let requested_cache = lexical_absolute(&self.cache_root)?;
        let spelled_cache = lexical_absolute(&self.project_root)?.join("target/rawr/cache");
        if requested_cache != expected_cache && requested_cache != spelled_cache {
            return Err(Error::Path {
                field: "project cache root",
                path: self.cache_root,
                reason: format!("must be `{}`", expected_cache.display()).into_boxed_str(),
            });
        }
        reject_existing_symlink(&project_root, &expected_cache)?;
        let environment = self
            .environment
            .expect("each request is built with its Cargo environment");
        let policy = self
            .policy
            .expect("each request is built with its exclusion policy");
        let internal = internal::Seed::transform(&crate::host::root()?)?;
        let mut dependencies = Vec::with_capacity(self.dependencies.len().saturating_add(1));
        for dependency in self.dependencies {
            dependencies.push(dependency.seal(&project_root)?);
        }
        // A package imports Rust only when it has a dependency or Rust
        // modules, whose crate is then a dependency too; the internal
        // dependency below brings the runtime and compiler roles.
        assert!(
            !dependencies.is_empty(),
            "a request declares at least one library"
        );
        let internal_dependency = internal.dependency()?;
        dependencies.push(internal_dependency.seal(&project_root)?);
        dependencies.sort_unstable_by(|left, right| left.alias().cmp(right.alias()));
        // A dependency may share the name of the package's own library, as
        // Cargo allows; both are crates of the one program rawr builds.
        if let Some(pair) = dependencies
            .windows(2)
            .find(|pair| pair[0].alias() == pair[1].alias())
        {
            let manifest = self
                .cargo_authority
                .as_deref()
                .unwrap_or_else(|| Path::new("Cargo.toml"));
            return Err(Error::Request {
                reason: format!(
                    "two crates of this package are named `{}`; give the dependency \
                     another name in `{}`",
                    pair[0].alias(),
                    manifest.display(),
                )
                .into_boxed_str(),
            });
        }

        let toolchain = toolchain::Requirement::pinned();
        let parallelism = process::parallelism::Proof::discover()?;
        let target = self
            .target
            .unwrap_or_else(|| toolchain.default_target().into());
        if !toolchain.supports_target(&target) {
            return Err(Error::Request {
                reason: format!("rawr does not support the target `{target}`").into_boxed_str(),
            });
        }

        let selected_authority = self
            .cargo_authority
            .as_deref()
            .map(canonical_authority)
            .transpose()?;
        // Cargo reads `[patch]`, `[replace]`, `[profile]`, the resolver and
        // the lock from the workspace root, so a member's authority is the
        // root manifest of the workspace it belongs to.
        let workspace_authority = selected_authority
            .as_deref()
            .and_then(Path::parent)
            .map(|package| rawr_location::workspace_root(package).join("Cargo.toml"))
            .map(|manifest| canonical_authority(&manifest))
            .transpose()?;
        let mut authority = Authority::load(workspace_authority.as_deref())?;
        if let (Some(universe), Some(patch)) = (self.universe.as_ref(), internal.patch()) {
            authority.inherit_patches(patch, |name| universe.names_package(name))?;
        }
        let config = config::Provenance::discover(
            &project_root,
            selected_authority.as_deref(),
            environment,
            &target,
            &self.profile,
            parallelism,
            limits,
        )?;
        let inputs = authority_inputs(&authority, &dependencies)?;
        if let Some(path) = self.demand.iter().find(|path| {
            !dependencies
                .iter()
                .any(|dependency| dependency.alias() == path[0].as_ref())
        }) {
            // Reached by a remembered path whose crate the package no longer
            // declares; the caller forgets the memory and asks again.
            return Err(Error::Request {
                reason: format!(
                    "failed to resolve: use of unresolved module or unlinked crate `{}`",
                    path[0]
                )
                .into_boxed_str(),
            });
        }
        Ok(Request {
            inventory: self.inventory,
            #[cfg(feature = "rustc-driver")]
            inputs_only: false,
            project_root,
            cache_root: expected_cache,
            authority,
            inputs,
            config,
            toolchain,
            target,
            profile: self.profile,
            dependencies: dependencies.into_boxed_slice(),
            policy,
            universe: self.universe,
            parallelism,
            internal,
            demand: self.demand.into_iter().collect(),
        })
    }
}

impl Resolution {
    /// What a derive's expansion needs of this resolution's build (#593).
    #[cfg(feature = "rustc-driver")]
    pub(crate) fn expansion(&self) -> Option<&crate::extract::plan::Expansion> {
        self.capture
            .as_ref()
            .map(crate::extract::plan::Capture::expansion)
    }

    /// What the nightly's build scripts gave a capture of intrinsic facts.
    #[cfg(feature = "rustc-driver")]
    pub(crate) fn take_nightly_scripts(&mut self) -> crate::applicability::store::Scripts {
        std::mem::take(&mut self.nightly_scripts)
    }

    #[cfg(feature = "rustc-driver")]
    pub(crate) fn into_native_inputs(
        self,
        limits: &Limits,
    ) -> Result<crate::applicability::native::Inputs> {
        assert!(
            self.capture.is_none(),
            "native inputs come from an input-only resolution, never an API capture"
        );
        let (proof, partitions) = self.input.prove_partitioned(limits)?;
        Ok(crate::applicability::native::Inputs::from_capture(
            proof, partitions,
        ))
    }
    /// Reconstruct launch paths solely from this resolution's validated
    /// toolchain proof; no discovery or version process is executed.
    #[doc(hidden)]
    #[must_use]
    pub fn replay_native(&self) -> Native {
        Native::from_toolchain(&self.toolchain)
    }

    /// The one fixed Rawr-owned resolution harness directory.
    pub fn harness_root(&self) -> &Path {
        &self.harness_root
    }

    /// The exact final-form harness manifest identity.
    pub const fn manifest_digest(&self) -> Digest {
        self.manifest
    }

    /// The exact generated source identity; later phases may replace only this layer.
    pub const fn source_digest(&self) -> Digest {
        self.source
    }

    /// The Rawr-owned final lock authority identity.
    pub const fn lock_digest(&self) -> Digest {
        self.lock
    }

    /// The selected upstream Cargo authority policy identity.
    pub const fn authority_digest(&self) -> Digest {
        self.authority
    }

    /// The hierarchical config-candidate path, state, and content identity.
    pub const fn config_digest(&self) -> Digest {
        self.config
    }

    /// The complete canonical Cargo metadata identity.
    pub fn metadata_digest(&self) -> Digest {
        self.metadata.digest()
    }

    /// Exact selected package identities in canonical alias order.
    pub fn selections(&self) -> impl ExactSizeIterator<Item = &Selection> {
        self.selected.iter()
    }

    /// The exact command-specific selected unit identity.
    pub fn build_unit_digest(&self) -> Digest {
        self.build_unit.digest()
    }

    /// The installed compiler/toolchain proof.
    pub const fn toolchain(&self) -> &toolchain::Proof {
        &self.toolchain
    }

    /// The exact selected target triple.
    pub fn target(&self) -> &str {
        &self.target
    }

    /// The exact selected Cargo profile.
    pub fn profile(&self) -> &str {
        &self.profile
    }

    #[cfg(feature = "rustc-driver")]
    pub(crate) fn into_extraction(self, limits: &Limits) -> Result<crate::extract::Publication> {
        self.into_extraction_mode(limits, true)
    }

    #[cfg(feature = "rustc-driver")]
    pub(crate) fn into_extraction_uncached(
        self,
        limits: &Limits,
    ) -> Result<crate::extract::Publication> {
        self.into_extraction_mode(limits, false)
    }

    #[cfg(feature = "rustc-driver")]
    fn into_extraction_mode(
        self,
        limits: &Limits,
        cached: bool,
    ) -> Result<crate::extract::Publication> {
        let Self {
            metadata,
            selected,
            build_unit,
            toolchain,
            target,
            profile,
            input,
            capture,
            policy,
            universe,
            dag,
            ..
        } = self;
        let publication = crate::extract::Publication::from_resolution_parts(
            &metadata,
            &build_unit,
            &selected,
            &toolchain,
            &target,
            &profile,
            universe,
            capture.expect("an input-only resolution never publishes an API"),
            input,
            policy,
            limits,
        )?;
        if cached && let Some(dag) = dag {
            return publication.cache(dag, limits);
        }
        Ok(publication)
    }

    #[cfg(feature = "rustc-driver")]
    pub(crate) fn publication<T>(
        mut self,
        native: &Native,
        limits: &Limits,
        publish: impl FnOnce(Self) -> Result<T>,
    ) -> Result<T> {
        let _process_owner = acquire_process_owner()?;
        let mut lease = open_lease_at(&self.project_root, &self.cache_root)?;
        let _lease_owner = lease.try_write().map_err(|source| {
            if source.kind() == std::io::ErrorKind::WouldBlock {
                Error::Owner
            } else {
                Error::Io {
                    operation: "acquire publication cache lease",
                    path: self.cache_root.join(LEASE_FILE),
                    source,
                }
            }
        })?;
        let native = native.production();
        execution::Scope::bind(
            &self.context,
            execution::Operation::Publication,
            self.input.fingerprint().as_bytes().to_owned(),
        )
        .run(|| self.revalidate_publication(&native, limits))?;
        publish(self)
    }

    #[cfg(feature = "rustc-driver")]
    fn revalidate_publication(
        &mut self,
        native: &process::ProductionNative<'_>,
        limits: &Limits,
    ) -> Result<()> {
        self.parallelism.revalidate("publication validation")?;
        self.retained_config
            .revalidate(limits, "publication validation")?;
        for input in &self.retained_inputs {
            if authority_input_digest(&input.path, process::TAIL_BYTES)? != input.digest {
                return Err(Error::ResolutionDrift {
                    command: "publication validation",
                    field: "Cargo manifest/lock authority",
                });
            }
        }
        // File reads are I/O retention bounded by the physical capture
        // constant.
        for (field, path, expected, limit) in [
            (
                "final-form harness manifest",
                self.harness_root.join(HARNESS_MANIFEST),
                self.manifest,
                process::TAIL_BYTES,
            ),
            (
                "generated harness source",
                self.harness_root.join("src/lib.rs"),
                self.source,
                process::TAIL_BYTES,
            ),
            (
                "Rawr-owned lock authority",
                self.harness_root.join("Cargo.lock"),
                self.lock,
                process::CAPTURE_BYTES,
            ),
        ] {
            let bytes = read_bounded(field, &path, limit)?;
            if digest(&bytes) != expected {
                return Err(Error::ResolutionDrift {
                    command: "publication validation",
                    field,
                });
            }
        }
        let environment = self
            .retained_config
            .command_environment(&self.harness_root.join("target"), native)?;
        let current_toolchain = toolchain::Requirement::pinned().verify(
            native,
            &self.project_root,
            environment,
            limits.process,
            &self.target,
        )?;
        if current_toolchain != self.toolchain {
            return Err(Error::ResolutionDrift {
                command: "publication validation",
                field: "pinned toolchain proof",
            });
        }
        self.internal.revalidate(&self.metadata, &self.selected)?;
        self.build_unit
            .bind_reachability(&self.cargo_allocation, limits)?;
        if let Some(capture) = &mut self.capture {
            capture.revalidate(limits)?;
        }
        Ok(())
    }
}

impl Selection {
    /// The Rawr-side dependency alias used by generated Rust paths.
    pub fn alias(&self) -> &str {
        &self.alias
    }

    /// Cargo's exact source/version-qualified package ID.
    pub fn package_id(&self) -> &str {
        &self.package_id
    }

    /// The dependency's canonical nonempty final-runner roles.
    pub const fn roles(&self) -> config::Roles {
        self.roles
    }
}

impl Authority {
    /// Cargo applies `[patch]` only from the workspace root, and the
    /// harness is that root. Rawr's own patches for packages its facade
    /// closure reaches (the cosmic-text revision the supported iced needs)
    /// are added under each registry unless the project patches that package
    /// itself; the project's patch wins, as Cargo's would.
    fn inherit_patches(&mut self, rawr: &Item, needed: impl Fn(&str) -> bool) -> Result<()> {
        let Some(registries) = rawr.as_table_like() else {
            return Ok(());
        };
        for (registry, entries) in registries.iter() {
            let Some(entries) = entries.as_table_like() else {
                continue;
            };
            for (name, specification) in entries.iter() {
                if !needed(name) {
                    continue;
                }
                let patch = self.retained.entry("patch".into()).or_insert_with(|| {
                    let mut table = Table::new();
                    table.set_implicit(true);
                    Item::Table(table)
                });
                let invalid = || Error::Schema {
                    document: "Cargo authority manifest",
                    reason: "`patch` is not a table of registry tables".into(),
                };
                let registry_table = patch
                    .as_table_like_mut()
                    .ok_or_else(invalid)?
                    .entry(registry)
                    .or_insert(Item::Table(Table::new()))
                    .as_table_like_mut()
                    .ok_or_else(invalid)?;
                if registry_table.contains_key(name) {
                    continue;
                }
                registry_table.insert(name, specification.clone());
            }
        }
        Ok(())
    }

    fn load(manifest: Option<&Path>) -> Result<Self> {
        let Some(manifest) = manifest else {
            let digest = digest(b"generated Cargo authority\0resolver=3");
            return Ok(Self {
                resolver: "3".into(),
                retained: BTreeMap::new(),
                digest,
                inputs: Box::default(),
                lock: None,
            });
        };
        let bytes = read_bounded("Cargo authority manifest", manifest, process::TAIL_BYTES)?;
        let document = std::str::from_utf8(&bytes)
            .map_err(|error| Error::Schema {
                document: "Cargo authority manifest",
                reason: error.to_string().into_boxed_str(),
            })?
            .parse::<DocumentMut>()
            .map_err(|error| Error::Schema {
                document: "Cargo authority manifest",
                reason: error.to_string().into_boxed_str(),
            })?;
        let resolver_value = authority_resolver(&document)?;
        let resolver = resolver_value.into();
        let mut retained = BTreeMap::new();
        for key in ["cargo-features", "patch", "replace", "profile"] {
            if let Some(item) = document.get(key) {
                let rendered = formatted(format_args!("{item}"))?;
                drop(rendered);
                let mut item = item.clone();
                if matches!(key, "patch" | "replace") {
                    let root = manifest.parent().ok_or_else(|| Error::Path {
                        field: "Cargo authority manifest",
                        path: manifest.to_path_buf(),
                        reason: "has no containing directory".into(),
                    })?;
                    absolutize_policy_paths(&mut item, root)?;
                }
                retained.insert(key.into(), item);
            }
        }
        let mut lock_evidence = None;
        let mut lock_input = None;
        if let Some(root) = manifest.parent() {
            let lock = root.join("Cargo.lock");
            let present = match fs::symlink_metadata(&lock) {
                Ok(_) => true,
                Err(source) if source.kind() == std::io::ErrorKind::NotFound => false,
                Err(source) => {
                    return Err(Error::Io {
                        operation: "inspect Cargo authority lock",
                        path: lock,
                        source,
                    })
                }
            };
            lock_input = Some(Input {
                path: lock.clone(),
                digest: None,
                role: Role::Lock,
            });
            if present {
                let lock = canonical_file("Cargo authority lock", &lock)?;
                let bytes = read_bounded("Cargo authority lock", &lock, process::TAIL_BYTES)?;
                if let Some(input) = lock_input.as_mut() {
                    input.digest = Some(digest(&bytes));
                }
                lock_evidence = Some((lock, bytes));
            }
        }
        let evidence_bytes = 8_usize
            .saturating_add(manifest.as_os_str().len())
            .saturating_add(8)
            .saturating_add(bytes.len())
            .saturating_add(lock_evidence.as_ref().map_or(0, |(lock, bytes)| {
                8_usize
                    .saturating_add(lock.as_os_str().len())
                    .saturating_add(8)
                    .saturating_add(bytes.len())
            }));
        let mut evidence = Vec::with_capacity(evidence_bytes);
        append_framed(&mut evidence, manifest.as_os_str().as_encoded_bytes());
        append_framed(&mut evidence, &bytes);
        if let Some((lock, bytes)) = &lock_evidence {
            append_framed(&mut evidence, lock.as_os_str().as_encoded_bytes());
            append_framed(&mut evidence, bytes);
        }
        let mut inputs = Vec::new();
        inputs
            .try_reserve_exact(2)
            .map_err(|source| Error::Allocation {
                operation: "interpreted Cargo authority inputs",
                source,
            })?;
        inputs.push(Input {
            path: manifest.to_path_buf(),
            digest: Some(digest(&bytes)),
            role: Role::Manifest,
        });
        if let Some(input) = lock_input {
            inputs.push(input);
        }
        #[cfg(test)]
        OWNED_AUTHORITIES.with(|owned| owned.set(owned.get().saturating_add(1)));
        Ok(Self {
            resolver,
            retained,
            digest: digest(&evidence),
            inputs: inputs.into_boxed_slice(),
            lock: lock_evidence.map(|(_, bytes)| bytes.into_boxed_slice()),
        })
    }
}

fn open_lease(request: &Request) -> Result<fd_lock::RwLock<File>> {
    open_lease_at(&request.project_root, &request.cache_root)
}

fn acquire_process_owner() -> Result<std::sync::MutexGuard<'static, ()>> {
    match HEAVY_OWNER.try_lock() {
        Ok(owner) => Ok(owner),
        Err(TryLockError::WouldBlock) => Err(Error::Owner),
        // The gate guards no data, so a panic while another build held it
        // leaves nothing to repair.
        Err(TryLockError::Poisoned(owner)) => Ok(owner.into_inner()),
    }
}

fn acquire_lease<'lease>(
    lease: &'lease mut fd_lock::RwLock<File>,
    path: &Path,
    operation: &'static str,
) -> Result<fd_lock::RwLockWriteGuard<'lease, File>> {
    lease.try_write().map_err(|source| {
        if source.kind() == std::io::ErrorKind::WouldBlock {
            Error::Owner
        } else {
            Error::Io {
                operation,
                path: path.to_path_buf(),
                source,
            }
        }
    })
}

fn open_lease_at(project_root: &Path, requested_cache: &Path) -> Result<fd_lock::RwLock<File>> {
    reject_existing_symlink(project_root, requested_cache)?;
    fs::create_dir_all(requested_cache).map_err(|source| Error::Io {
        operation: "create project cache root",
        path: requested_cache.to_path_buf(),
        source,
    })?;
    let cache_root = canonical_directory("project cache root", requested_cache)?;
    if cache_root != requested_cache || !cache_root.starts_with(project_root) {
        return Err(Error::Path {
            field: "project cache root",
            path: cache_root,
            reason: "is outside the project directory".into(),
        });
    }
    let lease_path = cache_root.join(LEASE_FILE);
    if fs::symlink_metadata(&lease_path).is_ok_and(|metadata| metadata.file_type().is_symlink()) {
        return Err(Error::Path {
            field: "resolution cache lease",
            path: lease_path,
            reason: "must not be a symlink".into(),
        });
    }
    let file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(&lease_path)
        .map_err(|source| Error::Io {
            operation: "open resolution cache lease",
            path: lease_path.clone(),
            source,
        })?;
    let canonical_lease = canonical_file("resolution cache lease", &lease_path)?;
    if canonical_lease != lease_path {
        return Err(Error::Path {
            field: "resolution cache lease",
            path: lease_path,
            reason: "is reached through a symlink".into(),
        });
    }
    Ok(fd_lock::RwLock::new(file))
}

/// The fixed namespace's placement when a completed receipt for this request
/// is there, `None` when it is another request's, and an error when it is not
/// this version's.
fn fixed_placement(
    fixed: &Path,
    request_identity: Digest,
    definition: &universe::Definition,
) -> Result<Option<Placement>> {
    let fixed_lock_path = fixed.join("Cargo.lock");
    if !fixed_lock_path.exists() {
        return Err(Error::ResolutionDrift {
            command: "persisted universe preflight",
            field: "final lock is absent from an existing namespace",
        });
    }
    let fixed_lock_bytes = read_bounded(
        "persisted final resolution lock",
        &fixed_lock_path,
        process::CAPTURE_BYTES,
    )?;
    let fixed_source = read_bounded(
        "persisted generated harness source",
        &fixed.join("src/lib.rs"),
        process::TAIL_BYTES,
    )?;
    if fixed_source != HARNESS_SOURCE.as_bytes() {
        return Err(Error::ResolutionDrift {
            command: "persisted universe preflight",
            field: "generated harness source",
        });
    }
    let fixed_manifest = read_bounded(
        "persisted final-form harness manifest",
        &fixed.join(HARNESS_MANIFEST),
        process::TAIL_BYTES,
    )?;
    let receipt_path = fixed.join(UNIVERSE_RECEIPT);
    if receipt_path.exists() {
        // Receipt and lock reads are I/O retention bounded by the
        // physical capture constant.
        let bytes = read_bounded(
            "resolution universe receipt",
            &receipt_path,
            process::CAPTURE_BYTES,
        )?;
        let selected_identity = universe::Receipt::universe(&bytes)?;
        if selected_identity != *definition.identity() {
            return Err(Error::ResolutionDrift {
                command: "persisted universe selection",
                field: "current runnable receipt universe",
            });
        }
        let fixed_lock = definition.candidate(fixed_lock_bytes.into_boxed_slice())?;
        let receipt = universe::Receipt::parse(&bytes, definition)?;
        receipt.preflight_files(
            digest(&fixed_manifest),
            digest(&fixed_source),
            fixed_lock.digest(),
        )?;
        if receipt.matches_request(request_identity) {
            return Ok(Some(Placement {
                root: fixed.to_path_buf(),
                request: request_identity,
                state: PlacementState::Persisted {
                    lock: fixed_lock,
                    receipt,
                },
                exclusive: false,
            }));
        }
    }
    Ok(None)
}

fn select_placement(
    request: &Request,
    catalog: &universe::Catalog,
    provisional_manifest: &str,
) -> Result<Placement> {
    let definition = catalog.current();
    let request_identity = versioned_request_identity(request, definition, provisional_manifest);
    let fixed = request.cache_root.join(HARNESS_DIRECTORY);
    match fs::symlink_metadata(&fixed) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(Placement {
                root: fixed,
                request: request_identity,
                state: PlacementState::Fresh,
                exclusive: true,
            });
        }
        Err(source) => {
            return Err(Error::Io {
                operation: "inspect final resolution namespace",
                path: fixed,
                source,
            });
        }
        Ok(metadata) if !metadata.is_dir() || metadata.file_type().is_symlink() => {
            return Err(Error::Path {
                field: "final resolution namespace",
                path: fixed,
                reason: "is not a directory".into(),
            });
        }
        Ok(_) => {}
    }

    // A namespace another version wrote, or one whose receipt is absent or
    // for another request, is a clean miss: a side-by-side candidate is built.
    match fixed_placement(&fixed, request_identity, definition) {
        Ok(Some(placement)) => return Ok(placement),
        Err(error @ Error::Io { .. }) => return Err(error),
        Ok(None) | Err(_) => {}
    }
    let candidate = fixed
        .join(CANDIDATE_DIRECTORY)
        .join(format!("{request_identity}"));
    match fs::symlink_metadata(&candidate) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Placement {
            root: candidate,
            request: request_identity,
            state: PlacementState::Fresh,
            exclusive: true,
        }),
        Err(source) => Err(Error::Io {
            operation: "inspect side-by-side resolution candidate",
            path: candidate,
            source,
        }),
        Ok(metadata) if !metadata.is_dir() || metadata.file_type().is_symlink() => {
            Err(Error::Path {
                field: "side-by-side resolution candidate",
                path: candidate,
                reason: "is not a directory".into(),
            })
        }
        Ok(_) => {
            let lock_bytes = read_bounded(
                "persisted final resolution lock",
                &candidate.join("Cargo.lock"),
                process::CAPTURE_BYTES,
            )?;
            let lock = definition.candidate(lock_bytes.into_boxed_slice())?;
            let manifest = read_bounded(
                "candidate final-form harness manifest",
                &candidate.join(HARNESS_MANIFEST),
                process::TAIL_BYTES,
            )?;
            let source = read_bounded(
                "candidate generated harness source",
                &candidate.join("src/lib.rs"),
                process::TAIL_BYTES,
            )?;
            let receipt_bytes = read_bounded(
                "candidate resolution universe receipt",
                &candidate.join(UNIVERSE_RECEIPT),
                process::CAPTURE_BYTES,
            )?;
            let selected_identity = universe::Receipt::universe(&receipt_bytes)?;
            if selected_identity != *definition.identity() {
                return Err(Error::ResolutionDrift {
                    command: "side-by-side candidate preflight",
                    field: "current universe identity",
                });
            }
            let receipt = universe::Receipt::parse(&receipt_bytes, definition)?;
            if !receipt.matches_request(request_identity) {
                return Err(Error::ResolutionDrift {
                    command: "side-by-side candidate preflight",
                    field: "request identity",
                });
            }
            receipt.preflight_files(digest(&manifest), digest(&source), lock.digest())?;
            Ok(Placement {
                root: candidate,
                request: request_identity,
                state: PlacementState::Persisted { lock, receipt },
                exclusive: false,
            })
        }
    }
}

fn versioned_request_identity(
    request: &Request,
    definition: &universe::Definition,
    provisional_manifest: &str,
) -> Digest {
    let mut identity = cargo_view(request, definition, provisional_manifest);
    // Demand changes what the probe extracts. An empty demand keeps
    // every existing identity; v2 is demand-seeded extraction.
    if !request.demand.is_empty() {
        identity.update(b"demand/v2");
        identity.update((request.demand.len() as u64).to_le_bytes());
        for path in &request.demand {
            identity.update((path.len() as u64).to_le_bytes());
            for segment in path {
                identity.update((segment.len() as u64).to_le_bytes());
                identity.update(segment.as_bytes());
            }
        }
    }
    Digest(identity.finalize().into())
}

/// The request as Cargo sees it: every input but the demand, which only
/// the probe reads. Cargo's outputs are a function of it, so a new demand
/// reuses them (`resolution::remembered`).
fn cargo_identity(
    request: &Request,
    definition: &universe::Definition,
    provisional_manifest: &str,
) -> Digest {
    Digest(
        cargo_view(request, definition, provisional_manifest)
            .finalize()
            .into(),
    )
}

fn cargo_view(
    request: &Request,
    definition: &universe::Definition,
    provisional_manifest: &str,
) -> Sha256 {
    let mut identity = Sha256::new();
    identity.update(b"rawr.import.cargo.resolution-request/v2");
    if request.inventory {
        identity.update(b"intrinsic-inventory/v1");
    }
    #[cfg(feature = "rustc-driver")]
    if request.inputs_only {
        identity.update(b"application-native-inputs/v1");
    }
    identity.update(definition.identity());
    identity.update((provisional_manifest.len() as u64).to_le_bytes());
    identity.update(provisional_manifest.as_bytes());
    identity.update(HARNESS_SOURCE.as_bytes());
    identity.update(request.authority.digest.as_bytes());
    identity.update(request.config.digest().as_bytes());
    identity.update((request.target.len() as u64).to_le_bytes());
    identity.update(request.target.as_bytes());
    for input in &request.inputs {
        identity.update((input.path.as_os_str().len() as u64).to_le_bytes());
        identity.update(input.path.as_os_str().as_encoded_bytes());
        match &input.digest {
            Some(digest) => {
                identity.update([1]);
                identity.update(digest.as_bytes());
            }
            None => identity.update([0]),
        }
    }
    identity
}

fn prepare_versioned_roots(
    request: &Request,
    harness_root: &Path,
    exclusive: bool,
) -> Result<PathBuf> {
    reject_existing_symlink(&request.project_root, &request.cache_root)?;
    let parent = harness_root.parent().ok_or_else(|| Error::Path {
        field: "final resolution harness",
        path: harness_root.to_path_buf(),
        reason: "has no parent directory".into(),
    })?;
    fs::create_dir_all(parent).map_err(|source| Error::Io {
        operation: "create resolution namespace parent",
        path: parent.to_path_buf(),
        source,
    })?;
    if exclusive {
        fs::create_dir(harness_root).map_err(|source| Error::Io {
            operation: "exclusively create final resolution harness",
            path: harness_root.to_path_buf(),
            source,
        })?;
    }
    reject_existing_symlink(&request.project_root, harness_root)?;
    let provisional_root = harness_root.join(PROVISIONAL_DIRECTORY);
    for path in [
        harness_root.join("src"),
        harness_root.join("target"),
        provisional_root.clone(),
        provisional_root.join("src"),
        provisional_root.join("target"),
    ] {
        reject_existing_symlink(&request.project_root, &path)?;
    }
    fs::create_dir(harness_root.join("src")).map_err(|source| Error::Io {
        operation: "create final resolution source namespace",
        path: harness_root.join("src"),
        source,
    })?;
    fs::create_dir(&provisional_root).map_err(|source| Error::Io {
        operation: "create provisional resolution harness",
        path: provisional_root.clone(),
        source,
    })?;
    fs::create_dir(provisional_root.join("src")).map_err(|source| Error::Io {
        operation: "create provisional resolution source namespace",
        path: provisional_root.join("src"),
        source,
    })?;
    let harness_root = canonical_directory("final resolution harness", harness_root)?;
    let provisional_root = canonical_directory(
        "provisional resolution harness",
        &harness_root.join(PROVISIONAL_DIRECTORY),
    )?;
    retire_stale_temporaries(&harness_root)?;
    let manifest = render_manifest(request, None)?;
    write_atomic(
        &provisional_root.join(HARNESS_MANIFEST),
        manifest.as_bytes(),
    )?;
    write_atomic(
        &provisional_root.join("src/lib.rs"),
        HARNESS_SOURCE.as_bytes(),
    )?;
    Ok(provisional_root)
}

fn require_exact_file(
    field: &'static str,
    path: &Path,
    expected: &[u8],
    limit: usize,
) -> Result<()> {
    let actual = read_bounded(field, path, limit)?;
    if actual == expected {
        Ok(())
    } else {
        Err(Error::ResolutionDrift {
            command: "persisted universe selection",
            field,
        })
    }
}

fn prepare_harness(request: &Request) -> Result<PathBuf> {
    reject_existing_symlink(&request.project_root, &request.cache_root)?;
    fs::create_dir_all(&request.cache_root).map_err(|source| Error::Io {
        operation: "create project cache root",
        path: request.cache_root.clone(),
        source,
    })?;
    let cache_root = canonical_directory("project cache root", &request.cache_root)?;
    if !cache_root.starts_with(&request.project_root) {
        return Err(Error::Path {
            field: "project cache root",
            path: cache_root,
            reason: "is outside the project directory".into(),
        });
    }
    let harness_root = cache_root.join(HARNESS_DIRECTORY);
    reject_existing_symlink(&request.project_root, &harness_root)?;
    reject_existing_symlink(&request.project_root, &harness_root.join("src"))?;
    reject_existing_symlink(&request.project_root, &harness_root.join("target"))?;
    fs::create_dir_all(harness_root.join("src")).map_err(|source| Error::Io {
        operation: "create resolution harness",
        path: harness_root.clone(),
        source,
    })?;
    let harness_root = canonical_directory("resolution harness", &harness_root)?;
    retire_stale_temporaries(&harness_root)?;
    let manifest = render_manifest(request, None)?;
    let manifest_path = harness_root.join(HARNESS_MANIFEST);
    let changed = fs::read(&manifest_path).map_or(true, |current| current != manifest.as_bytes());
    write_atomic(&manifest_path, manifest.as_bytes())?;
    write_atomic(&harness_root.join("src/lib.rs"), HARNESS_SOURCE.as_bytes())?;
    let lock = harness_root.join("Cargo.lock");
    if fs::symlink_metadata(&lock).is_ok_and(|metadata| metadata.file_type().is_symlink()) {
        return Err(Error::Path {
            field: "Rawr-owned lock authority",
            path: lock,
            reason: "must not be a symlink".into(),
        });
    }
    if changed && lock.exists() {
        fs::remove_file(&lock).map_err(|source| Error::Io {
            operation: "retire stale Rawr-owned harness lock",
            path: lock,
            source,
        })?;
    }
    Ok(harness_root)
}

/// Sweep stale generated temporaries from a Rawr-owned harness directory. The
/// entry count is bounded by the physical `HARNESS_ENTRIES` sweep ceiling,
/// not by an input figure.
fn retire_stale_temporaries(root: &Path) -> Result<()> {
    let entries = fs::read_dir(root).map_err(|source| Error::Io {
        operation: "inspect resolution harness",
        path: root.to_path_buf(),
        source,
    })?;
    for (index, entry) in entries.enumerate() {
        if index >= HARNESS_ENTRIES {
            return Err(Error::Limit {
                resource: "resolution harness entries",
                limit: HARNESS_ENTRIES,
                actual: index.saturating_add(1),
            });
        }
        let entry = entry.map_err(|source| Error::Io {
            operation: "inspect resolution harness entry",
            path: root.to_path_buf(),
            source,
        })?;
        let name = entry.file_name();
        if name.as_encoded_bytes().starts_with(b".rawr-write-") {
            let path = entry.path();
            let metadata = fs::symlink_metadata(&path).map_err(|source| Error::Io {
                operation: "inspect stale generated temporary",
                path: path.clone(),
                source,
            })?;
            if !metadata.is_file() || metadata.file_type().is_symlink() {
                return Err(Error::Path {
                    field: "generated temporary file",
                    path,
                    reason: "must be a regular file".into(),
                });
            }
            fs::remove_file(&path).map_err(|source| Error::Io {
                operation: "retire stale generated temporary",
                path,
                source,
            })?;
        }
    }
    Ok(())
}

/// Render one harness manifest.
fn render_manifest(request: &Request, internal: Option<&internal::Recipe>) -> Result<String> {
    let mut document = DocumentMut::new();
    if let Some(item) = request.authority.retained.get("cargo-features") {
        document["cargo-features"] = item.clone();
    }
    let mut workspace = Table::new();
    workspace.insert("resolver", value(request.authority.resolver.as_ref()));
    document
        .as_table_mut()
        .insert("workspace", Item::Table(workspace));
    let mut package = Table::new();
    package.insert("name", value(HARNESS_PACKAGE));
    package.insert("version", value(HARNESS_VERSION));
    package.insert("edition", value(HARNESS_EDITION));
    package.insert("publish", value(false));
    document
        .as_table_mut()
        .insert("package", Item::Table(package));
    let mut library = Table::new();
    library.insert("path", value("src/lib.rs"));
    document.as_table_mut().insert("lib", Item::Table(library));
    for (key, item) in &request.authority.retained {
        if key.as_ref() != "cargo-features" {
            document[key.as_ref()] = item.clone();
        }
    }

    let mut dependencies = Table::new();
    let mut build_dependencies = Table::new();
    for (index, dependency) in request.dependencies.iter().enumerate() {
        if request.dependencies[..index]
            .iter()
            .any(|existing| existing.shares_cargo_unit(dependency))
        {
            continue;
        }
        let mut specification = InlineTable::new();
        if let Some(package) = dependency.package() {
            specification.insert("package", Value::from(package));
        }
        match dependency.source() {
            config::Source::Path(path) => {
                let path = path_string("dependency", path)?;
                specification.insert("path", Value::from(path));
            }
            config::Source::Registry { manifest, .. } => {
                specification.insert("version", Value::from(manifest.as_ref()));
            }
            config::Source::Git {
                url,
                reference,
                version,
            } => {
                specification.insert("git", Value::from(url.as_ref()));
                if let Some((key, name)) = reference.key_value() {
                    specification.insert(key, Value::from(name));
                }
                if let Some(version) = version {
                    specification.insert("version", Value::from(version.as_ref()));
                }
            }
        }
        specification.insert(
            "default-features",
            Value::from(dependency.default_features()),
        );
        if !dependency.features().is_empty() {
            let mut features = Array::new();
            for feature in dependency.features() {
                features.push(feature.as_ref());
            }
            specification.insert("features", Value::Array(features));
        }
        // Rawr's own crate is a build dependency of the harness, which has
        // no build script: Cargo resolves, locks and fetches it but builds
        // no unit of it. Only the runner, which the project's Cargo builds,
        // links it (D609.2).
        let table = if dependency.alias() == config::INTERNAL_ALIAS {
            &mut build_dependencies
        } else {
            &mut dependencies
        };
        table.insert(
            dependency.alias(),
            Item::Value(Value::InlineTable(specification)),
        );
    }
    document["dependencies"] = Item::Table(dependencies);
    if !build_dependencies.is_empty() {
        document[BUILD_DEPENDENCIES] = Item::Table(build_dependencies);
    }
    if let Some(internal) = internal {
        internal.apply_profile(&mut document, &request.profile)?;
    }
    formatted(format_args!("{document}"))
}

fn finalize_harness(
    request: &Request,
    harness_root: &Path,
    internal: &internal::Recipe,
) -> Result<()> {
    let manifest = render_manifest(request, Some(internal))?;
    write_atomic(&harness_root.join(HARNESS_MANIFEST), manifest.as_bytes())?;
    let lock = harness_root.join("Cargo.lock");
    if lock.exists() {
        let lock = canonical_file("provisional harness lock", &lock)?;
        fs::remove_file(&lock).map_err(|source| Error::Io {
            operation: "retire provisional Rawr-owned harness lock",
            path: lock,
            source,
        })?;
    }
    Ok(())
}

fn cargo_metadata(
    native: &impl ToolPaths,
    request: &Request,
    manifest: &Path,
    target_dir: &Path,
    locked: bool,
    limits: &Limits,
) -> Result<Box<[u8]>> {
    let prefix = config_arguments(request.config.explicit_configs());
    let command = cargo_metadata_command(native, request, manifest, target_dir, prefix, locked)?;
    run_cargo(native, &command, limits)
}

fn cargo_metadata_command(
    native: &impl ToolPaths,
    request: &Request,
    manifest: &Path,
    target_dir: &Path,
    prefix: Vec<OsString>,
    locked: bool,
) -> Result<process::Command> {
    let tail = cargo_metadata_arguments(
        request.toolchain.schemas().graph(),
        manifest,
        &request.target,
        locked,
    );
    process::Command::cargo(
        prefix,
        process::CargoAction::Metadata,
        tail,
        request.project_root.clone(),
        request.config.command_environment(target_dir, native)?,
    )
}

fn cargo_units(
    native: &impl ToolPaths,
    request: &Request,
    manifest: &Path,
    target_dir: &Path,
    limits: &Limits,
) -> Result<Box<[u8]>> {
    let prefix = config_arguments(request.config.explicit_configs());
    let command = cargo_unit_command(native, request, manifest, target_dir, prefix)?;
    run_cargo(native, &command, limits)
}

fn cargo_unit_command(
    native: &impl ToolPaths,
    request: &Request,
    manifest: &Path,
    target_dir: &Path,
    mut prefix: Vec<OsString>,
) -> Result<process::Command> {
    prefix.extend(["-Z".into(), "unstable-options".into()]);
    let tail = cargo_unit_arguments(manifest, &request.target, &request.profile);
    process::Command::cargo(
        prefix,
        process::CargoAction::UnitBuild(request.parallelism),
        tail,
        request.project_root.clone(),
        request.config.command_environment(target_dir, native)?,
    )
}

#[cfg(not(feature = "rustc-driver"))]
#[allow(
    clippy::type_complexity,
    reason = "the closed result pairs exact Cargo JSON with its sealed environment"
)]
fn cargo_execute_build(
    native: &impl ToolPaths,
    request: &Request,
    manifest: &Path,
    target_dir: &Path,
    limits: &Limits,
) -> Result<(Box<[u8]>, Box<[(OsString, OsString)]>)> {
    let prefix = config_arguments(request.config.explicit_configs());
    let tail = cargo_build_arguments(manifest, &request.target, &request.profile);
    let environment = request.config.command_environment(target_dir, native)?;
    let command = cargo_build_command(request, prefix, tail, environment)?;
    let bytes = native
        .run(&command, limits.process)?
        .checked(Program::Cargo)?;
    Ok((bytes, command.into_environment()))
}

/// What a build script that declares no `rerun-if` input watches: its whole
/// package. A registry package is proved by its extraction marker; a path or
/// Git package by its source tree, which replay walks again with no process.
fn fallback_files(
    request: &Request,
    package: &unit::Package,
    manifest: &Path,
    cargo_commit: &str,
    usage: crate::input::Usage,
    limits: &Limits,
) -> Result<crate::input::package::Set> {
    if package.manifest() != manifest {
        return Err(Error::Input {
            code: "fallback-manifest-mismatch",
            reason: package.id().into(),
        });
    }
    match package.source() {
        unit::PackageSource::Registry { .. } => {
            let cargo_home = request.config.cargo_home()?.ok_or_else(|| Error::Input {
                code: "missing-cargo-home-authority",
                reason: package.id().into(),
            })?;
            crate::input::package::Set::capture_registry(
                cargo_commit,
                package,
                &cargo_home,
                usage,
                limits,
            )
        }
        unit::PackageSource::Path { .. } | unit::PackageSource::Git { .. } => {
            crate::input::package::Set::capture_tree(cargo_commit, package, manifest, usage, limits)
        }
    }
}

#[cfg(feature = "rustc-driver")]
/// The extraction plan's build, in the pinned nightly's shared build
/// directory as a resolution's is: its target directory starts empty each
/// time, and only the copies of what this build reads fill it, so a crate
/// any earlier build compiled is not compiled again.
fn cargo_execute_authoritative_build(
    native: &impl ToolPaths,
    request: &Request,
    manifest: &Path,
    target: &Path,
    environment: Box<[(OsString, OsString)]>,
    limits: &Limits,
) -> Result<Box<[u8]>> {
    let prefix = config_arguments(request.config.explicit_configs());
    resolution::cold::shared_build(
        native,
        request,
        prefix,
        manifest,
        environment,
        (target, target),
        limits,
    )
    .map(|(bytes, _)| bytes)
}

/// `cargo clean -p <package>` for the build `prefix` and `manifest` give, so
/// the next build there runs the package's build script again (D742).
/// Cargo cleans every version of a package by its name.
fn cargo_clean_command(
    request: &Request,
    prefix: Vec<OsString>,
    manifest: &Path,
    package: &str,
    environment: Box<[(OsString, OsString)]>,
) -> Result<process::Command> {
    process::Command::cargo(
        prefix,
        process::CargoAction::Clean,
        Vec::from([
            "--manifest-path".into(),
            manifest.as_os_str().to_owned(),
            "--target".into(),
            request.target.as_ref().into(),
            "--profile".into(),
            request.profile.as_ref().into(),
            "--locked".into(),
            "--package".into(),
            package.into(),
        ]),
        request.project_root.clone(),
        environment,
    )
}

fn cargo_build_command(
    request: &Request,
    prefix: Vec<OsString>,
    tail: Vec<OsString>,
    environment: Box<[(OsString, OsString)]>,
) -> Result<process::Command> {
    process::Command::cargo(
        prefix,
        process::CargoAction::AuthoritativeBuild(request.parallelism),
        tail,
        request.project_root.clone(),
        environment,
    )
}

#[cfg(feature = "rustc-driver")]
fn cargo_authoritative_environment(
    native: &impl ToolPaths,
    request: &Request,
    plan: &crate::extract::plan::Plan,
) -> Result<Box<[(OsString, OsString)]>> {
    request
        .config
        .command_environment(plan.target_directory(), native)
}

/// Run one Cargo command within the physical capture bounds.
///
/// The only refusal here is physical: `limits.process` truncates and refuses a
/// stream past `process::CAPTURE_BYTES`. The Cargo JSON epoch that parses the
/// captured bytes (`unit::preflight_from`) charges them.
fn run_cargo(
    native: &impl ToolPaths,
    command: &process::Command,
    limits: &Limits,
) -> Result<Box<[u8]>> {
    native.run(command, limits.process)?.checked(Program::Cargo)
}

fn cargo_metadata_arguments(
    schema: u32,
    manifest: &Path,
    target: &str,
    locked: bool,
) -> Vec<OsString> {
    let mut arguments = Vec::from([
        "--format-version".into(),
        schema.to_string().into(),
        "--manifest-path".into(),
        manifest.as_os_str().to_owned(),
        "--filter-platform".into(),
        target.into(),
    ]);
    if locked {
        arguments.push("--locked".into());
    }
    arguments
}

fn locked_metadata_node_authority(
    request: Digest,
    toolchain: &toolchain::Proof,
    resolution: &Request,
    manifest: &Path,
) -> Result<Box<[u8]>> {
    let arguments = process::Command::cargo_arguments(
        config_arguments(resolution.config.explicit_configs()),
        process::CargoAction::Metadata,
        cargo_metadata_arguments(
            resolution.toolchain.schemas().graph(),
            manifest,
            &resolution.target,
            true,
        ),
    )?;
    let mut bytes = Vec::new();
    bytes.extend_from_slice(b"rawr.import.locked-metadata-node-authority/v1");
    bytes.extend_from_slice(request.as_bytes());
    bytes.extend_from_slice(&toolchain.node_digest());
    bytes.extend_from_slice(&(arguments.len() as u64).to_le_bytes());
    for argument in arguments {
        let argument = argument.as_os_str().as_encoded_bytes();
        bytes.extend_from_slice(&(argument.len() as u64).to_le_bytes());
        bytes.extend_from_slice(argument);
    }
    Ok(bytes.into_boxed_slice())
}

fn unit_graph_node_authority(
    request: Digest,
    toolchain: &toolchain::Proof,
    resolution: &Request,
    manifest: &Path,
) -> Result<Box<[u8]>> {
    let arguments = process::Command::cargo_arguments(
        config_arguments(resolution.config.explicit_configs()),
        process::CargoAction::UnitBuild(resolution.parallelism),
        cargo_unit_arguments(manifest, &resolution.target, &resolution.profile),
    )?;
    let mut bytes = Vec::new();
    bytes.extend_from_slice(b"rawr.import.unit-graph-node-authority/v1");
    bytes.extend_from_slice(request.as_bytes());
    bytes.extend_from_slice(&toolchain.node_digest());
    bytes.extend_from_slice(&(arguments.len() as u64).to_le_bytes());
    for argument in arguments {
        let argument = argument.as_os_str().as_encoded_bytes();
        bytes.extend_from_slice(&(argument.len() as u64).to_le_bytes());
        bytes.extend_from_slice(argument);
    }
    Ok(bytes.into_boxed_slice())
}

#[cfg(feature = "rustc-driver")]
fn authoritative_build_node_authority(
    request: Digest,
    toolchain: &toolchain::Proof,
    resolution: &Request,
    manifest: &Path,
) -> Result<Box<[u8]>> {
    let arguments = process::Command::cargo_arguments(
        config_arguments(resolution.config.explicit_configs()),
        process::CargoAction::AuthoritativeBuild(resolution.parallelism),
        cargo_build_arguments(manifest, &resolution.target, &resolution.profile),
    )?;
    let mut bytes = Vec::new();
    bytes.extend_from_slice(b"rawr.import.authoritative-build-node-authority/v1");
    bytes.extend_from_slice(request.as_bytes());
    bytes.extend_from_slice(&toolchain.node_digest());
    bytes.extend_from_slice(&(arguments.len() as u64).to_le_bytes());
    for argument in arguments {
        let argument = argument.as_os_str().as_encoded_bytes();
        bytes.extend_from_slice(&(argument.len() as u64).to_le_bytes());
        bytes.extend_from_slice(argument);
    }
    Ok(bytes.into_boxed_slice())
}

fn final_command_cohort(
    graph_schema: u32,
    target: &str,
    profile: &str,
    parallelism: process::parallelism::Proof,
    explicit_configs: &[PathBuf],
) -> Result<Digest> {
    command_cohort(
        graph_schema,
        target,
        profile,
        parallelism,
        || Ok(config_arguments(explicit_configs)),
        None,
    )
}

fn command_cohort(
    graph_schema: u32,
    target: &str,
    profile: &str,
    parallelism: process::parallelism::Proof,
    mut prefix: impl FnMut() -> Result<Vec<OsString>>,
    reserved_config: Option<&[u8]>,
) -> Result<Digest> {
    let manifest = Path::new("$RAWR_FINAL_MANIFEST");
    let operations = [
        (
            "metadata",
            process::CargoAction::Metadata,
            prefix()?,
            cargo_metadata_arguments(graph_schema, manifest, target, true),
        ),
        (
            "unit",
            process::CargoAction::UnitBuild(parallelism),
            {
                let mut prefix = prefix()?;
                prefix.extend([OsString::from("-Z"), OsString::from("unstable-options")]);
                prefix
            },
            cargo_unit_arguments(manifest, target, profile),
        ),
        (
            "build",
            process::CargoAction::AuthoritativeBuild(parallelism),
            prefix()?,
            cargo_build_arguments(manifest, target, profile),
        ),
        (
            "replay-metadata",
            process::CargoAction::Metadata,
            prefix()?,
            cargo_metadata_arguments(graph_schema, manifest, target, true),
        ),
        (
            "replay-unit",
            process::CargoAction::UnitBuild(parallelism),
            {
                let mut prefix = prefix()?;
                prefix.extend([OsString::from("-Z"), OsString::from("unstable-options")]);
                prefix
            },
            cargo_unit_arguments(manifest, target, profile),
        ),
        (
            "replay-build",
            process::CargoAction::AuthoritativeBuild(parallelism),
            prefix()?,
            cargo_build_arguments(manifest, target, profile),
        ),
    ];
    let mut digest = Sha256::new();
    digest.update(b"rawr.import.cargo.final-command-cohort/v1");
    digest.update(FINAL_COMMAND_SCHEMA.to_le_bytes());
    if let Some(bytes) = reserved_config {
        digest.update(b"rawr.import.cargo.reserved-build-config/v1");
        digest.update(1_u32.to_le_bytes());
        digest.update((bytes.len() as u64).to_le_bytes());
        digest.update(bytes);
    }
    for (name, action, prefix, tail) in operations {
        let arguments = process::Command::cargo_arguments(prefix, action, tail)?;
        digest.update((name.len() as u64).to_le_bytes());
        digest.update(name.as_bytes());
        digest.update((arguments.len() as u64).to_le_bytes());
        for argument in arguments {
            let argument = argument.as_os_str().as_encoded_bytes();
            digest.update((argument.len() as u64).to_le_bytes());
            digest.update(argument);
        }
    }
    Ok(Digest(digest.finalize().into()))
}

fn cargo_unit_arguments(manifest: &Path, target: &str, profile: &str) -> Vec<OsString> {
    Vec::from([
        "--unit-graph".into(),
        "--manifest-path".into(),
        manifest.as_os_str().to_owned(),
        "--target".into(),
        target.into(),
        "--profile".into(),
        profile.into(),
        "--locked".into(),
    ])
}

fn cargo_build_arguments(manifest: &Path, target: &str, profile: &str) -> Vec<OsString> {
    Vec::from([
        "--message-format".into(),
        "json-render-diagnostics".into(),
        "--manifest-path".into(),
        manifest.as_os_str().to_owned(),
        "--target".into(),
        target.into(),
        "--profile".into(),
        profile.into(),
        "--locked".into(),
    ])
}

fn config_arguments(configs: &[PathBuf]) -> Vec<OsString> {
    let mut arguments = Vec::with_capacity(configs.len() * 2);
    for config in configs {
        arguments.push("--config".into());
        arguments.push(config.as_os_str().to_owned());
    }
    arguments
}

#[allow(
    clippy::too_many_arguments,
    reason = "revalidation compares every retained harness authority in one proof step"
)]
fn revalidate_harness(
    request: &Request,
    manifest_path: &Path,
    manifest: Digest,
    source_path: &Path,
    source: Digest,
    lock_path: &Path,
    lock: Digest,
    limits: &Limits,
    command: &'static str,
) -> Result<()> {
    request.revalidate(limits, command)?;
    for (field, path, expected, limit) in [
        (
            "final-form harness manifest",
            manifest_path,
            manifest,
            process::TAIL_BYTES,
        ),
        (
            "generated harness source",
            source_path,
            source,
            process::TAIL_BYTES,
        ),
        (
            "Rawr-owned lock authority",
            lock_path,
            lock,
            process::CAPTURE_BYTES,
        ),
    ] {
        let bytes = read_bounded(field, path, limit)?;
        if digest(&bytes) != expected {
            return Err(Error::ResolutionDrift { command, field });
        }
    }
    Ok(())
}

fn canonical_authority(path: &Path) -> Result<PathBuf> {
    let manifest = if path.is_dir() {
        path.join("Cargo.toml")
    } else {
        path.to_path_buf()
    };
    // Cargo follows symlinks to the manifest; rawr reads the file they name.
    let manifest = fs::canonicalize(&manifest).map_err(|source| Error::Io {
        operation: "canonicalize file",
        path: manifest,
        source,
    })?;
    canonical_file("Cargo authority manifest", &manifest)
}

fn authority_inputs(
    authority: &Authority,
    dependencies: &[config::Selected],
) -> Result<Box<[Input]>> {
    let mut paths = BTreeSet::new();
    for input in &authority.inputs {
        paths.insert((input.path.clone(), input.role));
    }
    for dependency in dependencies {
        if let config::Source::Path(path) = dependency.source() {
            let candidate = path.join("Cargo.toml");
            let manifest = canonical_file("dependency manifest", &candidate)?;
            paths.insert((manifest, Role::Manifest));
        }
    }
    let mut inputs = Vec::with_capacity(paths.len());
    for (path, role) in paths {
        if let Some(original) = authority
            .inputs
            .iter()
            .find(|input| input.path == path && input.role == role)
        {
            inputs.push(Input {
                path,
                digest: original.digest,
                role,
            });
            continue;
        }
        let bytes = read_bounded("Cargo authority input", &path, process::TAIL_BYTES)?;
        inputs.push(Input {
            path,
            digest: Some(digest(&bytes)),
            role,
        });
    }
    Ok(inputs.into_boxed_slice())
}

fn authority_input_digest(path: &Path, limit: usize) -> Result<Option<Digest>> {
    match read_bounded("Cargo authority input", path, limit) {
        Ok(bytes) => Ok(Some(digest(&bytes))),
        Err(Error::Io { source, .. }) if source.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error),
    }
}

/// Old Build records never gain a new workspace certificate merely because
/// their output bytes happen to agree. Every miss is also checked on both
/// sides of its actual command before that native result can be published.
fn workspace_node_authority(
    authority: &[u8],
    workspace: &crate::input::workspace::Image,
) -> Result<Box<[u8]>> {
    const DOMAIN: &[u8] = b"rawr.cargo.workspace-consumption/v1";
    let length = DOMAIN
        .len()
        .checked_add(8)
        .and_then(|n| n.checked_add(authority.len()))
        .and_then(|n| n.checked_add(32))
        .ok_or_else(|| Error::Node {
            operation: "workspace command authority",
            source: Box::new(crate::node::Error::Allocation),
        })?;
    let mut bytes = Vec::new();
    bytes
        .try_reserve_exact(length)
        .map_err(|source| Error::Allocation {
            operation: "workspace command authority",
            source,
        })?;
    bytes.extend_from_slice(DOMAIN);
    bytes.extend_from_slice(&(authority.len() as u64).to_le_bytes());
    bytes.extend_from_slice(authority);
    bytes.extend_from_slice(workspace.digest()?);
    Ok(bytes.into_boxed_slice())
}

fn authority_resolver(document: &DocumentMut) -> Result<&str> {
    if let Some(resolver) = document
        .get("workspace")
        .and_then(Item::as_table)
        .and_then(|workspace| workspace.get("resolver"))
        .and_then(Item::as_str)
    {
        return if matches!(resolver, "1" | "2" | "3") {
            Ok(resolver)
        } else {
            Err(Error::Schema {
                document: "Cargo authority manifest",
                reason: format!("unsupported resolver `{resolver}`").into_boxed_str(),
            })
        };
    }
    let edition = document
        .get("package")
        .and_then(Item::as_table)
        .and_then(|package| package.get("edition"))
        .and_then(Item::as_str);
    let resolver = if edition == Some("2024") {
        "3"
    } else if edition == Some("2021") {
        "2"
    } else {
        "1"
    };
    Ok(resolver)
}

fn absolutize_policy_paths(item: &mut Item, authority_root: &Path) -> Result<()> {
    match item {
        Item::Table(table) => {
            for (key, item) in table.iter_mut() {
                if key == "path" {
                    absolutize_path_item(item, authority_root)?;
                } else {
                    absolutize_policy_paths(item, authority_root)?;
                }
            }
        }
        Item::ArrayOfTables(tables) => {
            for table in tables.iter_mut() {
                for (key, item) in table.iter_mut() {
                    if key == "path" {
                        absolutize_path_item(item, authority_root)?;
                    } else {
                        absolutize_policy_paths(item, authority_root)?;
                    }
                }
            }
        }
        Item::Value(Value::InlineTable(table)) => {
            if let Some(path) = table.get("path").and_then(Value::as_str) {
                let candidate = authority_root.join(path);
                let canonical = canonical_directory("Cargo authority patch path", &candidate)?;
                table.insert(
                    "path",
                    Value::from(path_string("Cargo authority patch path", &canonical)?),
                );
            }
        }
        Item::Value(_) | Item::None => {}
    }
    Ok(())
}

fn absolutize_path_item(item: &mut Item, authority_root: &Path) -> Result<()> {
    let path = item.as_str().ok_or_else(|| Error::Schema {
        document: "Cargo authority policy",
        reason: "`path` must be a string".into(),
    })?;
    let candidate = authority_root.join(path);
    let canonical = canonical_directory("Cargo authority patch path", &candidate)?;
    let canonical = path_string("Cargo authority patch path", &canonical)?;
    *item = value(canonical);
    Ok(())
}

fn write_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    if path.exists()
        && fs::symlink_metadata(path)
            .map_err(|source| Error::Io {
                operation: "inspect generated file",
                path: path.to_path_buf(),
                source,
            })?
            .file_type()
            .is_symlink()
    {
        return Err(Error::Path {
            field: "generated file",
            path: path.to_path_buf(),
            reason: "must not be a symlink".into(),
        });
    }
    if fs::read(path).is_ok_and(|current| current == bytes) {
        return Ok(());
    }
    let parent = path.parent().ok_or_else(|| Error::Path {
        field: "generated file",
        path: path.to_path_buf(),
        reason: "has no parent directory".into(),
    })?;
    let temporary = parent.join(format!(
        ".rawr-write-{}-{}",
        std::process::id(),
        TEMPORARY_ID.fetch_add(1, Ordering::Relaxed)
    ));
    let mut file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&temporary)
        .map_err(|source| Error::Io {
            operation: "create generated temporary file",
            path: temporary.clone(),
            source,
        })?;
    let result = file.write_all(bytes).and_then(|()| file.sync_all());
    if let Err(source) = result {
        let _ = fs::remove_file(&temporary);
        return Err(Error::Io {
            operation: "write generated temporary file",
            path: temporary,
            source,
        });
    }
    if let Err(source) = fs::rename(&temporary, path) {
        let _ = fs::remove_file(&temporary);
        return Err(Error::Io {
            operation: "publish generated file",
            path: path.to_path_buf(),
            source,
        });
    }
    Ok(())
}

/// Read a whole input file, whatever its size: a large valid project's
/// manifest, lock or receipt is never refused (#1231).
fn read_whole(resource: &'static str, path: &Path) -> Result<Vec<u8>> {
    read_bounded(resource, path, usize::MAX)
}

fn read_bounded(resource: &'static str, path: &Path, limit: usize) -> Result<Vec<u8>> {
    let canonical = canonical_file(resource, path)?;
    if canonical != path {
        return Err(Error::Path {
            field: resource,
            path: path.to_path_buf(),
            reason: "is reached through a symlink".into(),
        });
    }
    let mut file = File::open(path).map_err(|source| Error::Io {
        operation: "open bounded input",
        path: path.to_path_buf(),
        source,
    })?;
    let metadata = file.metadata().map_err(|source| Error::Io {
        operation: "inspect bounded input handle",
        path: path.to_path_buf(),
        source,
    })?;
    let length = usize::try_from(metadata.len()).unwrap_or(usize::MAX);
    if length > limit {
        return Err(Error::Limit {
            resource,
            limit,
            actual: length,
        });
    }
    let mut bytes = Vec::new();
    bytes
        .try_reserve_exact(length)
        .map_err(|source| Error::Allocation {
            operation: "bounded Cargo input",
            source,
        })?;
    bytes.resize(length, 0);
    if let Err(source) = file.read_exact(&mut bytes) {
        if source.kind() == std::io::ErrorKind::UnexpectedEof {
            return Err(Error::ResolutionDrift {
                command: "bounded input read",
                field: resource,
            });
        }
        return Err(Error::Io {
            operation: "read bounded input",
            path: path.to_path_buf(),
            source,
        });
    }
    let mut extra = [0_u8; 1];
    let extra = file.read(&mut extra).map_err(|source| Error::Io {
        operation: "probe bounded input growth",
        path: path.to_path_buf(),
        source,
    })?;
    if extra != 0 {
        return Err(Error::ResolutionDrift {
            command: "bounded input read",
            field: resource,
        });
    }
    Ok(bytes)
}

pub(crate) fn canonical_directory(field: &'static str, path: &Path) -> Result<PathBuf> {
    let normalized = normalized_absolute(field, path)?;
    let canonical = fs::canonicalize(path).map_err(|source| Error::Io {
        operation: "canonicalize directory",
        path: path.to_path_buf(),
        source,
    })?;
    if !canonical.is_dir() {
        return Err(Error::Path {
            field,
            path: canonical,
            reason: "is not a directory".into(),
        });
    }
    if canonical != normalized {
        return Err(Error::Path {
            field,
            path: path.to_path_buf(),
            reason: "is reached through a symlink".into(),
        });
    }
    Ok(canonical)
}

/// The directory a user-given path names, its symlinks resolved as Cargo
/// follows them. Rawr then works only in canonical paths below it.
pub(crate) fn resolved_directory(field: &'static str, path: &Path) -> Result<PathBuf> {
    let canonical = fs::canonicalize(path).map_err(|source| Error::Io {
        operation: "canonicalize directory",
        path: path.to_path_buf(),
        source,
    })?;
    if !canonical.is_dir() {
        return Err(Error::Path {
            field,
            path: canonical,
            reason: "expected a directory".into(),
        });
    }
    Ok(canonical)
}

pub(crate) fn canonical_file(field: &'static str, path: &Path) -> Result<PathBuf> {
    let metadata = fs::symlink_metadata(path).map_err(|source| Error::Io {
        operation: "inspect file",
        path: path.to_path_buf(),
        source,
    })?;
    if metadata.file_type().is_symlink() {
        return Err(Error::Path {
            field,
            path: path.to_path_buf(),
            reason: "must not be a symlink".into(),
        });
    }
    let normalized = normalized_absolute(field, path)?;
    let canonical = fs::canonicalize(path).map_err(|source| Error::Io {
        operation: "canonicalize file",
        path: path.to_path_buf(),
        source,
    })?;
    if !canonical.is_file() {
        return Err(Error::Path {
            field,
            path: canonical,
            reason: "is not a file".into(),
        });
    }
    if canonical != normalized {
        return Err(Error::Path {
            field,
            path: path.to_path_buf(),
            reason: "is reached through a symlink".into(),
        });
    }
    Ok(canonical)
}

fn lexical_absolute(path: &Path) -> Result<PathBuf> {
    normalized_absolute("project cache root", path)
}

pub(crate) fn normalized_absolute(field: &'static str, path: &Path) -> Result<PathBuf> {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .map_err(|source| Error::Io {
                operation: "read current directory",
                path: path.to_path_buf(),
                source,
            })?
            .join(path)
    };
    let mut normalized = PathBuf::new();
    for component in absolute.components() {
        match component {
            Component::Prefix(prefix) => normalized.push(prefix.as_os_str()),
            Component::RootDir => normalized.push(component.as_os_str()),
            Component::CurDir => {}
            Component::ParentDir => {
                if !normalized.pop() {
                    return Err(Error::Path {
                        field,
                        path: path.to_path_buf(),
                        reason: "is outside the filesystem root".into(),
                    });
                }
            }
            Component::Normal(component) => normalized.push(component),
        }
    }
    Ok(normalized)
}

fn reject_existing_symlink(root: &Path, target: &Path) -> Result<()> {
    let relative = target.strip_prefix(root).map_err(|_| Error::Path {
        field: "Rawr-owned cache path",
        path: target.to_path_buf(),
        reason: "is outside the project directory".into(),
    })?;
    let mut current = root.to_path_buf();
    for component in relative.components() {
        current.push(component.as_os_str());
        match fs::symlink_metadata(&current) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return Err(Error::Path {
                    field: "Rawr-owned cache path",
                    path: current,
                    reason: "is reached through a symlink".into(),
                });
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => break,
            Err(source) => {
                return Err(Error::Io {
                    operation: "inspect Rawr-owned cache path",
                    path: current,
                    source,
                });
            }
        }
    }
    Ok(())
}

fn path_string<'a>(field: &'static str, path: &'a Path) -> Result<&'a str> {
    path.to_str().ok_or_else(|| Error::Path {
        field,
        path: path.to_path_buf(),
        reason: "is not valid UTF-8, which Cargo.toml requires".into(),
    })
}

pub(crate) fn digest(bytes: &[u8]) -> Digest {
    let mut hasher = <Sha256 as sha2::Digest>::new();
    sha2::Digest::update(&mut hasher, bytes);
    let output = sha2::Digest::finalize(hasher);
    let mut digest = [0_u8; 32];
    digest.copy_from_slice(&output);
    Digest(digest)
}

fn append_framed(output: &mut Vec<u8>, bytes: &[u8]) {
    output.extend_from_slice(&(bytes.len() as u64).to_le_bytes());
    output.extend_from_slice(bytes);
}

#[cfg(test)]
mod universe_placement_tests {
    use super::*;

    const SEED: &str = "version = 4\n\n[[package]]\nname = \"rawr-iced\"\nversion = \"1.0.0\"\ndependencies = [\"shared\"]\n\n[[package]]\nname = \"shared\"\nversion = \"1.2.3\"\nsource = \"registry+exact\"\nchecksum = \"aa\"\n";
    const UNKNOWN_SEED: &str =
        "version = 4\n\n[[package]]\nname = \"other\"\nversion = \"1.0.0\"\n";
    const MIXED_SEED: &str = "version = 4\n\n[[package]]\nname = \"rawr-iced\"\nversion = \"1.0.0\"\ndependencies = [\"shared\", \"skew\"]\n\n[[package]]\nname = \"shared\"\nversion = \"1.2.3\"\nsource = \"registry+exact\"\nchecksum = \"aa\"\n\n[[package]]\nname = \"skew\"\nversion = \"9.9.9\"\nsource = \"registry+skew\"\nchecksum = \"ff\"\n";

    #[test]
    fn versioned_metadata_and_build_share_one_target_authority() {
        let root = Path::new("/fixture/resolution");
        assert_eq!(
            versioned_target_directory(root),
            root.join(AUTHORITATIVE_TARGET_DIRECTORY)
        );
        assert_ne!(versioned_target_directory(root), root.join("target"));
    }

    /// Cargo builds a project reached through a symlink; rawr works in the
    /// directory the symlink names, its cache spelled through either path.
    #[cfg(unix)]
    #[test]
    fn symlinked_project_root_resolves_to_the_directory_it_names() {
        crate::store::isolate_for_tests();
        let project = Project::new("symlinked");
        let link = project.0.with_extension("link");
        std::os::unix::fs::symlink(&project.0, &link).expect("project symlink exists");
        let limits = Limits::standard();
        let dependency = fs::canonicalize(
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/catalog-api"),
        )
        .expect("catalog fixture canonicalizes");
        let build = |cache: PathBuf| {
            Request::builder(&link, &cache, &limits)
                .expect("request builder seals")
                .environment(config::Environment::capture().expect("environment captures"))
                .policy(crate::extract::Policy::manifest().seal())
                .dependency(
                    config::Dependency::path("catalog", &dependency)
                        .and_then(|dependency| dependency.package("catalog-api"))
                        .expect("catalog dependency seals"),
                )
                .build()
        };
        for cache in [link.join("target/rawr/cache"), project.cache()] {
            let request = build(cache).expect("a symlinked project seals");
            assert_eq!(request.project_root, project.0);
            assert_eq!(request.cache_root, project.cache());
        }
        drop(fs::remove_file(&link));
    }

    #[derive(serde::Serialize)]
    struct FixtureReceipt {
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

    struct Project(PathBuf);

    impl Project {
        fn new(label: &str) -> Self {
            let root = std::env::temp_dir().join(format!(
                "rawr-universe-{label}-{}-{}",
                std::process::id(),
                TEMPORARY_ID.fetch_add(1, Ordering::Relaxed),
            ));
            fs::create_dir(&root).expect("universe fixture root creates exclusively");
            Self(fs::canonicalize(root).expect("universe fixture root canonicalizes"))
        }

        fn cache(&self) -> PathBuf {
            self.0.join("target/rawr/cache")
        }
    }

    impl Drop for Project {
        fn drop(&mut self) {
            drop(fs::remove_dir_all(&self.0));
        }
    }

    fn definition() -> universe::Definition {
        universe::Definition::new(
            "rawr-iced",
            Some("iced"),
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

    fn request(project: &Project, changed: bool, limits: &Limits) -> Request {
        crate::store::isolate_for_tests();
        let dependency = fs::canonicalize(
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/catalog-api"),
        )
        .expect("catalog fixture canonicalizes");
        let mut dependency = config::Dependency::path("catalog", &dependency)
            .and_then(|dependency| dependency.package("catalog-api"))
            .expect("catalog dependency seals");
        if changed {
            dependency = dependency
                .features(["migration-selection"], true)
                .expect("changed feature seals");
        }
        Request::builder(&project.0, &project.cache(), limits)
            .expect("request builder seals")
            .environment(config::Environment::capture().expect("environment captures"))
            .policy(crate::extract::Policy::manifest().seal())
            .dependency(dependency)
            .build()
            .expect("request seals")
    }

    fn install_legacy(root: &Path, provisional: &str, lock: &str) {
        fs::create_dir_all(root.join("src")).expect("legacy namespace creates");
        fs::write(root.join("Cargo.lock"), lock).expect("legacy lock writes");
        fs::write(root.join("src/lib.rs"), HARNESS_SOURCE).expect("legacy source writes");
        let manifest = format!(
            "{provisional}\n[profile.dev.package.\"path+file:///fixture#rawr@1.25.1\"]\ndebug = 0\n"
        );
        fs::write(root.join(HARNESS_MANIFEST), manifest).expect("legacy manifest writes");
    }

    fn install_current_receipt(
        root: &Path,
        request: &Request,
        definition: &universe::Definition,
        provisional: &str,
    ) {
        let lock = definition
            .candidate(SEED.as_bytes().into())
            .expect("fixture current lock authenticates");
        write_fixture_receipt(
            root,
            request,
            definition,
            provisional,
            *definition.identity(),
            *lock.digest().as_bytes(),
        );
    }

    fn write_fixture_receipt(
        root: &Path,
        request: &Request,
        definition: &universe::Definition,
        provisional: &str,
        universe: [u8; 32],
        lock: [u8; 32],
    ) {
        let manifest = fs::read(root.join(HARNESS_MANIFEST)).expect("fixture manifest reads");
        let source = fs::read(root.join("src/lib.rs")).expect("fixture source reads");
        let receipt = FixtureReceipt {
            schema: 2,
            request: *versioned_request_identity(request, definition, provisional).as_bytes(),
            universe,
            manifest: *digest(&manifest).as_bytes(),
            source: *digest(&source).as_bytes(),
            lock,
            metadata: [0; 32],
            unit: [0; 32],
            build: [0; 32],
            input: [0; 32],
            complete: [0; 32],
        };
        fs::write(
            root.join(UNIVERSE_RECEIPT),
            serde_json::to_vec(&receipt).expect("fixture receipt encodes canonically"),
        )
        .expect("fixture receipt writes");
    }

    /// Every entry below `root` but the tool digest table. The first import
    /// context of a process opens that table in the project's
    /// `target/rawr`, or under `CARGO_TARGET_DIR` when it is set, so its
    /// presence depends on the environment, not on the work under test.
    fn filesystem_entries(root: &Path) -> usize {
        fs::read_dir(root)
            .expect("fixture tree reads")
            .map(|entry| entry.expect("fixture entry reads").path())
            .filter(|path| !path.ends_with("target/rawr/digests"))
            .map(|path| {
                if path.is_dir() {
                    1 + filesystem_entries(&path)
                } else {
                    1
                }
            })
            .sum()
    }

    fn select_before_command(
        request: &Request,
        catalog: &universe::Catalog,
        manifest: &str,
    ) -> (Result<Placement>, usize) {
        let commands = std::cell::Cell::new(0_usize);
        let selected = select_placement(request, catalog, manifest).inspect(|_| {
            commands.set(commands.get().saturating_add(1));
        });
        (selected, commands.get())
    }

    #[test]
    fn user_profile_flows_into_runner_manifest_and_key() {
        let limits = Limits::standard();
        let project = Project::new("user-profile");
        let render = |profile: &str| {
            let authority = project.0.join("Cargo.toml");
            fs::write(
                &authority,
                format!("[workspace]\nresolver = \"3\"\n{profile}"),
            )
            .expect("authority manifest writes");
            let catalog = fs::canonicalize(
                PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/catalog-api"),
            )
            .expect("catalog fixture canonicalizes");
            let request = Request::builder(&project.0, &project.cache(), &limits)
                .expect("request builder seals")
                .environment(config::Environment::capture().expect("environment captures"))
                .policy(crate::extract::Policy::manifest().seal())
                .dependency(
                    config::Dependency::path("catalog", &catalog)
                        .and_then(|dependency| dependency.package("catalog-api"))
                        .expect("the catalog dependency seals"),
                )
                .cargo_authority(&authority)
                .build()
                .expect("request seals");
            let provisional = render_manifest(&request, None).expect("provisional");
            let key = versioned_request_identity(&request, &definition(), &provisional);
            let manifest = render_manifest(&request, Some(&internal::Recipe::fixture()))
                .expect("manifest renders");
            (
                manifest.parse::<DocumentMut>().expect("manifest parses"),
                key,
            )
        };
        let (plain, plain_key) = render("");
        // Rawr's own crate is a build dependency, which the harness never
        // builds (D609.2); the project's crates are its dependencies.
        let table = |name: &str| plain.get(name).and_then(Item::as_table);
        assert!(table(BUILD_DEPENDENCIES)
            .is_some_and(|table| table.contains_key(config::INTERNAL_ALIAS)));
        assert!(table("dependencies").is_some_and(|table| {
            table.contains_key("catalog") && !table.contains_key(config::INTERNAL_ALIAS)
        }));
        let packages = plain["profile"]["dev"]["package"]
            .as_table()
            .expect("rawr override table");
        assert_eq!(
            packages.iter().map(|(name, _)| name).collect::<Vec<_>>(),
            ["path+file:///exact/rawr#rawr@1.23.6"]
        );
        let (tuned, tuned_key) = render("[profile.dev.package.\"*\"]\nopt-level = 3\n");
        assert_eq!(
            tuned["profile"]["dev"]["package"]["*"]["opt-level"].as_integer(),
            Some(3)
        );
        assert_ne!(plain_key, tuned_key);
        assert_eq!(render("").1, plain_key);
    }

    #[test]
    fn fresh_authenticated_current_and_changed_request_choose_distinct_states() {
        let limits = Limits::standard();
        let project = Project::new("placement");
        let current = request(&project, false, &limits);
        let universe = universe::Catalog::new(definition());
        let manifest = render_manifest(&current, None).expect("manifest renders");
        let fresh =
            select_placement(&current, &universe, &manifest).expect("fresh placement selects");
        assert!(matches!(fresh.state, PlacementState::Fresh));
        assert!(fresh.exclusive);
        assert_eq!(fresh.root, project.cache().join(HARNESS_DIRECTORY));

        install_legacy(&fresh.root, &manifest, SEED);
        install_current_receipt(&fresh.root, &current, universe.current(), &manifest);
        let persisted = select_placement(&current, &universe, &manifest)
            .expect("authenticated persisted placement selects");
        assert!(matches!(persisted.state, PlacementState::Persisted { .. }));
        assert!(!persisted.exclusive);

        let changed = request(&project, true, &limits);
        let changed_manifest = render_manifest(&changed, None).expect("changed manifest renders");
        let before = filesystem_entries(&project.0);
        let (migration, commands) = select_before_command(&changed, &universe, &changed_manifest);
        let migration = migration.expect("changed request selects side-by-side candidate");
        assert_eq!(
            commands, 1,
            "authenticated current state reaches the next stage"
        );
        assert_eq!(filesystem_entries(&project.0), before);
        assert!(matches!(migration.state, PlacementState::Fresh));
        assert!(migration.exclusive);
        assert_eq!(
            migration.root.parent().and_then(Path::file_name),
            Some(std::ffi::OsStr::new(CANDIDATE_DIRECTORY))
        );
        assert_eq!(
            fs::read(fresh.root.join("Cargo.lock")).expect("last-good lock remains"),
            SEED.as_bytes()
        );
        assert!(!migration.root.exists());
    }

    /// The temporaries sweep over a Rawr-owned harness directory is bounded by
    /// the fixed `HARNESS_ENTRIES` ceiling (the 4,096 figure it always had),
    /// never by the configurable `packages` target: exactly the ceiling sweeps,
    /// one entry more refuses physically.
    #[test]
    fn stale_temporaries_sweep_is_bounded_by_the_fixed_harness_entry_ceiling() {
        assert_eq!(HARNESS_ENTRIES, 4_096);
        assert_eq!(HARNESS_ENTRIES, Limits::standard().packages);
        let project = Project::new("harness-entries");
        let root = project.cache().join("sweep");
        fs::create_dir_all(&root).expect("sweep root");
        for index in 0..HARNESS_ENTRIES {
            fs::write(root.join(format!("entry-{index}")), b"").expect("entry");
        }
        retire_stale_temporaries(&root).expect("exactly the ceiling sweeps");
        fs::write(root.join("one-more"), b"").expect("entry over the ceiling");
        assert!(matches!(
            retire_stale_temporaries(&root),
            Err(Error::Limit {
                resource: "resolution harness entries",
                limit: HARNESS_ENTRIES,
                actual,
            }) if actual == HARNESS_ENTRIES + 1
        ));
    }

    /// The side-by-side candidate a clean miss selects: fresh, exclusive,
    /// not yet created, and the stale namespace left as it was.
    fn assert_clean_miss(selected: Result<Placement>, fixed: &Path, lock: &str) {
        let selected = selected.expect("a stale namespace is a clean miss");
        assert!(matches!(selected.state, PlacementState::Fresh));
        assert!(selected.exclusive);
        assert_eq!(
            selected.root.parent().and_then(Path::file_name),
            Some(std::ffi::OsStr::new(CANDIDATE_DIRECTORY))
        );
        assert!(!selected.root.exists());
        assert_eq!(
            fs::read(fixed.join("Cargo.lock")).expect("the stale lock remains"),
            lock.as_bytes()
        );
    }

    #[test]
    fn a_namespace_another_version_wrote_is_a_clean_miss() {
        let limits = Limits::standard();
        for (label, lock, skew_source) in [
            ("skew", SEED, true),
            ("unknown", UNKNOWN_SEED, false),
            ("mixed", MIXED_SEED, false),
            ("receipt-absent", SEED, false),
        ] {
            let project = Project::new(label);
            let request = request(&project, false, &limits);
            let catalog = universe::Catalog::new(definition());
            let manifest = render_manifest(&request, None).expect("manifest renders");
            let fixed = project.cache().join(HARNESS_DIRECTORY);
            install_legacy(&fixed, &manifest, lock);
            if skew_source {
                fs::write(fixed.join("src/lib.rs"), b"skew").expect("source skews");
            }
            let before = filesystem_entries(&project.0);
            let (selected, commands) = select_before_command(&request, &catalog, &manifest);
            assert_eq!(commands, 1, "{label}");
            assert_eq!(filesystem_entries(&project.0), before, "{label}");
            assert_clean_miss(selected, &fixed, lock);
        }
    }

    /// #521: Cargo accepts `version` beside `git` and checks it against the
    /// Git package, so the rendered manifest carries it as written.
    #[test]
    fn a_git_dependency_renders_its_version_for_cargo_to_check() {
        let limits = Limits::standard();
        let project = Project::new("git-version");
        let dependency = config::Dependency::git(
            "catalog",
            "https://example.invalid/catalog",
            config::GitReference::Tag("v1.2.0".into()),
        )
        .map(|dependency| dependency.git_version("1.2"))
        .and_then(|dependency| dependency.package("catalog-api"))
        .expect("Cargo accepts `version` beside `git`");
        let request = Request::builder(&project.0, &project.cache(), &limits)
            .expect("request builder seals")
            .environment(config::Environment::capture().expect("environment captures"))
            .policy(crate::extract::Policy::manifest().seal())
            .dependency(dependency)
            .build()
            .expect("request seals");
        let manifest = render_manifest(&request, None).expect("manifest renders");
        let document = manifest
            .parse::<toml_edit::DocumentMut>()
            .expect("manifest parses");
        let catalog = document["dependencies"]["catalog"]
            .as_inline_table()
            .expect("the dependency is an inline table");
        assert_eq!(
            catalog.get("git").and_then(|value| value.as_str()),
            Some("https://example.invalid/catalog")
        );
        assert_eq!(
            catalog.get("tag").and_then(|value| value.as_str()),
            Some("v1.2.0")
        );
        assert_eq!(
            catalog.get("version").and_then(|value| value.as_str()),
            Some("1.2")
        );
    }

    #[test]
    fn current_receipt_lock_and_universe_skew_are_clean_misses() {
        let limits = Limits::standard();
        let project = Project::new("receipt-skew");
        let request = request(&project, false, &limits);
        let catalog = universe::Catalog::new(definition());
        let manifest = render_manifest(&request, None).expect("manifest renders");
        let fixed = project.cache().join(HARNESS_DIRECTORY);
        install_legacy(&fixed, &manifest, SEED);
        for universe in [[0x91; 32], *catalog.current().identity()] {
            write_fixture_receipt(
                &fixed,
                &request,
                catalog.current(),
                &manifest,
                universe,
                [0x92; 32],
            );
            let (selected, _) = select_before_command(&request, &catalog, &manifest);
            assert_clean_miss(selected, &fixed, SEED);
        }
    }

    #[test]
    fn every_final_cargo_operation_has_locked_authority() {
        let manifest = Path::new("/fixture/Cargo.toml");
        let candidate = cargo_metadata_arguments(1, manifest, "target", false);
        let final_metadata = cargo_metadata_arguments(1, manifest, "target", true);
        let unit = cargo_unit_arguments(manifest, "target", "dev");
        let build = cargo_build_arguments(manifest, "target", "dev");

        assert!(!candidate.iter().any(|argument| argument == "--locked"));
        for arguments in [final_metadata, unit, build] {
            assert_eq!(arguments.last(), Some(&OsString::from("--locked")));
        }
    }

    #[test]
    fn final_command_identity_binds_sealed_subcommands_jobs_and_injected_arguments() {
        let one = process::parallelism::Proof::fixture(
            std::num::NonZeroUsize::new(1).expect("fixture jobs are nonzero"),
        )
        .expect("one job seals");
        let many = process::parallelism::Proof::fixture(
            std::num::NonZeroUsize::new(12).expect("fixture jobs are nonzero"),
        )
        .expect("twelve jobs seal");
        let baseline = final_command_cohort(1, "aarch64-apple-darwin", "dev", many, &[])
            .expect("baseline command seals");
        let job_drift = final_command_cohort(1, "aarch64-apple-darwin", "dev", one, &[])
            .expect("job drift command seals");
        let config_drift = final_command_cohort(
            1,
            "aarch64-apple-darwin",
            "dev",
            many,
            &[PathBuf::from("/fixture/config.toml")],
        )
        .expect("config argument drift command seals");
        assert_ne!(baseline, job_drift);
        assert_ne!(baseline, config_drift);
    }
}

#[cfg(all(test, feature = "rustc-driver"))]
mod authority_boundary_tests {
    use std::fs;

    #[test]
    fn standard_request_authority_is_exact() {
        let standard = super::Limits::standard();
        assert_eq!(standard.items, super::ITEMS);
        assert_eq!(standard.items, 2_500_000);
        assert_eq!(standard.canonical_items, super::CANONICAL_ITEMS);
        assert_eq!(standard.canonical_items, 19_000_000);
        assert_eq!(standard.canonical_bytes, super::CANONICAL_BYTES);
        assert_eq!(standard.canonical_bytes, 1_207_959_552);
        assert_eq!(standard.input_proof_bytes, super::INPUT_PROOF_BYTES);
        assert_eq!(standard.input_proof_bytes, 536_870_912);
        assert_eq!(standard.retained_graph_items, super::RETAINED_GRAPH_ITEMS);
        assert_eq!(standard.retained_graph_items, 6_500_000);
        assert_eq!(standard.retained_graph_bytes, super::RETAINED_GRAPH_BYTES);
        assert_eq!(standard.retained_graph_bytes, 536_870_912);
        assert_eq!(
            standard.classified_contract_items,
            super::CLASSIFIED_CONTRACT_ITEMS
        );
        assert_eq!(standard.classified_contract_items, 6_500_000);
        assert_eq!(
            standard.classified_contract_bytes,
            super::CLASSIFIED_CONTRACT_BYTES
        );
        assert_eq!(standard.classified_contract_bytes, 671_088_640);
        // The JSON figure and the physical capture bounds are separate
        // numbers from separate modules.
        assert_eq!(standard.json_bytes, super::JSON_BYTES);
        assert_eq!(standard.json_bytes, 33_554_432);
        assert_eq!(standard.process, crate::process::Limits::capture());
        assert_eq!(standard.process.stdout(), crate::process::CAPTURE_BYTES);
        assert_eq!(standard.process.stdout(), 268_435_456);
        assert_eq!(standard.process.stderr(), crate::process::TAIL_BYTES);
        assert_eq!(standard.process.stderr(), 1_048_576);
        assert_eq!(standard.containers, 1_500_000);
    }

    /// Rawr's own patch for a package its facade closure reaches is
    /// added to the harness root; an unrelated Rawr patch is not, and the
    /// project's own patch of the same package wins.
    #[test]
    fn rawr_patches_for_the_facade_closure_reach_the_harness_root() {
        let rawr = "[patch.crates-io]\ncosmic-text = { git = \"https://example.invalid/cosmic-text\", rev = \"aa\" }\nrustyline = { git = \"https://example.invalid/rustyline\", branch = \"b\" }\n"
            .parse::<toml_edit::DocumentMut>()
            .expect("Rawr patch fixture parses");
        let rawr = rawr.get("patch").expect("Rawr patch table");
        let needed = |name: &str| name == "cosmic-text";

        let mut bare = super::Authority::load(None).expect("generated authority");
        bare.inherit_patches(rawr, needed).expect("patches merge");
        let patch = bare.retained.get("patch").expect("patch table is added");
        let crates = patch["crates-io"].as_table_like().expect("registry table");
        assert_eq!(
            crates.iter().map(|(name, _)| name).collect::<Vec<_>>(),
            ["cosmic-text"]
        );
        assert_eq!(
            crates
                .get("cosmic-text")
                .and_then(|item| item.get("rev"))
                .and_then(|item| item.as_str()),
            Some("aa")
        );

        let mut own = super::Authority::load(None).expect("generated authority");
        own.retained.insert(
            "patch".into(),
            "[patch.crates-io]\ncosmic-text = { path = \"/own\" }\n"
                .parse::<toml_edit::DocumentMut>()
                .expect("project patch parses")["patch"]
                .clone(),
        );
        own.inherit_patches(rawr, needed).expect("patches merge");
        let own = own.retained["patch"]["crates-io"]["cosmic-text"]
            .get("path")
            .and_then(|item| item.as_str());
        assert_eq!(own, Some("/own"));
    }

    #[test]
    fn maximal_authority_evidence_is_owned_once() {
        let root = std::env::temp_dir().join(format!(
            "rawr-authority-ledger-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        fs::create_dir(&root).expect("authority fixture root is created");
        let vendor = root.join("vendor/example");
        fs::create_dir_all(&vendor).expect("authority patch directory is created");
        let manifest = root.join("Cargo.toml");
        fs::write(
            &manifest,
            "[workspace]\nresolver = \"3\"\n[profile.dev]\nopt-level = 1\n[patch.crates-io]\nexample = { path = \"vendor/example\" }\n",
        )
        .expect("authority manifest is written");
        fs::write(root.join("Cargo.lock"), b"version = 4\n").expect("authority lock is written");
        let manifest = fs::canonicalize(manifest).expect("authority manifest canonicalizes");
        super::OWNED_AUTHORITIES.with(|owned| owned.set(0));
        super::Authority::load(Some(&manifest)).expect("the authority loads");
        assert_eq!(super::OWNED_AUTHORITIES.with(std::cell::Cell::get), 1);
        fs::remove_dir_all(root).expect("authority fixture is removed");
    }
}

#[cfg(all(test, feature = "rustc-driver"))]
mod epoch_tests {
    use super::*;
    use std::process::Command;
    use std::sync::{Arc, Barrier};

    const ROOT_ENV: &str = "RAWR_IMPORT_EPOCH_TEST_ROOT";

    fn fixture(name: &str) -> crate::store::TestProject {
        crate::store::TestProject::new(&format!("rawr-epoch-{name}"))
    }

    fn helper(name: &str, root: &Path) -> std::process::Child {
        Command::new(std::env::current_exe().expect("test executable is available"))
            .args(["--exact", name, "--ignored", "--nocapture"])
            .env(ROOT_ENV, root)
            .spawn()
            .expect("competing process can spawn")
    }

    #[test]
    #[ignore = "subprocess entry point"]
    fn production_lease_contender_process_helper() {
        let Some(root) = std::env::var_os(ROOT_ENV).map(PathBuf::from) else {
            return;
        };
        let cache = root.join("cache");
        let mut lease = open_lease_at(&root, &cache).expect("contender opens the cache lease");
        let error = acquire_lease(&mut lease, &cache.join(LEASE_FILE), "test lease")
            .expect_err("the production epoch excludes a competing process");
        assert!(matches!(error, Error::Owner));
    }

    #[test]
    fn production_epoch_holds_process_and_file_ownership_across_a_barrier() {
        let project = fixture("production-barrier");
        let root = project.path();
        let cache = root.join("cache");
        fs::create_dir(&cache).expect("cache fixture can be created");
        let _process_owner = acquire_process_owner().expect("fixture owns the process epoch");
        let mut lease = open_lease_at(root, &cache).expect("fixture opens the cache lease");
        let _lease_owner = acquire_lease(&mut lease, &cache.join(LEASE_FILE), "test lease")
            .expect("fixture owns the cache epoch");
        let barrier = Arc::new(Barrier::new(2));
        std::thread::scope(|scope| {
            let contender_barrier = Arc::clone(&barrier);
            scope.spawn(move || {
                contender_barrier.wait();
                assert!(matches!(acquire_process_owner(), Err(Error::Owner)));
                contender_barrier.wait();
            });
            barrier.wait();
            let status = helper(
                "cargo::epoch_tests::production_lease_contender_process_helper",
                root,
            )
            .wait()
            .expect("lease contender can be reaped");
            assert!(status.success(), "lease contender observed the held epoch");
            barrier.wait();
        });
    }
}

/// The physical child capture bound refuses at the `run_cargo` boundary.
#[cfg(test)]
mod capture_tests {
    use std::fs;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use crate::process;
    use crate::process::test_launch::{Expected, FileLimits, Reply, Script, Step};

    struct Root(PathBuf);

    impl Root {
        fn new() -> Self {
            static NEXT: AtomicUsize = AtomicUsize::new(0);
            let root = std::env::temp_dir().join(format!(
                "rawr-cargo-capture-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir(&root).unwrap();
            Self(fs::canonicalize(root).unwrap())
        }

        fn script(&self, limits: &super::Limits, reply: Reply) -> Script {
            let process = limits.process_limits();
            let step = Step::new(
                move |_| Expected::new("/fixture/cargo".into(), command(), process),
                |_| reply,
            );
            Script::new(
                self.0.clone(),
                FileLimits {
                    count: 1,
                    bytes: 16,
                },
                vec![step],
            )
            .unwrap()
        }
    }

    impl Drop for Root {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn command() -> process::Command {
        process::Command::cargo(
            Vec::new(),
            process::CargoAction::Metadata,
            vec!["--offline".into(), "--format-version=1".into()],
            PathBuf::from("/fixture/project"),
            vec![("CARGO_HOME".into(), "/fixture/cargo-home".into())].into_boxed_slice(),
        )
        .unwrap()
    }

    #[test]
    fn standard_limits_separate_the_json_target_from_the_physical_capture() {
        let standard = super::Limits::standard();
        assert_eq!(standard.json_bytes, 33_554_432);
        assert_eq!(standard.process_limits().stdout(), 268_435_456);
        assert_eq!(standard.process_limits().stderr(), 1_048_576);
        assert!(standard.json_bytes <= standard.process_limits().stdout());
        // The process slot is filled by the physical constructor alone.
        assert_eq!(standard.process_limits(), process::Limits::capture());
        assert_eq!(
            process::Limits::discovery().stdout(),
            process::Limits::capture().stderr()
        );
    }

    /// A child emitting one byte past the physical capture bound is refused as
    /// `Error::Limit` at the capture itself, whatever the JSON target says.
    #[test]
    fn capture_plus_one_is_physically_refused() {
        let root = Root::new();
        let limits = super::Limits {
            json_bytes: process::CAPTURE_BYTES,
            ..super::Limits::standard()
        };
        let native = process::Native::fixture();
        let error = root
            .script(
                &limits,
                Reply::stdout(vec![b'0'; process::CAPTURE_BYTES + 1]),
            )
            .run(|| super::run_cargo(&native.production(), &command(), &limits))
            .unwrap()
            .expect_err("the physical capture bound refuses");
        assert!(matches!(
            error,
            crate::Error::Limit {
                resource: "child stdout",
                limit: process::CAPTURE_BYTES,
                actual,
            } if actual == process::CAPTURE_BYTES + 1
        ));
        // Exactly the bound is captured whole.
        let bytes = root
            .script(&limits, Reply::stdout(vec![b'0'; process::CAPTURE_BYTES]))
            .run(|| super::run_cargo(&native.production(), &command(), &limits))
            .unwrap()
            .unwrap();
        assert_eq!(bytes.len(), process::CAPTURE_BYTES);
    }
}
