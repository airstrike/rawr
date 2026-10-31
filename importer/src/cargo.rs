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
/// The Cargo JSON accounting target: the figure a Cargo document is
/// measured against and warned past. It is distinct from the physical child
/// capture bound `process::CAPTURE_BYTES`, which refuses; a target above that
/// bound is unreachable and is refused at load.
const JSON_BYTES: usize = 32 * 1024 * 1024;
const _: () = assert!(JSON_BYTES <= process::CAPTURE_BYTES);
const MANIFEST_BYTES: usize = 1024 * 1024;
const PACKAGES: usize = 4_096;
/// Physical bound on the directory entries swept when retiring stale
/// temporaries from a Rawr-owned resolution harness. Fixed at the 4,096 figure
/// (the standard package envelope) this sweep has always refused at; it is a
/// finite-work ceiling over a directory Rawr owns, not an input measure, so it
/// never follows the configurable `packages` accounting target (a configured
/// count must not move a terminal bound).
const HARNESS_ENTRIES: usize = 4_096;
const UNITS: usize = 500_000;
const EDGES: usize = 1_000_000;
const ITEMS: usize = 2_500_000;
const CANONICAL_ITEMS: usize = 19_000_000;
/// The canonical bytes accounting target starts at the node content figure;
/// it is an alias of the physical `node::MAX_CONTENT_BYTES` value, not a second
/// spelling of it, and `node_canonical_bytes()` stays decoupled from the target.
const CANONICAL_BYTES: usize = crate::node::MAX_CONTENT_BYTES;
const INPUT_PROOF_BYTES: usize = 512 * 1024 * 1024;
/// Physical bound on one typed-store node's recipe authority, discovered
/// inputs and retained input evidence (the locked-metadata, unit-graph and
/// build node authorities, the resolution evidence capture and the cached
/// toolchain inputs). Fixed at 512 MiB, its effective value before the
/// accounting-target series (at c1fedce0^ it was read through the then-default
/// `input_proof_bytes`). It is not configurable and never follows the
/// `input_proof_bytes` accounting target, which only shares the figure.
const NODE_INPUT_BYTES: usize = 512 * 1024 * 1024;
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
    pub(crate) graph_lookups: usize,
    pub(crate) graph_comparisons: usize,
    pub(crate) graph_deep_validations: usize,
    pub(crate) graph_decode_bytes: u64,
    pub(crate) graph_path_checks: usize,
    pub(crate) graph_hashed_bytes: u64,
    process: process::Limits,
}

/// Why a configured accounting target was refused at load.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ConfigureError {
    /// The key names no configurable accounting target; physical limits are
    /// never configurable.
    UnknownTarget(Box<str>),
    /// A target must be a positive integer.
    Zero(&'static str),
    /// The value does not fit the account's counter.
    Unrepresentable(&'static str, u64),
    /// A configured target may never raise a physical limit it is compared against.
    AbovePhysicalCeiling {
        field: &'static str,
        value: u64,
        ceiling: u64,
        physical: &'static str,
    },
}

impl fmt::Display for ConfigureError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownTarget(key) => write!(
                formatter,
                "`rust.limits.{key}` is not a configurable accounting target; physical limits \
                 are not configurable"
            ),
            Self::Zero(field) => {
                write!(
                    formatter,
                    "`rust.limits.{field}` must be a positive integer"
                )
            }
            Self::Unrepresentable(field, value) => write!(
                formatter,
                "`rust.limits.{field}` = {value} does not fit this account's counter"
            ),
            Self::AbovePhysicalCeiling {
                field,
                value,
                ceiling,
                physical,
            } => write!(
                formatter,
                "`rust.limits.{field}` = {value} exceeds the physical {physical} ceiling \
                 {ceiling}; a configured accounting target may never raise a physical limit"
            ),
        }
    }
}

impl std::error::Error for ConfigureError {}

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

/// Staged inputs that can only become a `Request` after canonical sealing.
pub struct Builder {
    inventory: bool,
    limits: Limits,
    allocation: RequestAllocation,
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
    allocation: RequestAllocation,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct RequestAllocation {
    items: usize,
    bytes: usize,
    item_limit: usize,
    byte_limit: usize,
    string_limit: usize,
    /// Process-unique account identity for exceedance reporting.
    id: u64,
    exceeded: crate::exceedance::Record,
}

pub(crate) const REQUEST_ITEMS_RESOURCE: &str = "request retained items";
pub(crate) const REQUEST_BYTES_RESOURCE: &str = "request retained bytes";
pub(crate) const REQUEST_STRING_RESOURCE: &str = "request retained string bytes";

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

    /// Whether every selected crate replayed shared bindings, so no nightly
    /// build or probe ran.
    pub fn is_shared(&self) -> bool {
        matches!(self.owner, resolution::owner::Resolved::Shared(_))
    }

    /// Publish this currently validated owner through the extraction API node.
    pub fn publish(self, limits: &Limits) -> Result<crate::api::Graph<crate::api::Extracted>> {
        match self.owner {
            resolution::owner::Resolved::Owner(owner) => owner.publish(limits),
            resolution::owner::Resolved::Shared(_) => Err(Error::Request {
                field: "API publication",
                reason: "shared bindings publish through the retained API".into(),
            }),
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
}

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
}

impl CompletedHarness {
    fn into_resolution(self, request: Request) -> Resolution {
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
    // Legacy callers carry their live request account through preparation and
    // completion. Reserved callers already borrow that account from the owner.
    request_allocation: Option<RequestAllocation>,
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
}

/// A prepared harness whose ordinary Build and both postchecks succeeded.
#[cfg(feature = "rustc-driver")]
struct Built<'allocation> {
    harness: Harness,
    build: crate::input::build::Build,
    allocation: &'allocation mut RequestAllocation,
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
    fn bind(
        metadata: unit::Transport,
        units: unit::Transport,
        parsed: &unit::Metadata,
        graph: &unit::BuildGraph,
    ) -> Result<Self> {
        parsed.require_exact_raw(metadata.bytes(), "retained Cargo observations")?;
        graph.require_exact_raw(units.bytes(), "retained Cargo observations")?;
        Ok(Self { metadata, units })
    }
}

/// One complete cold observation bundle; none of its fields is a current receipt.
struct Retained {
    documents: Documents,
    build: crate::input::build::Retained,
}

enum Postcheck {
    Units,
    Metadata,
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
    pub(crate) post_unit: crate::node::Validated<crate::cargo::resolution::UnitGraph>,
    pub(crate) post_metadata: crate::node::Validated<crate::cargo::resolution::Locked>,
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
            graph_lookups: crate::node::GraphTargets::STANDARD.lookups,
            graph_comparisons: crate::node::GraphTargets::STANDARD.comparisons,
            graph_deep_validations: crate::node::GraphTargets::STANDARD.deep,
            graph_decode_bytes: crate::node::GraphTargets::STANDARD.decoded,
            graph_path_checks: crate::node::GraphTargets::STANDARD.paths,
            graph_hashed_bytes: crate::node::GraphTargets::STANDARD.hashed,
            process: process::Limits::capture(),
        }
    }

    /// The configurable accounting-target keys of a `[rust.limits]` table, in
    /// kebab-case spelling. Physical limits (child capture, store payload
    /// ceilings, recursion depth, RSS, disk, process and IPC bounds) are absent.
    pub const TARGET_FIELDS: &'static [&'static str] = &[
        "json-bytes",
        "manifest-bytes",
        "request-bytes",
        "packages",
        "units",
        "edges",
        "items",
        "canonical-items",
        "canonical-bytes",
        "input-proof-bytes",
        "retained-graph-items",
        "retained-graph-bytes",
        "classified-contract-items",
        "classified-contract-bytes",
        "containers",
        "string-bytes",
        "graph-lookups",
        "graph-comparisons",
        "graph-deep-validations",
        "graph-decode-bytes",
        "graph-path-checks",
        "graph-hashed-bytes",
    ];

    /// Override one accounting target by its `[rust.limits]` key.
    ///
    /// Fails closed: unknown keys, zero, unrepresentable values and any target
    /// above the physical ceiling it is measured under are refused at load.
    /// `depth` and every physical limit are not configurable. Ceilings:
    /// `json-bytes` ≤ `process::CAPTURE_BYTES` (child stdout capture);
    /// `manifest-bytes`/`request-bytes` ≤ `process::TAIL_BYTES` (every
    /// manifest-class read — manifests, harness sources, authority inputs,
    /// Cargo config — binds there); the byte targets measured under node
    /// store content (`canonical-bytes`, `input-proof-bytes`,
    /// `retained-graph-bytes`, `classified-contract-bytes`, `string-bytes`)
    /// ≤ `node::MAX_CONTENT_BYTES`; counts and the cumulative graph byte
    /// targets have no single physical ceiling and take any non-zero value.
    pub fn configure(&mut self, key: &str, value: u64) -> std::result::Result<(), ConfigureError> {
        fn count(field: &'static str, value: u64) -> std::result::Result<usize, ConfigureError> {
            if value == 0 {
                return Err(ConfigureError::Zero(field));
            }
            usize::try_from(value).map_err(|_| ConfigureError::Unrepresentable(field, value))
        }
        fn bytes(
            field: &'static str,
            value: u64,
            ceiling: usize,
            physical: &'static str,
        ) -> std::result::Result<usize, ConfigureError> {
            let value = count(field, value)?;
            if value > ceiling {
                return Err(ConfigureError::AbovePhysicalCeiling {
                    field,
                    value: value as u64,
                    ceiling: ceiling as u64,
                    physical,
                });
            }
            Ok(value)
        }
        const CONTENT: &str = "node store payload";
        const CAPTURE: &str = "child stdout capture";
        const TAIL: &str = "manifest-class read";
        let content = crate::node::MAX_CONTENT_BYTES;
        match key {
            "json-bytes" => {
                self.json_bytes = bytes("json-bytes", value, process::CAPTURE_BYTES, CAPTURE)?;
            }
            "manifest-bytes" | "request-bytes" => {
                self.manifest_bytes = bytes("manifest-bytes", value, process::TAIL_BYTES, TAIL)?;
            }
            "packages" => self.packages = count("packages", value)?,
            "units" => self.units = count("units", value)?,
            "edges" => self.edges = count("edges", value)?,
            "items" => self.items = count("items", value)?,
            "canonical-items" => self.canonical_items = count("canonical-items", value)?,
            "canonical-bytes" => {
                self.canonical_bytes = bytes("canonical-bytes", value, content, CONTENT)?;
            }
            "input-proof-bytes" => {
                self.input_proof_bytes = bytes("input-proof-bytes", value, content, CONTENT)?;
            }
            "retained-graph-items" => {
                self.retained_graph_items = count("retained-graph-items", value)?;
            }
            "retained-graph-bytes" => {
                self.retained_graph_bytes = bytes("retained-graph-bytes", value, content, CONTENT)?;
            }
            "classified-contract-items" => {
                self.classified_contract_items = count("classified-contract-items", value)?;
            }
            "classified-contract-bytes" => {
                self.classified_contract_bytes =
                    bytes("classified-contract-bytes", value, content, CONTENT)?;
            }
            "containers" => self.containers = count("containers", value)?,
            "string-bytes" => {
                self.string_bytes = bytes("string-bytes", value, content, CONTENT)?;
            }
            "graph-lookups" => self.graph_lookups = count("graph-lookups", value)?,
            "graph-comparisons" => self.graph_comparisons = count("graph-comparisons", value)?,
            "graph-deep-validations" => {
                self.graph_deep_validations = count("graph-deep-validations", value)?;
            }
            "graph-decode-bytes" => {
                count("graph-decode-bytes", value.min(usize::MAX as u64))?;
                self.graph_decode_bytes = value;
            }
            "graph-path-checks" => self.graph_path_checks = count("graph-path-checks", value)?,
            "graph-hashed-bytes" => {
                count("graph-hashed-bytes", value.min(usize::MAX as u64))?;
                self.graph_hashed_bytes = value;
            }
            other => return Err(ConfigureError::UnknownTarget(other.into())),
        }
        Ok(())
    }

    /// Whether any accounting target differs from `Limits::standard()`.
    #[must_use]
    pub fn is_configured(&self) -> bool {
        *self != Self::standard()
    }

    pub(crate) const fn graph_targets(&self) -> crate::node::GraphTargets {
        crate::node::GraphTargets {
            lookups: self.graph_lookups,
            comparisons: self.graph_comparisons,
            deep: self.graph_deep_validations,
            decoded: self.graph_decode_bytes,
            paths: self.graph_path_checks,
            hashed: self.graph_hashed_bytes,
        }
    }

    /// Derive explicit Phase 2 cumulative bounds for driver integration audits.
    ///
    /// This read-only driver boundary does not relax any Cargo, process,
    /// package, graph-depth, or manifest limit. Like `configure`, a JSON target
    /// above the physical child capture bound is refused.
    #[cfg(feature = "rustc-driver")]
    #[doc(hidden)]
    pub const fn with_p2_bounds(
        mut self,
        json_bytes: usize,
        items: usize,
        string_bytes: usize,
    ) -> std::result::Result<Self, ConfigureError> {
        if json_bytes > process::CAPTURE_BYTES {
            return Err(ConfigureError::AbovePhysicalCeiling {
                field: "json-bytes",
                value: json_bytes as u64,
                ceiling: process::CAPTURE_BYTES as u64,
                physical: "child stdout capture",
            });
        }
        self.json_bytes = json_bytes;
        self.items = items;
        self.classified_contract_items = items;
        self.classified_contract_bytes = json_bytes;
        self.string_bytes = string_bytes;
        Ok(self)
    }

    pub(crate) const fn process_limits(self) -> process::Limits {
        self.process
    }

    /// The node content bound for every typed-store product recipe. This is
    /// the physical `MAX_CONTENT_BYTES` ceiling, deliberately decoupled
    /// from the configurable `json_bytes` accounting target so a configured
    /// target can never size or move the store payload bound.
    pub(crate) const fn node_output_bytes(self) -> usize {
        crate::node::MAX_CONTENT_BYTES
    }

    /// The node content bound for recipe authorities, discovered inputs and
    /// retained input evidence of typed-store nodes. This is the physical
    /// `NODE_INPUT_BYTES` ceiling (512 MiB, its pre-series value), deliberately
    /// decoupled from the configurable `input_proof_bytes` accounting target so
    /// a configured target can never size or move a node content bound.
    /// Every file read feeding these nodes binds separately to
    /// `process::TAIL_BYTES` or `process::CAPTURE_BYTES`.
    pub(crate) const fn node_input_bytes(self) -> usize {
        NODE_INPUT_BYTES
    }

    pub(crate) const fn node_items(self) -> usize {
        self.items
    }

    pub(crate) const fn node_string_bytes(self) -> usize {
        self.string_bytes
    }

    /// The node store payload ceiling for stored API graphs. This is the
    /// physical `MAX_CONTENT_BYTES` bound, deliberately decoupled from
    /// the configurable `canonical_bytes` accounting target so a configured
    /// target can never raise the store ceiling.
    pub(crate) const fn node_canonical_bytes(self) -> usize {
        crate::node::MAX_CONTENT_BYTES
    }

    pub(crate) fn bind_node_context(&self, digest: &mut Sha256) {
        // Configured accounting targets are explicit proof inputs; v2
        // binds the per-graph targets beside every other limit.
        digest.update(b"rawr.import.cargo.limits/v2");
        for value in [
            self.json_bytes as u64,
            self.manifest_bytes as u64,
            self.packages as u64,
            self.units as u64,
            self.edges as u64,
            self.items as u64,
            self.canonical_items as u64,
            self.canonical_bytes as u64,
            self.input_proof_bytes as u64,
            self.retained_graph_items as u64,
            self.retained_graph_bytes as u64,
            self.classified_contract_items as u64,
            self.classified_contract_bytes as u64,
            self.containers as u64,
            self.string_bytes as u64,
            self.depth as u64,
            self.graph_lookups as u64,
            self.graph_comparisons as u64,
            self.graph_deep_validations as u64,
            self.graph_decode_bytes,
            self.graph_path_checks as u64,
            self.graph_hashed_bytes,
            self.process.stdout() as u64,
            self.process.stderr() as u64,
        ] {
            digest.update(value.to_le_bytes());
        }
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

#[cfg(test)]
thread_local! {
    /// Highest cumulative request retained bytes admitted on this test thread.
    pub(crate) static REQUEST_HIGH_WATER: std::cell::Cell<usize> =
        const { std::cell::Cell::new(0) };
}

impl RequestAllocation {
    fn high_water(&self) {
        #[cfg(test)]
        REQUEST_HIGH_WATER.with(|high| high.set(high.get().max(self.bytes)));
    }

    /// Publish this account's exceedance record.
    fn publish(&self) {
        crate::exceedance::publish(crate::exceedance::Account::Request, self.id, &self.exceeded);
    }

    fn new(limits: &Limits) -> Self {
        Self {
            items: 0,
            bytes: 0,
            item_limit: limits.items,
            byte_limit: limits.manifest_bytes,
            string_limit: limits.string_bytes,
            id: crate::exceedance::next_id(),
            exceeded: crate::exceedance::Record::new(),
        }
    }

    /// The axes of this request account that crossed their targets so far.
    #[cfg(test)]
    pub(crate) const fn exceedances(&self) -> crate::exceedance::Record {
        self.exceeded
    }

    /// Process-unique identity of this request account.
    #[cfg(test)]
    pub(crate) const fn id(&self) -> u64 {
        self.id
    }

    /// Checked-add overflow is an arithmetic failure, never an accounting
    /// exceedance: it stays terminal.
    fn overflow(operation: &'static str) -> Error {
        Error::Node {
            operation,
            source: Box::new(crate::node::Error::Allocation),
        }
    }

    /// Crossing an accounting target is recorded and published, never
    /// refused. The charge has already been added to the original account.
    pub(crate) fn exceeded(&mut self, resource: &'static str, target: usize, actual: usize) {
        if actual > target
            && self
                .exceeded
                .exceeded(resource, target as u64, actual as u64, None)
        {
            self.publish();
        }
    }

    pub(crate) fn items(&mut self, amount: usize) -> Result<()> {
        let actual = self
            .items
            .checked_add(amount)
            .ok_or_else(|| Self::overflow(REQUEST_ITEMS_RESOURCE))?;
        self.items = actual;
        self.exceeded(REQUEST_ITEMS_RESOURCE, self.item_limit, actual);
        Ok(())
    }

    pub(crate) fn bytes(&mut self, amount: usize) -> Result<()> {
        let actual = self
            .bytes
            .checked_add(amount)
            .ok_or_else(|| Self::overflow(REQUEST_BYTES_RESOURCE))?;
        self.bytes = actual;
        self.high_water();
        self.exceeded(REQUEST_BYTES_RESOURCE, self.byte_limit, actual);
        Ok(())
    }

    pub(crate) fn retained(&mut self, items: usize, bytes: usize) -> Result<()> {
        let retained_items = self
            .items
            .checked_add(items)
            .ok_or_else(|| Self::overflow(REQUEST_ITEMS_RESOURCE))?;
        let retained_bytes = self
            .bytes
            .checked_add(bytes)
            .ok_or_else(|| Self::overflow(REQUEST_BYTES_RESOURCE))?;
        self.items = retained_items;
        self.bytes = retained_bytes;
        self.high_water();
        self.exceeded(REQUEST_ITEMS_RESOURCE, self.item_limit, retained_items);
        self.exceeded(REQUEST_BYTES_RESOURCE, self.byte_limit, retained_bytes);
        Ok(())
    }

    pub(crate) fn string(&mut self, value: &str) -> Result<()> {
        self.exceeded(REQUEST_STRING_RESOURCE, self.string_limit, value.len());
        self.retained(1, value.len())
    }

    pub(crate) fn path(&mut self, value: &Path) -> Result<()> {
        self.retained(1, value.as_os_str().len())
    }

    pub(crate) fn formatted(&mut self, arguments: fmt::Arguments<'_>) -> Result<String> {
        struct Counter(usize);
        impl fmt::Write for Counter {
            fn write_str(&mut self, value: &str) -> fmt::Result {
                self.0 = self.0.checked_add(value.len()).ok_or(fmt::Error)?;
                Ok(())
            }
        }
        let retained = arguments;
        let mut counter = Counter(0);
        fmt::write(&mut counter, arguments).map_err(|_| Error::Node {
            operation: "request formatted length",
            source: Box::new(crate::node::Error::Allocation),
        })?;
        self.items(1)?;
        self.bytes(counter.0)?;
        let mut value = String::new();
        value
            .try_reserve_exact(counter.0)
            .map_err(|source| Error::Allocation {
                operation: "request formatted string",
                source,
            })?;
        fmt::write(&mut value, retained).map_err(|_| Error::Request {
            field: "request formatted allocation",
            reason: "formatted request rendering failed".into(),
        })?;
        Ok(value)
    }

    pub(crate) fn transfer(&mut self, retained: Self) -> Result<()> {
        if self.item_limit != retained.item_limit
            || self.byte_limit != retained.byte_limit
            || self.string_limit != retained.string_limit
        {
            return Err(Error::Request {
                field: "dependency allocation",
                reason: "was sealed under a different request limit profile".into(),
            });
        }
        let items = self
            .items
            .checked_add(retained.items)
            .ok_or_else(|| Self::overflow(REQUEST_ITEMS_RESOURCE))?;
        let bytes = self
            .bytes
            .checked_add(retained.bytes)
            .ok_or_else(|| Self::overflow(REQUEST_BYTES_RESOURCE))?;
        self.items = items;
        self.bytes = bytes;
        self.high_water();
        // The transferred account's own exceedances travel with its charges.
        if retained.exceeded.is_exceeded() {
            self.exceeded.absorb(&retained.exceeded);
            self.publish();
        }
        self.exceeded(REQUEST_ITEMS_RESOURCE, self.item_limit, items);
        self.exceeded(REQUEST_BYTES_RESOURCE, self.byte_limit, bytes);
        Ok(())
    }

    fn transfer_policy(&mut self, policy: &crate::extract::Policy) -> Result<()> {
        let (items, bytes) = policy.retained_usage();
        self.retained(items, bytes)
    }

    fn transfer_with_items(&mut self, retained: Self, amount: usize) -> Result<()> {
        let mut retained = retained;
        retained.items(amount)?;
        self.transfer(retained)
    }

    fn matches(self, limits: &Limits) -> bool {
        self.item_limit == limits.items
            && self.byte_limit == limits.manifest_bytes
            && self.string_limit == limits.string_bytes
    }
}

impl Request {
    /// Begin a request whose loose fields must still be promoted by `build`.
    pub fn builder(
        project_root: &Path,
        project_cache_root: &Path,
        limits: &Limits,
    ) -> Result<Builder> {
        let mut allocation = RequestAllocation::new(limits);
        allocation.items(1)?;
        allocation.path(project_root)?;
        allocation.path(project_cache_root)?;
        allocation.string(PROFILE_DEFAULT)?;
        Ok(Builder {
            inventory: false,
            limits: *limits,
            allocation,
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
        if self.inventory || !self.demand.is_empty() {
            return Err(Error::Request {
                field: "application native inputs",
                reason:
                    "native input capture cannot also request intrinsic or contextual API facts"
                        .into(),
            });
        }
        self.inputs_only = true;
        self.resolve(native, limits)
    }

    /// Every library crate this request's build reaches that it does not
    /// declare, as an exact dependency at its resolved source, version and
    /// features. An intrinsic inventory declares them too, so each crate
    /// that can implement a trait has its own complete partition (D650.5).
    /// This runs the normal resolution only: no metadata probe.
    #[cfg(feature = "rustc-driver")]
    pub fn closure_dependencies(
        mut self,
        native: &Native,
        limits: &Limits,
    ) -> Result<Vec<config::Dependency>> {
        self.inventory = false;
        self.demand = Box::default();
        self.inputs_only = true;
        let resolution = self.resolve(native, limits)?;
        let metadata = &resolution.metadata;
        let declared = metadata.declared_packages();
        let root = metadata.root_package_id();
        let mut seen = std::collections::BTreeSet::new();
        let mut dependencies = Vec::new();
        for output in resolution.build_unit.artifacts() {
            let library = output
                .target_kinds()
                .iter()
                .any(|kind| matches!(kind.as_ref(), "lib" | "rlib"));
            let proc_macro = output
                .target_kinds()
                .iter()
                .any(|kind| kind.as_ref() == "proc-macro");
            let package_id = output.package_id();
            if !library
                || proc_macro
                || Some(package_id) == root
                || declared.contains(package_id)
                || !seen.insert(package_id.to_owned())
            {
                continue;
            }
            let package = metadata.package(package_id).ok_or_else(|| Error::Request {
                field: "inventory closure",
                reason: format!("`{package_id}` has no resolved package").into_boxed_str(),
            })?;
            let alias = output.crate_name();
            let dependency = match package.source() {
                unit::PackageSource::Path { .. } => {
                    config::Dependency::path(alias, package.root(), limits)?
                }
                unit::PackageSource::Registry { cargo_id, .. }
                    if cargo_id.starts_with(
                        "registry+https://github.com/rust-lang/crates.io-index",
                    ) || cargo_id.starts_with("sparse+https://index.crates.io/") =>
                {
                    config::Dependency::registry(
                        alias,
                        &format!("={}", package.version()),
                        limits,
                    )?
                }
                unit::PackageSource::Git { cargo_id, oid } => {
                    let url = cargo_id
                        .strip_prefix("git+")
                        .and_then(|rest| rest.split(['?', '#']).next())
                        .ok_or_else(|| Error::Request {
                            field: "inventory closure",
                            reason: format!("`{package_id}` has no git URL").into_boxed_str(),
                        })?;
                    config::Dependency::git(alias, url, oid, limits)?
                }
                unit::PackageSource::Registry { .. } => {
                    return Err(Error::Request {
                        field: "inventory closure",
                        reason: format!(
                            "`{package_id}` comes from a registry other than crates.io, which an intrinsic inventory cannot declare yet"
                        )
                        .into_boxed_str(),
                    });
                }
            }
            .package(package.name())?
            .features(output.features().iter().map(AsRef::as_ref), false)?;
            dependencies.push(dependency);
        }
        Ok(dependencies)
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
        if self.inventory {
            return Err(Error::Request {
                field: "intrinsic inventory",
                reason: "use independent uncached acquisition; demand replay is not a completeness proof".into(),
            });
        }
        resolution::owner::prepare_cached_in(self, limits, scope, &crate::metrics::Current)
            .map(|owner| Prepared { owner })
            .map_err(|source| Error::Node {
                operation: "complete resolution owner",
                source: Box::new(source),
            })
    }

    /// Resolve through the shared toolchain node before any tool discovery or
    /// version child is launched. Evidence-only callers retain [`Self::resolve`].
    pub fn resolve_cached(mut self, limits: &Limits) -> Result<Resolution> {
        if self.inventory {
            return Err(Error::Request {
                field: "intrinsic inventory",
                reason: "use independent uncached acquisition; demand replay is not a completeness proof".into(),
            });
        }
        self.require_limits(limits)?;
        if self.universe.is_some() {
            let provisional_manifest = self.charge_provisional_manifest(limits)?;
            if let Some(catalog) = self.universe.as_ref() {
                let _placement = select_placement(&self, catalog, &provisional_manifest, limits)?;
            }
        }
        let context = crate::context::Context::capture_config(
            &self.project_root,
            &self.config,
            &self.target,
            limits,
        )
        .map_err(|error| Error::Request {
            field: "toolchain node context",
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
            field: "toolchain node context",
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

    /// Render the provisional manifest on the sealed request account
    /// and carry the charged account forward, totals included, so preparation
    /// resumes from them. The provisional string is dropped after placement,
    /// so preparation's later renders (even of identical bytes) are real work
    /// charged on top: nothing is lost, and each render is charged once.
    fn charge_provisional_manifest(&mut self, limits: &Limits) -> Result<String> {
        let mut allocation = self.allocation;
        let manifest = render_manifest_accounted(self, None, limits, &mut allocation);
        self.allocation = allocation;
        manifest
    }

    fn resolve_with(
        self,
        native: &Native,
        limits: &Limits,
        cached: Option<&CachedToolchain>,
    ) -> Result<Resolution> {
        self.require_limits(limits)?;
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
        self.require_limits(limits)?;
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
        // One working copy of the sealed request account serves every
        // render and charge of this preparation. It resumes from the sealed
        // totals, including any provisional render `resolve_cached` already
        // charged, and every render here (a re-render of the provisional
        // bytes included: that string was dropped, so this is new work) is
        // charged on top exactly once.
        let mut request_allocation = self.allocation;
        let node_context = Arc::new(match cached {
            Some(cached) => cached.context.clone(),
            None => crate::context::Context::capture_config(
                &self.project_root,
                &self.config,
                &self.target,
                limits,
            )
            .map_err(|error| Error::Request {
                field: "resolution node context",
                reason: error.to_string().into_boxed_str(),
            })?,
        });
        let versioned = self
            .universe
            .clone()
            .map(|catalog| {
                let provisional_manifest =
                    render_manifest_accounted(self, None, limits, &mut request_allocation)?;
                let placement = select_placement(self, &catalog, &provisional_manifest, limits)?;
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
                &mut request_allocation,
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
            digest(
                render_manifest_accounted(self, None, limits, &mut request_allocation)?.as_bytes(),
            )
            .0,
        )
        .run(|| {
            let harness_root = prepare_harness(self, limits, &mut request_allocation)?;
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
            let internal = self.internal.seal_accounted(
                &provisional_metadata,
                &provisional_selected,
                &mut request_allocation,
            )?;
            finalize_harness(
                self,
                &harness_root,
                &internal,
                limits,
                &mut request_allocation,
            )?;
            drop(provisional_selected);
            drop(provisional_metadata);
            // The post-build check observes metadata under the Build's target
            // directory, so the final metadata names that same directory.
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
            // constant, never by an accounting target. Manifest-class reads
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

            let workspace = self.capture_workspace(
                &metadata,
                &mut request_allocation,
                node_context.graph_budget(),
                limits,
            )?;

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
                request_allocation: Some(request_allocation),
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
        request_allocation: &mut RequestAllocation,
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
                let authority = locked_metadata_node_authority(
                    request,
                    &toolchain,
                    self,
                    &manifest_path,
                    limits,
                )?;
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
                let internal =
                    self.internal
                        .seal_accounted(&metadata, &selected, &mut *request_allocation)?;
                let manifest = render_manifest_accounted(
                    self,
                    Some(&internal),
                    limits,
                    &mut *request_allocation,
                )?;
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
                let authority = locked_metadata_node_authority(
                    request,
                    &toolchain,
                    self,
                    &manifest_path,
                    limits,
                )?;
                let mut materialized = None;
                let execute = || {
                    let provisional_root = prepare_versioned_roots(
                        self,
                        &harness_root,
                        exclusive,
                        limits,
                        &mut *request_allocation,
                    )?;
                    let provisional_path = provisional_root.join(HARNESS_MANIFEST);
                    let provisional_target = provisional_root.join("target");
                    let fresh = self.materialize_fresh(
                        limits,
                        &mut cargo_allocation,
                        &mut *request_allocation,
                        &definition,
                        (&manifest_path, &source_path, &lock_path),
                        |phase, _| match phase {
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
                            limits,
                        )?,
                    ),
                };
                let (metadata, selected, internal) = self.bind_fresh_metadata(
                    metadata,
                    internal,
                    &lock,
                    (&manifest_path, &source_path),
                    limits,
                    cargo_limits,
                    &mut cargo_allocation,
                    &mut *request_allocation,
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

        let workspace = self.capture_workspace(
            &metadata,
            &mut *request_allocation,
            node_context.graph_budget(),
            limits,
        )?;

        #[cfg(feature = "rustc-driver")]
        let initial_target = harness_root.join(AUTHORITATIVE_TARGET_DIRECTORY);
        #[cfg(not(feature = "rustc-driver"))]
        let initial_target = harness_root.join("target");
        let authority =
            unit_graph_node_authority(request, &toolchain, self, &manifest_path, limits)?;
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
            request_allocation: Some(*request_allocation),
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
        })
    }

    /// The lock a fresh resolution starts from: the project's own
    /// `Cargo.lock` when it has one, so every package it already locks keeps
    /// its version and Rawr's runner reuses what `cargo build` compiled, and
    /// the universe's seed otherwise. Admission still fixes only iced (D402).
    pub(crate) fn starting_lock<'a>(&'a self, definition: &'a universe::Definition) -> &'a [u8] {
        self.authority.lock.as_deref().unwrap_or(definition.seed())
    }

    #[allow(
        clippy::too_many_arguments,
        reason = "one shared Fresh phase preserves the legacy and reserved command sequences"
    )]
    fn materialize_fresh(
        &self,
        limits: &Limits,
        cargo_allocation: &mut unit::Allocation,
        allocation: &mut RequestAllocation,
        definition: &universe::Definition,
        paths: (&Path, &Path, &Path),
        mut observe: impl FnMut(MetadataPhase, &mut RequestAllocation) -> Result<Box<[u8]>>,
        parse: impl FnOnce(&[u8], &mut unit::Allocation) -> Result<unit::Metadata>,
    ) -> Result<Materialized> {
        let mut provisional_scope = cargo_allocation.begin_provisional()?;
        let provisional = (|| {
            let bytes = observe(MetadataPhase::Provisional, allocation)?;
            self.revalidate(limits, "provisional metadata")?;
            let metadata = parse(&bytes, cargo_allocation)?;
            let selected = metadata.selections(&self.dependencies, cargo_allocation)?;
            let internal = self
                .internal
                .seal_accounted(&metadata, &selected, allocation)?;
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
        let manifest = render_manifest_accounted(self, Some(&internal), limits, allocation)?;
        write_atomic(manifest_path, manifest.as_bytes())?;
        write_atomic(source_path, HARNESS_SOURCE.as_bytes())?;
        write_atomic(lock_path, self.starting_lock(definition))?;
        // The candidate's metadata is the final metadata: Cargo wrote the lock
        // it resolved, and the `--locked` unit graph that follows refuses a
        // lock it would change. Running `--locked` metadata again only
        // reproduced these bytes.
        let bytes = observe(MetadataPhase::Candidate, allocation)?;
        self.revalidate(limits, "resolution candidate materialization")?;
        let lock_bytes = read_bounded(
            "candidate resolution lock",
            lock_path,
            process::CAPTURE_BYTES,
        )?;
        let lock = definition.candidate(lock_bytes.into_boxed_slice(), limits)?;
        Ok(Materialized {
            bytes,
            internal,
            lock,
        })
    }

    #[allow(
        clippy::too_many_arguments,
        clippy::type_complexity,
        reason = "the shared transformation binds final metadata, selections and the internal recipe"
    )]
    fn bind_fresh_metadata(
        &self,
        metadata: unit::Metadata,
        internal: Option<internal::Recipe>,
        lock: &universe::Candidate,
        paths: (&Path, &Path),
        limits: &Limits,
        cargo_limits: &Limits,
        allocation: &mut unit::Allocation,
        request_allocation: &mut RequestAllocation,
    ) -> Result<(unit::Metadata, Box<[Selection]>, internal::Recipe)> {
        let metadata = metadata.bind_lock(lock.bytes(), cargo_limits, allocation)?;
        let selected = metadata.selections(&self.dependencies, allocation)?;
        let internal = match internal {
            Some(internal) => internal,
            None => {
                let internal =
                    self.internal
                        .seal_accounted(&metadata, &selected, request_allocation)?;
                require_exact_file(
                    "reused final-form harness manifest",
                    paths.0,
                    render_manifest_accounted(self, Some(&internal), limits, request_allocation)?
                        .as_bytes(),
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

    fn require_limits(&self, limits: &Limits) -> Result<()> {
        if self.allocation.matches(limits) {
            Ok(())
        } else {
            Err(Error::Request {
                field: "request limits",
                reason: "differ from the limits that sealed this request".into(),
            })
        }
    }
}

impl Request {
    fn capture_workspace(
        &self,
        metadata: &unit::Metadata,
        allocation: &mut RequestAllocation,
        graph: &crate::node::GraphBudget,
        limits: &Limits,
    ) -> Result<crate::input::workspace::Image> {
        let mut account = crate::input::workspace::Account::borrow(allocation, graph, limits);
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
        metadata: &unit::Metadata,
        mut initial: unit::BuildGraph,
        cargo_allocation: &unit::Allocation,
        toolchain: &toolchain::Proof,
        files: [(&Path, Digest); 3],
        limits: &Limits,
        cargo_limits: &Limits,
        workspace: &crate::input::workspace::Image,
        graph: &crate::node::GraphBudget,
        allocation: &mut RequestAllocation,
        postcheck: Option<impl FnMut(Postcheck, &mut RequestAllocation) -> Result<Box<[u8]>>>,
    ) -> Result<(crate::input::build::Build, unit::BuildGraph)> {
        let build = crate::input::build::Build::parse(
            bytes,
            environment,
            toolchain.cargo_commit(),
            limits,
        )?;
        let revalidate = |allocation: &mut RequestAllocation| {
            revalidate_harness(
                self, files[0].0, files[0].1, files[1].0, files[1].1, files[2].0, files[2].1,
                limits, "build",
            )?;
            workspace.require(&mut crate::input::workspace::Account::borrow(
                allocation, graph, limits,
            ))
        };
        revalidate(allocation)?;
        if let Some(mut observe) = postcheck {
            let units = observe(Postcheck::Units, allocation)?;
            initial.require_exact_raw(&units, "build")?;
            let post_metadata = observe(Postcheck::Metadata, allocation)?;
            metadata.require_exact_raw(&post_metadata, "build")?;
        }
        initial.bind_reachability(cargo_allocation, cargo_limits)?;
        revalidate(allocation)?;
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
            request_allocation,
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
        } = harness;
        let mut allocation = request_allocation.ok_or_else(|| Error::Request {
            field: "legacy workspace inputs",
            reason: "original request account is absent".into(),
        })?;
        workspace.require(&mut crate::input::workspace::Account::borrow(
            &mut allocation,
            node_context.graph_budget(),
            limits,
        ))?;
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
            let authority = authoritative_build_node_authority(
                request,
                &toolchain,
                &self,
                &manifest_path,
                limits,
            )?;
            let authority = workspace_node_authority(
                &authority,
                &workspace,
                &mut allocation,
                node_context.graph_budget(),
            )?;
            let execution_environment = environment.clone();
            let build = crate::cargo::resolution::cached_build(
                &context,
                &unit,
                &authority,
                extraction_plan.target_directory(),
                extraction_plan.protocol_directory(),
                &crate::metrics::Current,
                || {
                    workspace.require(&mut crate::input::workspace::Account::borrow(
                        &mut allocation,
                        node_context.graph_budget(),
                        limits,
                    ))?;
                    let bytes = cargo_execute_authoritative_build(
                        native,
                        &self,
                        &manifest_path,
                        execution_environment,
                        limits,
                    )?;
                    workspace.require(&mut crate::input::workspace::Account::borrow(
                        &mut allocation,
                        node_context.graph_budget(),
                        limits,
                    ))?;
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
        #[cfg(feature = "rustc-driver")]
        let mut post_unit_parent = None;
        #[cfg(feature = "rustc-driver")]
        let mut post_metadata_parent = None;
        let (build, build_unit) =
            self.check_build(
                &bytes,
                environment,
                &metadata,
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
                node_context.graph_budget(),
                &mut allocation,
                Some(
                    |phase,
                     #[cfg_attr(not(feature = "rustc-driver"), allow(unused_variables))]
                     allocation: &mut RequestAllocation| match phase {
                        Postcheck::Units => {
                            #[cfg(feature = "rustc-driver")]
                            if let Some((context, request, build)) = dag_build.as_ref() {
                                let authority = post_build_unit_graph_node_authority(
                                    *request,
                                    &toolchain,
                                    &self,
                                    &manifest_path,
                                    limits,
                                )?;
                                let authority = workspace_node_authority(
                                    &authority,
                                    &workspace,
                                    allocation,
                                    node_context.graph_budget(),
                                )?;
                                let post_unit = crate::cargo::resolution::cached_post_unit_graph(
                                    context,
                                    build,
                                    &authority,
                                    &crate::metrics::Current,
                                    || {
                                        workspace.require(
                                            &mut crate::input::workspace::Account::borrow(
                                                allocation,
                                                node_context.graph_budget(),
                                                limits,
                                            ),
                                        )?;
                                        let bytes = cargo_units(
                                            native,
                                            &self,
                                            &manifest_path,
                                            &command_target,
                                            limits,
                                        )?;
                                        workspace.require(
                                            &mut crate::input::workspace::Account::borrow(
                                                allocation,
                                                node_context.graph_budget(),
                                                limits,
                                            ),
                                        )?;
                                        Ok(bytes)
                                    },
                                )
                                .map_err(|source| Error::Node {
                                    operation: "post-build Cargo unit graph",
                                    source: Box::new(source),
                                })?;
                                let bytes = post_unit.value().raw().to_vec().into_boxed_slice();
                                post_unit_parent = Some(post_unit);
                                return Ok(bytes);
                            }
                            uncached(execution::Operation::UnitGraph).run(|| {
                                cargo_units(native, &self, &manifest_path, &command_target, limits)
                            })
                        }
                        Postcheck::Metadata => {
                            #[cfg(feature = "rustc-driver")]
                            if let Some((context, request, build)) = dag_build.as_ref() {
                                let authority = post_build_metadata_node_authority(
                                    *request,
                                    &toolchain,
                                    &self,
                                    &manifest_path,
                                    limits,
                                )?;
                                let authority = workspace_node_authority(
                                    &authority,
                                    &workspace,
                                    allocation,
                                    node_context.graph_budget(),
                                )?;
                                let post_metadata = crate::cargo::resolution::cached_post_metadata(
                                    context,
                                    build,
                                    &authority,
                                    &manifest_path,
                                    &lock_path,
                                    &command_target,
                                    &crate::metrics::Current,
                                    || {
                                        workspace.require(
                                            &mut crate::input::workspace::Account::borrow(
                                                allocation,
                                                node_context.graph_budget(),
                                                limits,
                                            ),
                                        )?;
                                        let bytes = cargo_metadata(
                                            native,
                                            &self,
                                            &manifest_path,
                                            &command_target,
                                            true,
                                            limits,
                                        )?;
                                        metadata.require_exact_raw(
                                            &bytes,
                                            "workspace-bound post-build metadata",
                                        )?;
                                        workspace.require(
                                            &mut crate::input::workspace::Account::borrow(
                                                allocation,
                                                node_context.graph_budget(),
                                                limits,
                                            ),
                                        )?;
                                        Ok(bytes)
                                    },
                                )
                                .map_err(|source| Error::Node {
                                    operation: "post-build Cargo metadata",
                                    source: Box::new(source),
                                })?;
                                let bytes = post_metadata.value().raw().to_vec().into_boxed_slice();
                                post_metadata_parent = Some(post_metadata);
                                return Ok(bytes);
                            }
                            uncached(execution::Operation::Metadata).run(|| {
                                cargo_metadata(
                                    native,
                                    &self,
                                    &manifest_path,
                                    &command_target,
                                    true,
                                    limits,
                                )
                            })
                        }
                    },
                ),
            )?;
        let package_scope = uncached(execution::Operation::Package);
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
            request_allocation: None,
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
        };
        #[allow(unused_mut)]
        let mut resolution = self.complete_harness(
            native,
            harness,
            build,
            &mut allocation,
            limits,
            [manifest_path, source_path, lock_path],
            &receipt_path,
            Some(&package_scope),
            #[cfg(feature = "rustc-driver")]
            Some(&probe_scope),
            #[cfg(feature = "rustc-driver")]
            None,
        )?;
        #[cfg(feature = "rustc-driver")]
        {
            resolution.dag = match (dag_build, post_unit_parent, post_metadata_parent) {
                (Some((context, request, build)), Some(post_unit), Some(post_metadata)) => {
                    Some(DagResolution {
                        context,
                        request,
                        build,
                        post_unit,
                        post_metadata,
                    })
                }
                (None, None, None) => None,
                _ => {
                    return Err(Error::ResolutionDrift {
                        command: "DAG resolution completion",
                        field: "post-build node closure",
                    });
                }
            };
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
        allocation: &mut RequestAllocation,
        limits: &Limits,
        files: [PathBuf; 3],
        receipt_path: &Path,
        package_scope: Option<&execution::Scope>,
        #[cfg(feature = "rustc-driver")] probe_scope: Option<&execution::Scope>,
        #[cfg(feature = "rustc-driver")] reserved_layout: Option<&resolution::layout::Layout>,
    ) -> Result<Resolution> {
        let complete = self.complete_harness_parts(
            native,
            harness,
            build,
            allocation,
            limits,
            files,
            receipt_path,
            package_scope,
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
        native: &impl ProductionReplay,
        harness: Harness,
        mut build: crate::input::build::Build,
        allocation: &mut RequestAllocation,
        limits: &Limits,
        files: [PathBuf; 3],
        receipt_path: &Path,
        package_scope: Option<&execution::Scope>,
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
            request_allocation: _,
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
        } = harness;
        let fallback_packages = build.fallback_packages(toolchain.cargo_commit(), limits)?;
        for (package_id, package_manifest) in fallback_packages {
            let package = metadata.package(&package_id).ok_or_else(|| Error::Input {
                code: "missing-fallback-package-authority",
                reason: package_id.clone(),
            })?;
            let files = cargo_package_files(
                native,
                package_scope,
                self,
                package,
                &package_manifest,
                &command_target,
                toolchain.cargo_commit(),
                build.usage(),
                limits,
                allocation,
                #[cfg(feature = "rustc-driver")]
                reserved_layout,
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
        let probe_artifacts = capture
            .as_mut()
            .map(|capture| capture.artifact_inputs(limits))
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
            self.config.input_config_accounted(allocation)?,
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
    pub fn cargo_authority(mut self, manifest: &Path) -> Result<Self> {
        self.allocation.path(manifest)?;
        self.cargo_authority = Some(manifest.to_path_buf());
        Ok(self)
    }

    /// Select a supported target triple instead of the pinned host.
    pub fn target(mut self, target: &str) -> Result<Self> {
        self.allocation.string(target)?;
        self.target = Some(nonempty("Cargo target", target)?);
        Ok(self)
    }

    /// Select one Cargo profile from the authority copied into the harness.
    pub fn profile(mut self, profile: &str) -> Result<Self> {
        let valid = profile.bytes().enumerate().all(|(index, byte)| {
            byte.is_ascii_alphanumeric() || byte == b'_' || index > 0 && byte == b'-'
        });
        if profile.is_empty() || !valid {
            return Err(Error::Request {
                field: "Cargo profile",
                reason: format!("`{profile}` is not a Cargo profile name").into_boxed_str(),
            });
        }
        self.allocation.string(profile)?;
        self.profile = profile.into();
        Ok(self)
    }

    /// Add one dependency with a final graph role.
    pub fn dependency(mut self, dependency: config::Dependency) -> Result<Self> {
        self.allocation
            .transfer_with_items(dependency.allocation(), 1)?;
        self.dependencies
            .try_reserve(1)
            .map_err(|error| Error::Request {
                field: "Rust dependencies",
                reason: error.to_string().into_boxed_str(),
            })?;
        self.dependencies.push(dependency);
        Ok(self)
    }

    /// Bind one preselected framework resolution universe to this request.
    #[doc(hidden)]
    pub fn resolution_universe(mut self, universe: universe::Catalog) -> Result<Self> {
        if self.universe.is_some() {
            return Err(Error::Request {
                field: "resolution universe",
                reason: "may be selected exactly once".into(),
            });
        }
        self.universe = Some(universe);
        Ok(self)
    }

    /// Demand one Rust path Rawr names, root alias first. A
    /// dependency with no demanded path is extracted whole; one with
    /// demand is seeded from its demanded paths only: each item a path names
    /// brings its closure (signatures, public fields, inherent and trait impl
    /// tables), by the same probe against the existing metadata. Paths
    /// through wholesale crate re-exports (`pub use other_crate;`) resolve
    /// the same way. Order and duplicates do not matter.
    pub fn demand<'a>(mut self, path: impl IntoIterator<Item = &'a str>) -> Result<Self> {
        let mut segments = Vec::new();
        for segment in path {
            if segment.is_empty() {
                return Err(Error::Request {
                    field: "Rust demand path",
                    reason: "segments must be nonempty".into(),
                });
            }
            self.allocation.string(segment)?;
            segments.push(Box::<str>::from(segment));
        }
        if segments.len() < 2 {
            return Err(Error::Request {
                field: "Rust demand path",
                reason: "must name an item below a dependency alias".into(),
            });
        }
        self.allocation.items(1)?;
        self.demand.insert(segments.into_boxed_slice());
        Ok(self)
    }

    /// Bind the exact Rawr-manifest exclusion policy to this request.
    pub fn policy(mut self, policy: crate::extract::Policy) -> Result<Self> {
        self.allocation.transfer_policy(&policy)?;
        self.policy = Some(policy);
        Ok(self)
    }

    /// Promote loose request fields into canonical path/config/authority proof.
    pub fn build(mut self) -> Result<Request> {
        if self.inventory && !self.demand.is_empty() {
            return Err(Error::Request {
                field: "intrinsic inventory",
                reason: "intrinsic acquisition cannot contain consuming-program demand".into(),
            });
        }
        let limits = &self.limits;
        let project_root = canonical_directory("project root", &self.project_root)?;
        let expected_cache = project_root.join("target/rawr/cache");
        let requested_cache = lexical_absolute(&self.cache_root)?;
        if requested_cache != expected_cache {
            return Err(Error::Path {
                field: "project cache root",
                path: self.cache_root,
                reason: format!("must be `{}`", expected_cache.display()).into_boxed_str(),
            });
        }
        reject_existing_symlink(&project_root, &expected_cache)?;
        let environment = self.environment.ok_or_else(|| Error::Request {
            field: "Cargo environment",
            reason: "must be captured or explicitly supplied".into(),
        })?;
        self.allocation.items(environment.entries().len())?;
        for (key, value) in environment.entries() {
            self.allocation.string(key)?;
            self.allocation.string(value)?;
        }
        let policy = self.policy.ok_or_else(|| Error::Request {
            field: "Rust exclusion policy",
            reason: "must be derived from the validated Rawr manifest".into(),
        })?;
        if self.dependencies.is_empty() {
            return Err(Error::Request {
                field: "Rust dependencies",
                reason: "at least one selected library is required".into(),
            });
        }
        let internal = internal::Seed::transform(&crate::host::root()?, &mut self.allocation)?;
        let count = self.dependencies.len().saturating_add(1);
        self.allocation
            .exceeded("Rust dependencies", limits.packages, count);
        self.allocation.items(count)?;
        let mut dependencies = Vec::new();
        dependencies
            .try_reserve_exact(count)
            .map_err(|error| Error::Request {
                field: "Rust dependencies",
                reason: error.to_string().into_boxed_str(),
            })?;
        for dependency in self.dependencies {
            dependencies.push(dependency.seal(&project_root, &mut self.allocation)?);
        }
        let internal_dependency = internal.dependency(limits)?;
        self.allocation.transfer(internal_dependency.allocation())?;
        dependencies.push(internal_dependency.seal(&project_root, &mut self.allocation)?);
        dependencies.sort_unstable_by(|left, right| left.alias().cmp(right.alias()));
        if dependencies
            .windows(2)
            .any(|pair| pair[0].alias() == pair[1].alias())
        {
            return Err(Error::Request {
                field: "Rust dependencies",
                reason: "dependency aliases must be unique".into(),
            });
        }
        for role in [
            config::Role::Library,
            config::Role::Runtime,
            config::Role::Compiler,
        ] {
            if !dependencies
                .iter()
                .any(|dependency| dependency.roles().contains(role))
            {
                return Err(Error::Request {
                    field: "final-form Cargo universe",
                    reason: format!("missing {role:?} dependency role").into_boxed_str(),
                });
            }
        }

        let toolchain = toolchain::Requirement::pinned();
        let parallelism = process::parallelism::Proof::discover()?;
        let target = match self.target {
            Some(target) => target,
            None => {
                self.allocation.string(toolchain.default_target())?;
                toolchain.default_target().into()
            }
        };
        if !toolchain.supports_target(&target) {
            return Err(Error::Request {
                field: "Cargo target",
                reason: format!("`{target}` is outside the pinned supported target set")
                    .into_boxed_str(),
            });
        }

        let selected_authority = self
            .cargo_authority
            .as_deref()
            .map(|path| canonical_authority(path, &mut self.allocation))
            .transpose()?;
        let mut authority = Authority::load(selected_authority.as_deref(), &mut self.allocation)?;
        if let (Some(universe), Some(patch)) = (self.universe.as_ref(), internal.patch()) {
            authority.inherit_patches(
                patch,
                |name| universe.names_package(name),
                &mut self.allocation,
            )?;
        }
        let config = config::Provenance::discover_budgeted(
            &project_root,
            selected_authority.as_deref(),
            environment,
            &target,
            &self.profile,
            parallelism,
            limits,
            &mut self.allocation,
        )?;
        let inputs = authority_inputs(&authority, &dependencies, &mut self.allocation)?;
        if let Some(path) = self.demand.iter().find(|path| {
            !dependencies
                .iter()
                .any(|dependency| dependency.alias() == path[0].as_ref())
        }) {
            return Err(Error::Request {
                field: "Rust demand path",
                reason: format!("`{}` is not a declared dependency alias", path[0])
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
            allocation: self.allocation,
        })
    }
}

impl Resolution {
    #[cfg(feature = "rustc-driver")]
    pub(crate) fn into_native_inputs(
        self,
        limits: &Limits,
    ) -> Result<crate::applicability::native::Inputs> {
        if self.capture.is_some() {
            return Err(Error::Request {
                field: "application native inputs",
                reason: "API capture cannot replace a consuming native input proof".into(),
            });
        }
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
            capture.ok_or_else(|| Error::Request {
                field: "API publication",
                reason: "native inputs do not establish compiler API facts".into(),
            })?,
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
        let _process_owner = match HEAVY_OWNER.try_lock() {
            Ok(owner) => owner,
            Err(TryLockError::WouldBlock) => {
                return Err(Error::Owner {
                    state: "process busy",
                });
            }
            Err(TryLockError::Poisoned(_)) => {
                return Err(Error::Owner {
                    state: "process gate poisoned",
                });
            }
        };
        let mut lease = open_lease_at(&self.project_root, &self.cache_root)?;
        let _lease_owner = lease.try_write().map_err(|source| {
            if source.kind() == std::io::ErrorKind::WouldBlock {
                Error::Owner {
                    state: "cache busy",
                }
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
        // constant, never by an accounting target.
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
        let manifest = self.harness_root.join(HARNESS_MANIFEST);
        let target_dir = self.harness_root.join(AUTHORITATIVE_TARGET_DIRECTORY);
        let metadata_bytes = replay_metadata(native, self, &manifest, &target_dir, limits)?;
        self.metadata
            .require_exact_raw(&metadata_bytes, "publication validation")?;
        self.internal.revalidate(&self.metadata, &self.selected)?;
        let unit_bytes = replay_units(native, self, &manifest, &target_dir, limits)?;
        self.build_unit
            .require_exact_raw(&unit_bytes, "publication validation")?;
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
    fn inherit_patches(
        &mut self,
        rawr: &Item,
        needed: impl Fn(&str) -> bool,
        allocation: &mut RequestAllocation,
    ) -> Result<()> {
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
                allocation.items(1)?;
                allocation.string(name)?;
                registry_table.insert(name, specification.clone());
            }
        }
        Ok(())
    }

    fn load(manifest: Option<&Path>, allocation: &mut RequestAllocation) -> Result<Self> {
        let Some(manifest) = manifest else {
            let digest = digest(b"generated Cargo authority\0resolver=3");
            allocation.string("3")?;
            return Ok(Self {
                resolver: "3".into(),
                retained: BTreeMap::new(),
                digest,
                inputs: Box::default(),
                lock: None,
            });
        };
        let bytes = read_bounded_budgeted(
            "Cargo authority manifest",
            manifest,
            process::TAIL_BYTES,
            allocation,
        )?;
        allocation.retained(bytes.len(), bytes.len())?;
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
        allocation.string(resolver_value)?;
        let resolver = resolver_value.into();
        let mut retained = BTreeMap::new();
        for key in ["cargo-features", "patch", "replace", "profile"] {
            if let Some(item) = document.get(key) {
                allocation.items(1)?;
                allocation.string(key)?;
                let rendered = allocation.formatted(format_args!("{item}"))?;
                drop(rendered);
                let mut item = item.clone();
                if matches!(key, "patch" | "replace") {
                    let root = manifest.parent().ok_or_else(|| Error::Path {
                        field: "Cargo authority manifest",
                        path: manifest.to_path_buf(),
                        reason: "has no containing directory".into(),
                    })?;
                    absolutize_policy_paths(&mut item, root, allocation)?;
                }
                retained.insert(key.into(), item);
            }
        }
        let mut lock_evidence = None;
        let mut lock_input = None;
        if let Some(root) = manifest.parent() {
            allocation.retained(
                1,
                root.as_os_str().len().saturating_add("/Cargo.lock".len()),
            )?;
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
            allocation.path(&lock)?;
            lock_input = Some(Input {
                path: lock.clone(),
                digest: None,
                role: Role::Lock,
            });
            if present {
                let lock = canonical_file("Cargo authority lock", &lock)?;
                let bytes = read_bounded_budgeted(
                    "Cargo authority lock",
                    &lock,
                    process::TAIL_BYTES,
                    allocation,
                )?;
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
        allocation.retained(1, evidence_bytes)?;
        let mut evidence = Vec::new();
        evidence
            .try_reserve_exact(evidence_bytes)
            .map_err(|error| Error::Request {
                field: "Cargo authority evidence allocation",
                reason: error.to_string().into_boxed_str(),
            })?;
        append_framed(&mut evidence, manifest.as_os_str().as_encoded_bytes());
        append_framed(&mut evidence, &bytes);
        if let Some((lock, bytes)) = &lock_evidence {
            append_framed(&mut evidence, lock.as_os_str().as_encoded_bytes());
            append_framed(&mut evidence, bytes);
        }
        allocation.path(manifest)?;
        allocation.retained(2, 2 * std::mem::size_of::<Input>())?;
        let mut inputs = Vec::new();
        inputs
            .try_reserve_exact(2)
            .map_err(|source| Error::Allocation {
                operation: "interpreted Cargo authority inputs",
                source,
            })?;
        allocation.path(manifest)?;
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
        Err(TryLockError::WouldBlock) => Err(Error::Owner {
            state: "process busy",
        }),
        Err(TryLockError::Poisoned(_)) => Err(Error::Owner {
            state: "process gate poisoned",
        }),
    }
}

fn acquire_lease<'lease>(
    lease: &'lease mut fd_lock::RwLock<File>,
    path: &Path,
    operation: &'static str,
) -> Result<fd_lock::RwLockWriteGuard<'lease, File>> {
    lease.try_write().map_err(|source| {
        if source.kind() == std::io::ErrorKind::WouldBlock {
            Error::Owner {
                state: "cache busy",
            }
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
            reason: "escaped its sealed canonical project location".into(),
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
            reason: "contains a symlink or non-canonical ancestor".into(),
        });
    }
    Ok(fd_lock::RwLock::new(file))
}

fn select_placement(
    request: &Request,
    catalog: &universe::Catalog,
    provisional_manifest: &str,
    limits: &Limits,
) -> Result<Placement> {
    select_placement_inner(
        request,
        catalog,
        provisional_manifest,
        limits,
        &mut ControlAccount::legacy(),
    )
}

fn select_placement_accounted(
    request: &Request,
    catalog: &universe::Catalog,
    provisional_manifest: &str,
    limits: &Limits,
    allocation: &mut RequestAllocation,
    graph: &crate::node::GraphBudget,
) -> Result<Placement> {
    select_placement_inner(
        request,
        catalog,
        provisional_manifest,
        limits,
        &mut ControlAccount::current(allocation, graph),
    )
}

struct ControlAccount<'a> {
    current: Option<(&'a mut RequestAllocation, &'a crate::node::GraphBudget)>,
    /// A lock parsed under no request account (a persisted or reused
    /// lock read from disk, a compiled-in seed) still records the targets it
    /// crosses on its own universe account and publishes them, so the
    /// disk-read lock routes never lose an exceedance.
    legacy: Option<LegacyAccount>,
}

struct LegacyAccount {
    id: u64,
    record: crate::exceedance::Record,
}

impl<'a> ControlAccount<'a> {
    fn legacy() -> Self {
        Self {
            current: None,
            legacy: Some(LegacyAccount {
                id: crate::exceedance::next_id(),
                record: crate::exceedance::Record::new(),
            }),
        }
    }

    fn current(allocation: &'a mut RequestAllocation, graph: &'a crate::node::GraphBudget) -> Self {
        Self {
            current: Some((allocation, graph)),
            legacy: None,
        }
    }

    /// The exceedances this control account recorded so far: the request
    /// account's record on the current route, the universe account's record
    /// on the legacy route.
    #[cfg(test)]
    fn exceedances(&self) -> crate::exceedance::Record {
        match (&self.current, &self.legacy) {
            (Some((allocation, _)), _) => allocation.exceedances(),
            (None, Some(legacy)) => legacy.record,
            (None, None) => crate::exceedance::Record::new(),
        }
    }

    /// The process-unique identity the legacy route publishes under.
    #[cfg(test)]
    fn legacy_id(&self) -> Option<u64> {
        self.legacy.as_ref().map(|legacy| legacy.id)
    }

    fn read(&mut self, resource: &'static str, path: &Path, limit: usize) -> Result<Vec<u8>> {
        match self.current.as_mut() {
            Some((allocation, graph)) => read_bounded_in(resource, path, limit, allocation, graph),
            None => read_bounded(resource, path, limit),
        }
    }

    fn parser(&mut self, bytes: usize) -> Result<()> {
        if let Some((allocation, graph)) = self.current.as_mut() {
            allocation.retained(bytes, bytes)?;
            graph.decode(bytes).map_err(Self::error)?;
        }
        Ok(())
    }

    /// A lock document crossing an accounting target is recorded and
    /// never refused: on the current request account, or on this control's
    /// own universe account when it parses under no request account.
    fn exceeded(&mut self, resource: &'static str, target: usize, actual: usize) {
        match (self.current.as_mut(), self.legacy.as_mut()) {
            (Some((allocation, _)), _) => allocation.exceeded(resource, target, actual),
            (None, Some(legacy)) => {
                if actual > target
                    && legacy
                        .record
                        .exceeded(resource, target as u64, actual as u64, None)
                {
                    crate::exceedance::publish(
                        crate::exceedance::Account::Universe,
                        legacy.id,
                        &legacy.record,
                    );
                }
            }
            (None, None) => {}
        }
    }

    /// One fixed shared-holder allocation retained with its parsed owner.
    fn holder(&mut self, bytes: usize) -> Result<()> {
        if let Some((allocation, graph)) = self.current.as_mut() {
            allocation.retained(1, bytes)?;
            graph.decode(bytes).map_err(Self::error)?;
        }
        Ok(())
    }

    fn hash(&self, bytes: usize) -> Result<()> {
        if let Some((_, graph)) = self.current.as_ref() {
            graph.hash(bytes).map_err(Self::error)?;
        }
        Ok(())
    }

    fn path(&mut self, path: &Path) -> Result<()> {
        if let Some((allocation, graph)) = self.current.as_mut() {
            allocation.path(path)?;
            graph.paths(1).map_err(Self::error)?;
            graph.decode(path.as_os_str().len()).map_err(Self::error)?;
        }
        Ok(())
    }

    fn error(source: crate::node::Error) -> Error {
        Error::Node {
            operation: "current resolution controls",
            source: Box::new(source),
        }
    }
}

/// The fixed namespace's placement when a completed receipt for this request
/// is there, `None` when it is another request's, and an error when it is not
/// this version's.
fn fixed_placement(
    fixed: &Path,
    request_identity: Digest,
    definition: &universe::Definition,
    limits: &Limits,
    account: &mut ControlAccount<'_>,
) -> Result<Option<Placement>> {
    let fixed_lock_path = fixed.join("Cargo.lock");
    account.path(&fixed_lock_path)?;
    if !fixed_lock_path.exists() {
        return Err(Error::ResolutionDrift {
            command: "persisted universe preflight",
            field: "final lock is absent from an existing namespace",
        });
    }
    let fixed_lock_bytes = account.read(
        "persisted final resolution lock",
        &fixed_lock_path,
        process::CAPTURE_BYTES,
    )?;
    let fixed_source = account.read(
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
    let fixed_manifest = account.read(
        "persisted final-form harness manifest",
        &fixed.join(HARNESS_MANIFEST),
        process::TAIL_BYTES,
    )?;
    let receipt_path = fixed.join(UNIVERSE_RECEIPT);
    account.path(&receipt_path)?;
    if receipt_path.exists() {
        // Receipt and lock reads are I/O retention bounded by the
        // physical capture constant, never by the JSON accounting target.
        let bytes = account.read(
            "resolution universe receipt",
            &receipt_path,
            process::CAPTURE_BYTES,
        )?;
        account.parser(bytes.len())?;
        let selected_identity = universe::Receipt::universe(&bytes)?;
        if selected_identity != *definition.identity() {
            return Err(Error::ResolutionDrift {
                command: "persisted universe selection",
                field: "current runnable receipt universe",
            });
        }
        let fixed_lock =
            definition.candidate_in(fixed_lock_bytes.into_boxed_slice(), limits, account)?;
        account.parser(bytes.len())?;
        let receipt = universe::Receipt::parse(&bytes, definition)?;
        account.hash(fixed_manifest.len() + fixed_source.len())?;
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

fn select_placement_inner(
    request: &Request,
    catalog: &universe::Catalog,
    provisional_manifest: &str,
    limits: &Limits,
    account: &mut ControlAccount<'_>,
) -> Result<Placement> {
    let definition = catalog.current();
    account.hash(
        b"rawr.import.cargo.resolution-request/v2".len()
            + provisional_manifest.len()
            + HARNESS_SOURCE.len()
            + request.target.len()
            + 112,
    )?;
    for input in &request.inputs {
        account
            .hash(input.path.as_os_str().len() + 9 + usize::from(input.digest.is_some()) * 32)?;
    }
    let request_identity = versioned_request_identity(request, definition, provisional_manifest);
    let fixed = request.cache_root.join(HARNESS_DIRECTORY);
    account.path(&fixed)?;
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
                reason: "must be a real directory".into(),
            });
        }
        Ok(_) => {}
    }

    // A namespace another version wrote, or one whose receipt is absent or
    // for another request, is a clean miss: a side-by-side candidate is built.
    match fixed_placement(&fixed, request_identity, definition, limits, account) {
        Ok(Some(placement)) => return Ok(placement),
        Err(error @ Error::Io { .. }) => return Err(error),
        Ok(None) | Err(_) => {}
    }
    let candidate = fixed
        .join(CANDIDATE_DIRECTORY)
        .join(format!("{request_identity}"));
    account.path(&candidate)?;
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
                reason: "must be a real directory".into(),
            })
        }
        Ok(_) => {
            let lock_bytes = account.read(
                "persisted final resolution lock",
                &candidate.join("Cargo.lock"),
                process::CAPTURE_BYTES,
            )?;
            let lock = definition.candidate_in(lock_bytes.into_boxed_slice(), limits, account)?;
            let manifest = account.read(
                "candidate final-form harness manifest",
                &candidate.join(HARNESS_MANIFEST),
                process::TAIL_BYTES,
            )?;
            let source = account.read(
                "candidate generated harness source",
                &candidate.join("src/lib.rs"),
                process::TAIL_BYTES,
            )?;
            let receipt_bytes = account.read(
                "candidate resolution universe receipt",
                &candidate.join(UNIVERSE_RECEIPT),
                process::CAPTURE_BYTES,
            )?;
            account.parser(receipt_bytes.len())?;
            let selected_identity = universe::Receipt::universe(&receipt_bytes)?;
            if selected_identity != *definition.identity() {
                return Err(Error::ResolutionDrift {
                    command: "side-by-side candidate preflight",
                    field: "current universe identity",
                });
            }
            account.parser(receipt_bytes.len())?;
            let receipt = universe::Receipt::parse(&receipt_bytes, definition)?;
            if !receipt.matches_request(request_identity) {
                return Err(Error::ResolutionDrift {
                    command: "side-by-side candidate preflight",
                    field: "request identity",
                });
            }
            account.hash(manifest.len() + source.len())?;
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

fn prepare_versioned_roots(
    request: &Request,
    harness_root: &Path,
    exclusive: bool,
    limits: &Limits,
    allocation: &mut RequestAllocation,
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
    let manifest = render_manifest_accounted(request, None, limits, allocation)?;
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

fn prepare_harness(
    request: &Request,
    limits: &Limits,
    allocation: &mut RequestAllocation,
) -> Result<PathBuf> {
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
            reason: "escaped the canonical project root".into(),
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
    // The rendered manifest size is an accounting target charged to the
    // caller's working request account, which records any exceedance.
    let manifest = render_manifest_accounted(request, None, limits, allocation)?;
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
/// never by a configurable accounting target.
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

/// Test-only render on a throwaway copy of the sealed request account. Every
/// production render charges a caller's working account through
/// `render_manifest_accounted`, so the manifest's exceedance record is
/// published with that account and never on a temporary that a later publish
/// overwrites.
#[cfg(test)]
fn render_manifest(
    request: &Request,
    internal: Option<&internal::Recipe>,
    limits: &Limits,
) -> Result<String> {
    let mut allocation = request.allocation;
    render_manifest_accounted(request, internal, limits, &mut allocation)
}

/// Render one harness manifest charged to the caller's working request
/// account. Every render allocates and is charged in full, including a
/// re-render of bytes an earlier render produced: no rendered manifest is
/// retained for reuse, so matching content never excuses a charge (totals
/// are conserved, never lost and never duplicated).
fn render_manifest_accounted(
    request: &Request,
    internal: Option<&internal::Recipe>,
    limits: &Limits,
    allocation: &mut RequestAllocation,
) -> Result<String> {
    if !request.allocation.matches(limits) || !allocation.matches(limits) {
        return Err(Error::Request {
            field: "harness manifest allocation",
            reason: "request and rendering limit profiles differ".into(),
        });
    }
    allocation.items(12)?;
    for value in [
        "cargo-features",
        "workspace",
        "resolver",
        request.authority.resolver.as_ref(),
        "package",
        "name",
        HARNESS_PACKAGE,
        "version",
        HARNESS_VERSION,
        "edition",
        HARNESS_EDITION,
        "publish",
        "lib",
        "path",
        "src/lib.rs",
        "dependencies",
    ] {
        allocation.string(value)?;
    }
    let mut document = DocumentMut::new();
    if let Some(item) = request.authority.retained.get("cargo-features") {
        let rendered = allocation.formatted(format_args!("{item}"))?;
        drop(rendered);
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
            allocation.items(1)?;
            allocation.string(key)?;
            let rendered = allocation.formatted(format_args!("{item}"))?;
            drop(rendered);
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
        allocation.items(1)?;
        allocation.string(dependency.alias())?;
        if let Some(package) = dependency.package() {
            allocation.items(1)?;
            allocation.string("package")?;
            allocation.string(package)?;
            specification.insert("package", Value::from(package));
        }
        match dependency.source() {
            config::Source::Path(path) => {
                let path = path_string("dependency", path)?;
                allocation.items(1)?;
                allocation.string("path")?;
                allocation.string(path)?;
                specification.insert("path", Value::from(path));
            }
            config::Source::Registry { manifest, .. } => {
                allocation.items(1)?;
                allocation.string("version")?;
                allocation.string(manifest)?;
                specification.insert("version", Value::from(manifest.as_ref()));
            }
            config::Source::Git { url, rev } => {
                allocation.items(2)?;
                allocation.string("git")?;
                allocation.string(url)?;
                allocation.string("rev")?;
                allocation.string(rev)?;
                specification.insert("git", Value::from(url.as_ref()));
                specification.insert("rev", Value::from(rev.as_ref()));
            }
        }
        allocation.items(1)?;
        allocation.string("default-features")?;
        specification.insert(
            "default-features",
            Value::from(dependency.default_features()),
        );
        if !dependency.features().is_empty() {
            allocation.items(dependency.features().len().saturating_add(1))?;
            allocation.string("features")?;
            let mut features = Array::new();
            for feature in dependency.features() {
                allocation.string(feature)?;
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
        allocation.items(1)?;
        allocation.string(BUILD_DEPENDENCIES)?;
        document[BUILD_DEPENDENCIES] = Item::Table(build_dependencies);
    }
    if let Some(internal) = internal {
        internal.apply_profile_budgeted(&mut document, &request.profile, allocation)?;
    }
    allocation.formatted(format_args!("{document}"))
}

fn finalize_harness(
    request: &Request,
    harness_root: &Path,
    internal: &internal::Recipe,
    limits: &Limits,
    allocation: &mut RequestAllocation,
) -> Result<()> {
    // The caller's working request account charges the rendered
    // manifest and records any exceedance of the request byte target; nothing
    // is refused here.
    let manifest = render_manifest_accounted(request, Some(internal), limits, allocation)?;
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

#[expect(
    clippy::too_many_arguments,
    reason = "fallback capture binds one package to its replay command, active allocation lineage, and pinned Cargo authority"
)]
fn cargo_package_files(
    native: &impl ProductionReplay,
    execution: Option<&execution::Scope>,
    request: &Request,
    package: &unit::Package,
    manifest: &Path,
    target_dir: &Path,
    cargo_commit: &str,
    usage: crate::input::Usage,
    limits: &Limits,
    #[cfg_attr(not(feature = "rustc-driver"), allow(unused_variables))]
    allocation: &mut RequestAllocation,
    #[cfg(feature = "rustc-driver")] reserved_layout: Option<&resolution::layout::Layout>,
) -> Result<crate::input::package::Set> {
    if package.manifest() != manifest {
        return Err(Error::Input {
            code: "fallback-manifest-mismatch",
            reason: package.id().into(),
        });
    }
    if matches!(package.source(), unit::PackageSource::Registry { .. }) {
        let cargo_home = request.config.cargo_home()?.ok_or_else(|| Error::Input {
            code: "missing-cargo-home-authority",
            reason: package.id().into(),
        })?;
        return crate::input::package::Set::capture_registry(
            cargo_commit,
            package,
            &cargo_home,
            usage,
            limits,
        );
    }
    let action = match package.source() {
        unit::PackageSource::Path { .. } => process::CargoAction::PackagePath,
        unit::PackageSource::Git { .. } => process::CargoAction::PackageGit,
        unit::PackageSource::Registry { .. } => {
            return Err(Error::Input {
                code: "package-source-dispatch",
                reason: package.id().into(),
            });
        }
    };
    #[cfg(feature = "rustc-driver")]
    let prefix = match reserved_layout {
        Some(layout) => {
            resolution::cold::require_configs(layout)?;
            resolution::cold::prefix(request, layout.final_form().config(), allocation)?
        }
        None => config_arguments(request.config.explicit_configs()),
    };
    #[cfg(not(feature = "rustc-driver"))]
    let prefix = config_arguments(request.config.explicit_configs());
    let tail = Vec::from([
        "--list".into(),
        "--allow-dirty".into(),
        "--manifest-path".into(),
        manifest.as_os_str().to_owned(),
    ]);
    let command = process::Command::cargo(
        prefix,
        action,
        tail,
        request.project_root.clone(),
        request.config.command_environment(target_dir, native)?,
    )?;
    let run = || {
        native
            .run(&command, limits.process)?
            .checked(Program::Cargo)
    };
    let bytes = match execution {
        Some(scope) => scope.run(run)?,
        None => run()?,
    };
    match package.source() {
        unit::PackageSource::Path { .. } => crate::input::package::Set::capture_path(
            cargo_commit,
            package.id(),
            manifest,
            native.cargo(),
            command,
            &bytes,
            usage,
            limits,
        ),
        unit::PackageSource::Git { .. } => crate::input::package::Set::capture_git(
            cargo_commit,
            package,
            manifest,
            native.cargo(),
            command,
            &bytes,
            usage,
            limits,
        ),
        unit::PackageSource::Registry { .. } => Err(Error::Input {
            code: "package-source-dispatch",
            reason: package.id().into(),
        }),
    }
}

#[cfg(feature = "rustc-driver")]
fn cargo_execute_authoritative_build(
    native: &impl ToolPaths,
    request: &Request,
    manifest: &Path,
    environment: Box<[(OsString, OsString)]>,
    limits: &Limits,
) -> Result<Box<[u8]>> {
    let prefix = config_arguments(request.config.explicit_configs());
    let tail = cargo_build_arguments(manifest, &request.target, &request.profile);
    let command = cargo_build_command(request, prefix, tail, environment)?;
    native
        .run(&command, limits.process)?
        .checked(Program::Cargo)
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
/// stream past `process::CAPTURE_BYTES`. The Cargo JSON accounting
/// target `json_bytes` is not a gate on the captured bytes; it is charged and,
/// when crossed, recorded as the `Cargo JSON bytes` exceedance of the Cargo
/// JSON epoch that parses them (`unit::preflight_from`), which the CLI reports
/// as one warning per account.
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
    limits: &Limits,
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
    if bytes.len() > limits.node_input_bytes() {
        return Err(Error::Limit {
            resource: "locked Cargo metadata node authority bytes",
            limit: limits.node_input_bytes(),
            actual: bytes.len(),
        });
    }
    Ok(bytes.into_boxed_slice())
}

fn unit_graph_node_authority(
    request: Digest,
    toolchain: &toolchain::Proof,
    resolution: &Request,
    manifest: &Path,
    limits: &Limits,
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
    if bytes.len() > limits.node_input_bytes() {
        return Err(Error::Limit {
            resource: "Cargo unit-graph node authority bytes",
            limit: limits.node_input_bytes(),
            actual: bytes.len(),
        });
    }
    Ok(bytes.into_boxed_slice())
}

#[cfg(feature = "rustc-driver")]
fn authoritative_build_node_authority(
    request: Digest,
    toolchain: &toolchain::Proof,
    resolution: &Request,
    manifest: &Path,
    limits: &Limits,
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
    if bytes.len() > limits.node_input_bytes() {
        return Err(Error::Limit {
            resource: "authoritative Cargo build node authority bytes",
            limit: limits.node_input_bytes(),
            actual: bytes.len(),
        });
    }
    Ok(bytes.into_boxed_slice())
}

#[cfg(feature = "rustc-driver")]
fn post_build_unit_graph_node_authority(
    request: Digest,
    toolchain: &toolchain::Proof,
    resolution: &Request,
    manifest: &Path,
    limits: &Limits,
) -> Result<Box<[u8]>> {
    let mut prefix = config_arguments(resolution.config.explicit_configs());
    prefix.extend(["-Z".into(), "unstable-options".into()]);
    let arguments = process::Command::cargo_arguments(
        prefix,
        process::CargoAction::UnitBuild(resolution.parallelism),
        cargo_unit_arguments(manifest, &resolution.target, &resolution.profile),
    )?;
    node_authority(
        b"rawr.import.post-build-unit-node-authority/v1",
        request,
        toolchain,
        &arguments,
        "post-build Cargo unit node authority bytes",
        limits,
    )
}

#[cfg(feature = "rustc-driver")]
fn post_build_metadata_node_authority(
    request: Digest,
    toolchain: &toolchain::Proof,
    resolution: &Request,
    manifest: &Path,
    limits: &Limits,
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
    node_authority(
        b"rawr.import.post-build-metadata-node-authority/v1",
        request,
        toolchain,
        &arguments,
        "post-build Cargo metadata node authority bytes",
        limits,
    )
}

#[cfg(feature = "rustc-driver")]
fn node_authority(
    domain: &[u8],
    request: Digest,
    toolchain: &toolchain::Proof,
    arguments: &[OsString],
    resource: &'static str,
    limits: &Limits,
) -> Result<Box<[u8]>> {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(domain);
    bytes.extend_from_slice(request.as_bytes());
    bytes.extend_from_slice(&toolchain.node_digest());
    bytes.extend_from_slice(&(arguments.len() as u64).to_le_bytes());
    for argument in arguments {
        let argument = argument.as_os_str().as_encoded_bytes();
        bytes.extend_from_slice(&(argument.len() as u64).to_le_bytes());
        bytes.extend_from_slice(argument);
    }
    if bytes.len() > limits.node_input_bytes() {
        return Err(Error::Limit {
            resource,
            limit: limits.node_input_bytes(),
            actual: bytes.len(),
        });
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

#[cfg(feature = "rustc-driver")]
fn replay_metadata(
    native: &impl ProductionReplay,
    resolution: &Resolution,
    manifest: &Path,
    target_dir: &Path,
    limits: &Limits,
) -> Result<Box<[u8]>> {
    let prefix = config_arguments(resolution.retained_config.explicit_configs());
    let tail = cargo_metadata_arguments(
        resolution.toolchain.schemas().graph(),
        manifest,
        &resolution.target,
        true,
    );
    replay_cargo(
        native,
        resolution,
        target_dir,
        prefix,
        process::CargoAction::PublicationMetadata,
        tail,
        limits,
    )
}

#[cfg(feature = "rustc-driver")]
fn replay_units(
    native: &impl ProductionReplay,
    resolution: &Resolution,
    manifest: &Path,
    target_dir: &Path,
    limits: &Limits,
) -> Result<Box<[u8]>> {
    let mut prefix = config_arguments(resolution.retained_config.explicit_configs());
    prefix.extend(["-Z".into(), "unstable-options".into()]);
    let tail = cargo_unit_arguments(manifest, &resolution.target, &resolution.profile);
    replay_cargo(
        native,
        resolution,
        target_dir,
        prefix,
        process::CargoAction::PublicationUnitBuild(resolution.parallelism),
        tail,
        limits,
    )
}

#[cfg(feature = "rustc-driver")]
fn replay_cargo(
    native: &impl ProductionReplay,
    resolution: &Resolution,
    target_dir: &Path,
    prefix: Vec<OsString>,
    action: process::CargoAction,
    tail: Vec<OsString>,
    limits: &Limits,
) -> Result<Box<[u8]>> {
    let command = process::Command::cargo(
        prefix,
        action,
        tail,
        resolution.project_root.clone(),
        resolution
            .retained_config
            .command_environment(target_dir, native)?,
    )?;
    // Physical capture refuses; the JSON accounting target is charged by
    // the epoch that parses these bytes, exactly as in `run_cargo`.
    native
        .run(&command, limits.process)?
        .checked(Program::Cargo)
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

fn canonical_authority(path: &Path, allocation: &mut RequestAllocation) -> Result<PathBuf> {
    let manifest = if path.is_dir() {
        allocation.retained(
            1,
            path.as_os_str().len().saturating_add("/Cargo.toml".len()),
        )?;
        path.join("Cargo.toml")
    } else {
        allocation.path(path)?;
        path.to_path_buf()
    };
    canonical_file("Cargo authority manifest", &manifest)
}

fn authority_inputs(
    authority: &Authority,
    dependencies: &[config::Selected],
    allocation: &mut RequestAllocation,
) -> Result<Box<[Input]>> {
    let mut paths = BTreeSet::new();
    for input in &authority.inputs {
        allocation.items(1)?;
        allocation.path(&input.path)?;
        paths.insert((input.path.clone(), input.role));
    }
    for dependency in dependencies {
        if let config::Source::Path(path) = dependency.source() {
            allocation.retained(
                2,
                path.as_os_str().len().saturating_add("/Cargo.toml".len()),
            )?;
            let candidate = path.join("Cargo.toml");
            let manifest = canonical_file("dependency manifest", &candidate)?;
            paths.insert((manifest, Role::Manifest));
        }
    }
    allocation.items(paths.len())?;
    let mut inputs = Vec::new();
    inputs
        .try_reserve_exact(paths.len())
        .map_err(|error| Error::Request {
            field: "Cargo authority inputs",
            reason: error.to_string().into_boxed_str(),
        })?;
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
        let bytes = read_bounded_budgeted(
            "Cargo authority input",
            &path,
            process::TAIL_BYTES,
            allocation,
        )?;
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

/// Old Build/postcheck records never gain a new workspace certificate merely
/// because their output bytes happen to agree. Every miss is also checked on
/// both sides of its actual command before that native result can be published.
fn workspace_node_authority(
    authority: &[u8],
    workspace: &crate::input::workspace::Image,
    allocation: &mut RequestAllocation,
    graph: &crate::node::GraphBudget,
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
    allocation.retained(1, length)?;
    graph.decode(length).map_err(|source| Error::Node {
        operation: "workspace command authority",
        source: Box::new(source),
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

fn absolutize_policy_paths(
    item: &mut Item,
    authority_root: &Path,
    allocation: &mut RequestAllocation,
) -> Result<()> {
    match item {
        Item::Table(table) => {
            for (key, item) in table.iter_mut() {
                if key == "path" {
                    absolutize_path_item(item, authority_root, allocation)?;
                } else {
                    absolutize_policy_paths(item, authority_root, allocation)?;
                }
            }
        }
        Item::ArrayOfTables(tables) => {
            for table in tables.iter_mut() {
                for (key, item) in table.iter_mut() {
                    if key == "path" {
                        absolutize_path_item(item, authority_root, allocation)?;
                    } else {
                        absolutize_policy_paths(item, authority_root, allocation)?;
                    }
                }
            }
        }
        Item::Value(Value::InlineTable(table)) => {
            if let Some(path) = table.get("path").and_then(Value::as_str) {
                allocation.retained(
                    1,
                    authority_root
                        .as_os_str()
                        .len()
                        .saturating_add(1)
                        .saturating_add(path.len()),
                )?;
                let candidate = authority_root.join(path);
                let canonical = canonical_directory("Cargo authority patch path", &candidate)?;
                allocation.string(path_string("Cargo authority patch path", &canonical)?)?;
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

fn absolutize_path_item(
    item: &mut Item,
    authority_root: &Path,
    allocation: &mut RequestAllocation,
) -> Result<()> {
    let path = item.as_str().ok_or_else(|| Error::Schema {
        document: "Cargo authority policy",
        reason: "`path` must be a string".into(),
    })?;
    allocation.retained(
        1,
        authority_root
            .as_os_str()
            .len()
            .saturating_add(1)
            .saturating_add(path.len()),
    )?;
    let candidate = authority_root.join(path);
    let canonical = canonical_directory("Cargo authority patch path", &candidate)?;
    let canonical = path_string("Cargo authority patch path", &canonical)?;
    allocation.string(canonical)?;
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

fn read_bounded(resource: &'static str, path: &Path, limit: usize) -> Result<Vec<u8>> {
    read_bounded_inner(resource, path, limit, None, None)
}

pub(crate) fn read_bounded_budgeted(
    resource: &'static str,
    path: &Path,
    limit: usize,
    allocation: &mut RequestAllocation,
) -> Result<Vec<u8>> {
    read_bounded_inner(resource, path, limit, Some(allocation), None)
}

/// Read a current replay control with both request and graph accounting.
pub(crate) fn read_bounded_in(
    resource: &'static str,
    path: &Path,
    limit: usize,
    allocation: &mut RequestAllocation,
    graph: &crate::node::GraphBudget,
) -> Result<Vec<u8>> {
    read_bounded_inner(resource, path, limit, Some(allocation), Some(graph))
}

fn read_bounded_inner(
    resource: &'static str,
    path: &Path,
    limit: usize,
    allocation: Option<&mut RequestAllocation>,
    graph: Option<&crate::node::GraphBudget>,
) -> Result<Vec<u8>> {
    if let Some(graph) = graph {
        // Admit canonical_file's three pathname probes, opening/statting the
        // handle, and both the exact read and growth probe before any I/O.
        let admit = || -> std::result::Result<(), crate::node::Error> {
            graph.paths(7)?;
            crate::store::path::canonical(path, graph)?;
            for _ in 0..3 {
                crate::store::path::input(path, graph)?;
            }
            Ok(())
        };
        admit().map_err(|source| Error::Node {
            operation: "bounded Cargo control read",
            source: Box::new(source),
        })?;
    }
    let canonical = canonical_file(resource, path)?;
    if canonical != path {
        return Err(Error::Path {
            field: resource,
            path: path.to_path_buf(),
            reason: "contains a symlink or non-canonical ancestor".into(),
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
    if let Some(allocation) = allocation {
        allocation.retained(1, length)?;
    }
    if let Some(graph) = graph {
        graph.decode(length).map_err(|source| Error::Node {
            operation: "bounded Cargo control allocation",
            source: Box::new(source),
        })?;
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
            reason: "expected a directory".into(),
        });
    }
    if canonical != normalized {
        return Err(Error::Path {
            field,
            path: path.to_path_buf(),
            reason: "contains a symlink or non-canonical ancestor".into(),
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
            reason: "expected a file".into(),
        });
    }
    if canonical != normalized {
        return Err(Error::Path {
            field,
            path: path.to_path_buf(),
            reason: "contains a symlink or non-canonical ancestor".into(),
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
                        reason: "escapes the filesystem root".into(),
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
        reason: "escapes the canonical project root".into(),
    })?;
    let mut current = root.to_path_buf();
    for component in relative.components() {
        current.push(component.as_os_str());
        match fs::symlink_metadata(&current) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return Err(Error::Path {
                    field: "Rawr-owned cache path",
                    path: current,
                    reason: "contains a symlink component".into(),
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
        reason: "is not valid Unicode Cargo TOML input".into(),
    })
}

fn nonempty(field: &'static str, value: &str) -> Result<Box<str>> {
    if value.is_empty() || value.len() > STRING_BYTES {
        return Err(Error::Request {
            field,
            reason: "must contain between 1 byte and 1 MiB".into(),
        });
    }
    Ok(value.into())
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

    #[test]
    fn current_control_read_records_exceedance_before_touching_path_when_graph_is_exhausted() {
        let graph = crate::node::GraphBudget::new();
        graph.paths(crate::node::MAX_PATH_CHECKS_PER_GRAPH).unwrap();
        assert!(!graph.exceedances().is_exceeded());
        let mut allocation = RequestAllocation::new(&Limits::standard());
        // The exhausted graph still admits the read's probes before any
        // I/O and records the path-check axis; only the missing path refuses.
        let error = read_bounded_in(
            "exhausted control read",
            Path::new("/nonexistent-rawr-control-budget-test"),
            1024,
            &mut allocation,
            &graph,
        )
        .unwrap_err();
        assert!(matches!(error, Error::Io { .. }));
        let paths = graph
            .exceedances()
            .axis(crate::node::PATHS_RESOURCE)
            .copied()
            .expect("the exhausted graph records the path-check axis");
        assert_eq!(paths.target, crate::node::MAX_PATH_CHECKS_PER_GRAPH as u64);
        assert_eq!(
            paths.first_requested,
            crate::node::MAX_PATH_CHECKS_PER_GRAPH as u64 + 7,
            "the seven-probe admission is the first exceeding charge"
        );
        assert_eq!(
            paths.high_water,
            graph.census_usage().unwrap().1 as u64,
            "the high water is the cumulative path-check account"
        );
        assert!(paths.site.is_some());
    }

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

    fn definition(limits: &Limits) -> universe::Definition {
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
            limits,
        )
        .expect("fixture universe seals")
    }

    fn request(project: &Project, changed: bool, limits: &Limits) -> Request {
        crate::store::isolate_for_tests();
        let dependency = fs::canonicalize(
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/catalog-api"),
        )
        .expect("catalog fixture canonicalizes");
        let mut dependency = config::Dependency::path("catalog", &dependency, limits)
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
            .policy(crate::extract::Policy::manifest(limits).seal())
            .expect("policy fits")
            .dependency(dependency)
            .expect("dependency fits")
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
        limits: &Limits,
    ) {
        let lock = definition
            .candidate(SEED.as_bytes().into(), limits)
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
        limits: &Limits,
    ) -> (Result<Placement>, usize) {
        let commands = std::cell::Cell::new(0_usize);
        let selected = select_placement(request, catalog, manifest, limits).inspect(|_| {
            commands.set(commands.get().saturating_add(1));
        });
        (selected, commands.get())
    }

    #[test]
    fn borrowed_manifest_renderer_keeps_prior_usage_and_exact_byte_floor() {
        let limits = Limits::standard();
        let project = Project::new("accounted-render");
        let current = request(&project, false, &limits);
        let expected = render_manifest(&current, None, &limits).expect("legacy rendering");
        let mut account = current.allocation;
        let before = account.bytes;
        let rendered = render_manifest_accounted(&current, None, &limits, &mut account)
            .expect("borrowed rendering");
        assert_eq!(rendered, expected);
        let required = account.bytes - before;
        assert!(required > 0);
        for remaining in [required, required - 1] {
            let mut account = current.allocation;
            account.bytes = account.byte_limit - remaining;
            let result = render_manifest_accounted(&current, None, &limits, &mut account);
            if remaining == required {
                assert_eq!(result.expect("exact remaining bytes"), expected);
                assert_eq!(account.bytes, account.byte_limit);
                assert!(!account.exceedances().is_exceeded());
            } else {
                // One byte short still renders and charges in full; the
                // request account records the byte axis one past its target.
                assert_eq!(result.expect("one byte short still renders"), expected);
                assert_eq!(account.bytes, account.byte_limit + 1);
                let bytes = account
                    .exceedances()
                    .axis(REQUEST_BYTES_RESOURCE)
                    .copied()
                    .expect("the short account records the byte axis");
                assert_eq!(
                    (bytes.target, bytes.high_water),
                    (account.byte_limit as u64, account.byte_limit as u64 + 1)
                );
            }
        }
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
            let request = Request::builder(&project.0, &project.cache(), &limits)
                .expect("request builder seals")
                .environment(config::Environment::capture().expect("environment captures"))
                .policy(crate::extract::Policy::manifest(&limits).seal())
                .and_then(|builder| {
                    let catalog = fs::canonicalize(
                        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                            .join("tests/fixtures/catalog-api"),
                    )
                    .expect("catalog fixture canonicalizes");
                    builder.dependency(
                        config::Dependency::path("catalog", &catalog, &limits)?
                            .package("catalog-api")?,
                    )
                })
                .and_then(|builder| builder.cargo_authority(&authority))
                .and_then(|builder| builder.build())
                .expect("request seals");
            let provisional = render_manifest(&request, None, &limits).expect("provisional");
            let key = versioned_request_identity(&request, &definition(&limits), &provisional);
            let manifest = render_manifest(&request, Some(&internal::Recipe::fixture()), &limits)
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
        let universe = universe::Catalog::new(definition(&limits));
        let manifest = render_manifest(&current, None, &limits).expect("manifest renders");
        let fresh = select_placement(&current, &universe, &manifest, &limits)
            .expect("fresh placement selects");
        assert!(matches!(fresh.state, PlacementState::Fresh));
        assert!(fresh.exclusive);
        assert_eq!(fresh.root, project.cache().join(HARNESS_DIRECTORY));

        install_legacy(&fresh.root, &manifest, SEED);
        install_current_receipt(
            &fresh.root,
            &current,
            universe.current(),
            &manifest,
            &limits,
        );
        let persisted = select_placement(&current, &universe, &manifest, &limits)
            .expect("authenticated persisted placement selects");
        assert!(matches!(persisted.state, PlacementState::Persisted { .. }));
        assert!(!persisted.exclusive);

        let changed = request(&project, true, &limits);
        let changed_manifest =
            render_manifest(&changed, None, &limits).expect("changed manifest renders");
        let before = filesystem_entries(&project.0);
        let (migration, commands) =
            select_before_command(&changed, &universe, &changed_manifest, &limits);
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

    /// The dependency count and the rendered harness manifest size are
    /// accounting targets on the request account; the request seals and the
    /// harness is prepared, each lowered axis recording its truthful figure.
    #[test]
    fn dependency_count_and_harness_manifest_record_their_targets_without_refusal() {
        // Every manifest, source and authority file read binds to the
        // physical tail constant and every lock read to the capture constant,
        // so a one-byte manifest target gates nothing below.
        assert_eq!(process::TAIL_BYTES, 1_048_576);
        assert_eq!(process::CAPTURE_BYTES, 256 * 1024 * 1024);
        let mut limits = Limits::standard();
        limits.packages = 1;
        limits.manifest_bytes = 1;
        let project = Project::new("request-targets");
        let request = request(&project, false, &limits);
        let record = request.allocation.exceedances();
        let axis = *record
            .axis("Rust dependencies")
            .expect("the dependency count records its axis");
        assert_eq!((axis.target, axis.high_water), (1, 2));
        let rendered = render_manifest(&request, None, &limits).expect("manifest renders");
        assert!(rendered.len() > 1);
        // The configured package count no longer reaches the temporaries sweep:
        // that bound is the fixed `HARNESS_ENTRIES`, so `packages = 1` gates
        // nothing here either.
        let mut allocation = request.allocation;
        let before = allocation
            .exceedances()
            .axis(REQUEST_BYTES_RESOURCE)
            .map_or(0, |axis| axis.high_water);
        let harness = prepare_harness(&request, &limits, &mut allocation)
            .expect("a manifest above the accounting target still prepares the harness");
        assert_eq!(
            fs::read(harness.join(HARNESS_MANIFEST)).expect("the harness manifest is written"),
            rendered.as_bytes()
        );
        // The manifest render charged the caller's working account, so its
        // exceedance is on that account (never on a discarded temporary).
        let bytes = allocation
            .exceedances()
            .axis(REQUEST_BYTES_RESOURCE)
            .copied()
            .expect("the rendered manifest's byte crossing is recorded on the threaded account");
        assert_eq!(bytes.target, 1);
        assert!(
            bytes.high_water >= before + rendered.len() as u64,
            "the threaded account's high water advanced by the rendered manifest"
        );
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

    /// A lock parsed under no request account (the legacy control
    /// route used by disk-read lock routes) records the targets it crosses
    /// on its own universe account instead of dropping them; the current
    /// route records on the request account as before.
    #[test]
    fn legacy_control_account_records_exceedances_instead_of_dropping_them() {
        let mut legacy = ControlAccount::legacy();
        assert!(legacy.legacy_id().is_some());
        legacy.exceeded("resolution universe lock bytes", 8, 8);
        assert!(
            !legacy.exceedances().is_exceeded(),
            "exactly the target records nothing"
        );
        legacy.exceeded("resolution universe lock bytes", 8, 9);
        legacy.exceeded("resolution universe lock precharge", 4, 6);
        legacy.exceeded("resolution universe lock bytes", 8, 12);
        let record = legacy.exceedances();
        let bytes = record
            .axis("resolution universe lock bytes")
            .copied()
            .expect("the legacy route records the crossing");
        assert_eq!(
            (bytes.target, bytes.first_requested, bytes.high_water),
            (8, 9, 12)
        );
        assert_eq!(
            record.first().unwrap().name,
            "resolution universe lock bytes"
        );
        assert!(record.axis("resolution universe lock precharge").is_some());
        assert_eq!(
            crate::exceedance::warnings(
                &[crate::exceedance::Exceedance {
                    account: crate::exceedance::Account::Universe,
                    id: legacy.legacy_id().unwrap(),
                    record,
                }],
                0
            )
            .len(),
            1,
            "one warning per account"
        );

        let limits = Limits::standard();
        let mut allocation = RequestAllocation::new(&limits);
        let graph = crate::node::GraphBudget::new();
        let mut current = ControlAccount::current(&mut allocation, &graph);
        assert!(current.legacy_id().is_none());
        current.exceeded("resolution universe lock bytes", 8, 9);
        assert_eq!(
            current
                .exceedances()
                .axis("resolution universe lock bytes")
                .map(|axis| axis.high_water),
            Some(9)
        );
        assert_eq!(
            allocation
                .exceedances()
                .axis("resolution universe lock bytes")
                .map(|axis| axis.high_water),
            Some(9),
            "the current route records on the request account"
        );

        // A persisted lock read from disk parses through the legacy route under
        // a one-byte JSON target: recorded, never refused.
        let mut lowered = Limits::standard();
        lowered.json_bytes = 1;
        definition(&lowered)
            .candidate(SEED.as_bytes().into(), &lowered)
            .expect("a lock above the JSON target still parses on the legacy route");
    }

    /// The provisional render `resolve_cached` charges (P) is carried
    /// forward on the sealed account, totals included, and every render
    /// preparation actually performs is charged on top of it. Preparation
    /// re-renders the provisional form (P again: the provisional string was
    /// dropped, so the allocation is real work) and renders a distinct form
    /// (Q), here twice: the account measures exactly P + P + Q + Q, the sum of
    /// every render performed, and the byte target is crossed once with that
    /// truthful cumulative high water.
    #[test]
    fn provisional_and_preparation_manifest_charges_are_conserved_once() {
        fn measure(
            project: &Project,
            limits: &Limits,
        ) -> (RequestAllocation, RequestAllocation, RequestAllocation) {
            let mut current = request(project, false, limits);
            let changed = request(project, true, limits);
            let sealed = current.allocation;
            let provisional = current
                .charge_provisional_manifest(limits)
                .expect("provisional render");
            let carried = current.allocation;
            // Preparation starts from the carried account, re-renders the
            // provisional manifest (charged again), renders a distinct
            // manifest (Q), then re-renders that one too (charged again).
            let mut working = current.allocation;
            let again = render_manifest_accounted(&current, None, limits, &mut working)
                .expect("provisional re-render");
            assert_eq!(again, provisional);
            assert_eq!(
                (working.items - carried.items, working.bytes - carried.bytes),
                (carried.items - sealed.items, carried.bytes - sealed.bytes),
                "an identical re-render is real work and charges P again"
            );
            let before = working;
            let distinct = render_manifest_accounted(&changed, None, limits, &mut working)
                .expect("distinct render");
            assert_ne!(distinct, provisional);
            let once = (working.items - before.items, working.bytes - before.bytes);
            let before = working;
            render_manifest_accounted(&changed, None, limits, &mut working)
                .expect("distinct re-render");
            assert_eq!(
                (working.items - before.items, working.bytes - before.bytes),
                once,
                "a distinct re-render charges Q again"
            );
            (sealed, carried, working)
        }
        let project = Project::new("conserved-render");
        let limits = Limits::standard();
        let (sealed, carried, working) = measure(&project, &limits);
        let provisional = (carried.items - sealed.items, carried.bytes - sealed.bytes);
        assert!(provisional.1 > 0, "P is measured on the sealed account");
        assert_eq!(
            carried.id, sealed.id,
            "the carried account is the same account"
        );
        // P matches a stand-alone render of the same manifest exactly.
        let alone = request(&project, false, &limits);
        let mut fresh = alone.allocation;
        render_manifest_accounted(&alone, None, &limits, &mut fresh).expect("render");
        assert_eq!(
            (
                fresh.items - alone.allocation.items,
                fresh.bytes - alone.allocation.bytes
            ),
            provisional
        );
        // Q matches a stand-alone render of the distinct manifest exactly.
        let changed = request(&project, true, &limits);
        let mut fresh = changed.allocation;
        render_manifest_accounted(&changed, None, &limits, &mut fresh).expect("render");
        let distinct = (
            fresh.items - changed.allocation.items,
            fresh.bytes - changed.allocation.bytes,
        );
        assert!(distinct.1 > 0);
        assert_eq!(
            (working.items, working.bytes),
            (
                sealed.items + 2 * provisional.0 + 2 * distinct.0,
                sealed.bytes + 2 * provisional.1 + 2 * distinct.1
            ),
            "every render performed (P, P, Q, Q) is charged, none lost or doubled"
        );

        // With the byte target one below that sum, the account crosses once and
        // records exactly the conserved cumulative total.
        let mut lowered = Limits::standard();
        lowered.manifest_bytes = working.bytes - 1;
        let (_, carried, working) = measure(&project, &lowered);
        assert!(!carried.exceedances().is_exceeded());
        let axis = working
            .exceedances()
            .axis(REQUEST_BYTES_RESOURCE)
            .copied()
            .expect("P + P + Q + Q crosses the byte target");
        assert_eq!(
            (axis.target, axis.first_requested, axis.high_water),
            (
                lowered.manifest_bytes as u64,
                lowered.manifest_bytes as u64 + 1,
                lowered.manifest_bytes as u64 + 1
            )
        );
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
            let catalog = universe::Catalog::new(definition(&limits));
            let manifest = render_manifest(&request, None, &limits).expect("manifest renders");
            let fixed = project.cache().join(HARNESS_DIRECTORY);
            install_legacy(&fixed, &manifest, lock);
            if skew_source {
                fs::write(fixed.join("src/lib.rs"), b"skew").expect("source skews");
            }
            let before = filesystem_entries(&project.0);
            let (selected, commands) =
                select_before_command(&request, &catalog, &manifest, &limits);
            assert_eq!(commands, 1, "{label}");
            assert_eq!(filesystem_entries(&project.0), before, "{label}");
            assert_clean_miss(selected, &fixed, lock);
        }
    }

    #[test]
    fn current_receipt_lock_and_universe_skew_are_clean_misses() {
        let limits = Limits::standard();
        let project = Project::new("receipt-skew");
        let request = request(&project, false, &limits);
        let catalog = universe::Catalog::new(definition(&limits));
        let manifest = render_manifest(&request, None, &limits).expect("manifest renders");
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
            let (selected, _) = select_before_command(&request, &catalog, &manifest, &limits);
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
    use std::path::Path;

    use crate::extract::{ManifestSpan, Policy};

    /// A target one unit under an exact total refuses nothing; the
    /// account records the axis with that target and a high water equal to
    /// the exact total the exercise charged.
    fn assert_exceeded(
        record: crate::exceedance::Record,
        name: &str,
        target: usize,
        high_water: usize,
    ) {
        let axis = record
            .axis(name)
            .unwrap_or_else(|| panic!("{name} records the lowered target"));
        assert_eq!(axis.target, target as u64);
        assert_eq!(axis.high_water, high_water as u64);
        assert!(axis.first_requested > axis.target);
        assert!(axis.first_requested <= axis.high_water);
    }

    /// Accounting targets are configurable by name, validated at load,
    /// and may never raise a physical limit; `depth` and every physical limit
    /// are not configurable.
    #[test]
    fn configured_accounting_targets_are_validated_and_never_raise_physical_limits() {
        use super::ConfigureError;
        let standard = super::Limits::standard();
        let mut configured = standard;
        configured.configure("graph-path-checks", 70_000).unwrap();
        configured.configure("request-bytes", 512 * 1024).unwrap();
        configured
            .configure("graph-decode-bytes", 64 * 1024 * 1024)
            .unwrap();
        configured.configure("items", 3_000_000).unwrap();
        assert_eq!(configured.graph_path_checks, 70_000);
        assert_eq!(configured.manifest_bytes, 512 * 1024);
        assert_eq!(configured.graph_decode_bytes, 64 * 1024 * 1024);
        assert_eq!(configured.items, 3_000_000);
        assert_eq!(configured.graph_targets().paths, 70_000);
        assert_eq!(configured.graph_targets().decoded, 64 * 1024 * 1024);
        assert!(configured.is_configured());
        assert!(!standard.is_configured());
        assert_eq!(
            standard.graph_targets(),
            crate::node::GraphTargets::STANDARD
        );
        for key in super::Limits::TARGET_FIELDS {
            super::Limits::standard().configure(key, 1).unwrap();
        }

        let mut refused = standard;
        assert!(matches!(
            refused.configure("depth", 512),
            Err(ConfigureError::UnknownTarget(key)) if &*key == "depth"
        ));
        assert!(matches!(
            refused.configure("stdout-bytes", 1),
            Err(ConfigureError::UnknownTarget(_))
        ));
        assert!(matches!(
            refused.configure("graph-path-checks", 0),
            Err(ConfigureError::Zero("graph-path-checks"))
        ));
        // The JSON target is compared against the physical capture bound,
        // never against its own default. Exactly the bound loads; one byte more
        // is refused at load, and neither moves the physical slot.
        let mut capped = standard;
        capped
            .configure("json-bytes", crate::process::CAPTURE_BYTES as u64)
            .expect("a JSON target equal to the physical capture bound loads");
        assert_eq!(capped.json_bytes, crate::process::CAPTURE_BYTES);
        assert_eq!(capped.process_limits(), standard.process_limits());
        assert!(matches!(
            refused.configure("json-bytes", crate::process::CAPTURE_BYTES as u64 + 1),
            Err(ConfigureError::AbovePhysicalCeiling {
                field: "json-bytes",
                value,
                ceiling,
                physical: "child stdout capture",
            }) if ceiling == crate::process::CAPTURE_BYTES as u64
                && value == crate::process::CAPTURE_BYTES as u64 + 1
        ));
        // Every manifest-class read binds physically to the 1 MiB
        // tail constant, so a manifest/request byte target above it would be
        // inert and is refused at load under either key. Exactly the bound
        // loads; one byte more is refused; the physical slot never moves.
        for key in ["manifest-bytes", "request-bytes"] {
            let mut capped = standard;
            capped
                .configure(key, crate::process::TAIL_BYTES as u64)
                .unwrap_or_else(|_| panic!("{key} equal to the manifest-class read bound loads"));
            assert_eq!(capped.manifest_bytes, crate::process::TAIL_BYTES);
            assert_eq!(capped.process_limits(), standard.process_limits());
            assert!(
                matches!(
                    refused.configure(key, crate::process::TAIL_BYTES as u64 + 1),
                    Err(ConfigureError::AbovePhysicalCeiling {
                        field: "manifest-bytes",
                        value,
                        ceiling,
                        physical: "manifest-class read",
                    }) if ceiling == crate::process::TAIL_BYTES as u64
                        && value == crate::process::TAIL_BYTES as u64 + 1
                ),
                "{key} one byte above the manifest-class read bound is refused"
            );
        }
        assert!(matches!(
            refused.configure("canonical-bytes", crate::node::MAX_CONTENT_BYTES as u64 + 1),
            Err(ConfigureError::AbovePhysicalCeiling {
                field: "canonical-bytes",
                ceiling,
                ..
            }) if ceiling == crate::node::MAX_CONTENT_BYTES as u64
        ));
        assert!(matches!(
            refused.configure(
                "input-proof-bytes",
                crate::node::MAX_CONTENT_BYTES as u64 + 1
            ),
            Err(ConfigureError::AbovePhysicalCeiling { .. })
        ));
        assert_eq!(refused, standard, "a refused key changes nothing");
        assert!(ConfigureError::AbovePhysicalCeiling {
            field: "json-bytes",
            value: 1,
            ceiling: 0,
            physical: "child stdout capture",
        }
        .to_string()
        .contains("may never raise a physical limit"));
        // The store payload ceiling is decoupled from the configurable target.
        let mut canonical = standard;
        canonical.configure("canonical-bytes", 4096).unwrap();
        assert_eq!(
            canonical.node_canonical_bytes(),
            crate::node::MAX_CONTENT_BYTES
        );
        assert_eq!(
            standard.node_canonical_bytes(),
            crate::node::MAX_CONTENT_BYTES
        );
        assert_eq!(standard.node_output_bytes(), crate::node::MAX_CONTENT_BYTES);
        assert_eq!(
            canonical.node_output_bytes(),
            crate::node::MAX_CONTENT_BYTES
        );
        assert_eq!(canonical.process_limits(), standard.process_limits());
        // The node input bound is decoupled from the configurable input-proof
        // target the same way: a configured target never moves it.
        let mut proof = standard;
        proof.configure("input-proof-bytes", 4096).unwrap();
        assert_eq!(proof.input_proof_bytes, 4096);
        assert_eq!(proof.node_input_bytes(), super::NODE_INPUT_BYTES);
        assert_eq!(standard.node_input_bytes(), super::NODE_INPUT_BYTES);
        // The physical node input bound keeps its pre-series 512 MiB value; a
        // target configured up to the store payload ceiling never raises it.
        assert_eq!(super::NODE_INPUT_BYTES, 536_870_912);
        let mut raised = standard;
        raised
            .configure("input-proof-bytes", crate::node::MAX_CONTENT_BYTES as u64)
            .unwrap();
        assert_eq!(raised.node_input_bytes(), 536_870_912);
        assert_eq!(super::CANONICAL_BYTES, crate::node::MAX_CONTENT_BYTES);
    }

    /// `with_p2_bounds` applies the same load rule as `configure`: a
    /// JSON target at the physical capture bound loads, one byte above is
    /// refused, and the physical slot never moves.
    #[test]
    fn p2_bounds_apply_the_capture_load_rule() {
        let standard = super::Limits::standard();
        let exact = standard
            .with_p2_bounds(crate::process::CAPTURE_BYTES, 7, 8)
            .expect("a JSON target equal to the physical capture bound loads");
        assert_eq!(exact.json_bytes, crate::process::CAPTURE_BYTES);
        assert_eq!(
            exact.classified_contract_bytes,
            crate::process::CAPTURE_BYTES
        );
        assert_eq!(
            (
                exact.items,
                exact.classified_contract_items,
                exact.string_bytes
            ),
            (7, 7, 8)
        );
        assert_eq!(exact.process_limits(), standard.process_limits());
        assert!(matches!(
            standard.with_p2_bounds(crate::process::CAPTURE_BYTES + 1, 7, 8),
            Err(super::ConfigureError::AbovePhysicalCeiling {
                field: "json-bytes",
                value,
                ceiling,
                physical: "child stdout capture",
            }) if value == crate::process::CAPTURE_BYTES as u64 + 1
                && ceiling == crate::process::CAPTURE_BYTES as u64
        ));
    }

    /// Configured targets are explicit proof inputs. The limits context
    /// is versioned `/v2`, and a configured profile digests differently from
    /// the standard one, so cache identity separates by target.
    #[test]
    fn limits_context_digest_is_v2_and_separates_configured_targets() {
        fn digest(limits: &super::Limits) -> [u8; 32] {
            let mut digest = <sha2::Sha256 as sha2::Digest>::new();
            limits.bind_node_context(&mut digest);
            sha2::Digest::finalize(digest).into()
        }
        let standard = super::Limits::standard();
        assert_eq!(digest(&standard), digest(&super::Limits::standard()));
        let mut configured = standard;
        configured.configure("graph-path-checks", 70_000).unwrap();
        assert_ne!(digest(&standard), digest(&configured));
        let mut other = standard;
        other.configure("graph-path-checks", 70_001).unwrap();
        assert_ne!(digest(&configured), digest(&other));
        // The JSON accounting target and the physical capture constants
        // are both digest inputs, under the unchanged /v2 input shape.
        let mut target = standard;
        target.configure("json-bytes", 16 * 1024 * 1024).unwrap();
        assert_ne!(digest(&standard), digest(&target));
        let physical = super::Limits {
            process: crate::process::Limits::fixed(
                crate::process::CAPTURE_BYTES,
                crate::process::TAIL_BYTES + 1,
            ),
            ..standard
        };
        assert_ne!(digest(&standard), digest(&physical));
        let mut v1 = <sha2::Sha256 as sha2::Digest>::new();
        sha2::Digest::update(&mut v1, b"rawr.import.cargo.limits/v1");
        let mut v2 = <sha2::Sha256 as sha2::Digest>::new();
        sha2::Digest::update(&mut v2, b"rawr.import.cargo.limits/v2");
        assert_ne!(
            sha2::Digest::finalize(v1).as_slice(),
            sha2::Digest::finalize(v2).as_slice()
        );
    }

    /// The request account is an optimization target. Crossing it keeps
    /// charging cumulatively, records the first exceeding axis, target and
    /// requested amount with a truthful high water, returns `Ok`, and is
    /// published for exactly one warning; checked-add overflow stays terminal
    /// as an arithmetic failure, never `Error::Limit`.
    #[test]
    fn request_account_exceedance_records_and_continues_while_overflow_stays_terminal() {
        let mut limits = super::Limits::standard();
        limits.configure("request-bytes", 16).unwrap();
        limits.configure("items", 3).unwrap();
        limits.configure("string-bytes", 4).unwrap();
        let mut allocation = super::RequestAllocation::new(&limits);
        assert!(!allocation.exceedances().is_exceeded());
        allocation.retained(1, 16).unwrap();
        assert!(
            !allocation.exceedances().is_exceeded(),
            "exact fit is not an exceedance"
        );
        allocation.bytes(1).unwrap();
        let first = allocation.exceedances().first().copied().unwrap();
        assert_eq!(first.name, super::REQUEST_BYTES_RESOURCE);
        assert_eq!(
            (first.target, first.first_requested, first.high_water),
            (16, 17, 17)
        );
        allocation.retained(3, 100).unwrap();
        allocation.string("abcdefgh").unwrap();
        assert_eq!((allocation.items, allocation.bytes), (5, 125));
        let record = allocation.exceedances();
        assert_eq!(record.first().unwrap().name, super::REQUEST_BYTES_RESOURCE);
        assert_eq!(record.first().unwrap().high_water, 125);
        assert_eq!(
            record
                .axis(super::REQUEST_ITEMS_RESOURCE)
                .unwrap()
                .high_water,
            5
        );
        assert_eq!(
            record
                .axis(super::REQUEST_STRING_RESOURCE)
                .unwrap()
                .high_water,
            8
        );
        // The account's own record is the proof; the shared fixed-capacity
        // process registry is reporting state and is not asserted here.
        assert_eq!(
            crate::exceedance::warnings(
                &[crate::exceedance::Exceedance {
                    account: crate::exceedance::Account::Request,
                    id: allocation.id(),
                    record,
                }],
                0
            )
            .len(),
            1,
            "one warning per account"
        );
        // Checked-add overflow is an arithmetic failure, not Error::Limit.
        let error = allocation.bytes(usize::MAX).unwrap_err();
        assert!(matches!(
            &error,
            crate::Error::Node { operation, source }
                if *operation == super::REQUEST_BYTES_RESOURCE
                    && matches!(source.as_ref(), crate::node::Error::Allocation)
        ));
        assert!(!matches!(error, crate::Error::Limit { .. }));
        // Transfer merges the sub-account's exceedances without reset or refund.
        let mut parent = super::RequestAllocation::new(&limits);
        parent.transfer(allocation).unwrap();
        assert_eq!((parent.items, parent.bytes), (5, 125));
        assert!(parent
            .exceedances()
            .axis(super::REQUEST_STRING_RESOURCE)
            .is_some());
        assert!(parent
            .transfer(super::RequestAllocation::new(&super::Limits::standard()))
            .is_err());
    }

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
        // The JSON accounting target and the physical capture bounds are
        // separate numbers from separate modules.
        assert_eq!(standard.json_bytes, super::JSON_BYTES);
        assert_eq!(standard.json_bytes, 33_554_432);
        assert_eq!(standard.process, crate::process::Limits::capture());
        assert_eq!(standard.process.stdout(), crate::process::CAPTURE_BYTES);
        assert_eq!(standard.process.stdout(), 268_435_456);
        assert_eq!(standard.process.stderr(), crate::process::TAIL_BYTES);
        assert_eq!(standard.process.stderr(), 1_048_576);
        assert_eq!(standard.containers, 1_500_000);

        let mut normal = super::RequestAllocation::new(&standard);
        normal
            .items(super::ITEMS)
            .expect("the exact normal request boundary remains accepted");
        assert!(!normal.exceedances().is_exceeded());
        normal
            .items(1)
            .expect("one item past the normal target still charges");
        assert_exceeded(
            normal.exceedances(),
            super::REQUEST_ITEMS_RESOURCE,
            super::ITEMS,
            super::ITEMS + 1,
        );
    }

    #[test]
    fn request_allocation_accepts_exact_caps_and_records_plus_one_without_refusal() {
        let mut limits = super::Limits::standard();
        limits.items = 1;
        limits.manifest_bytes = 3;
        limits.string_bytes = 3;
        let mut allocation = super::RequestAllocation::new(&limits);
        allocation
            .string("abc")
            .expect("exact item and byte caps seal");
        assert!(!allocation.exceedances().is_exceeded());
        let retained = (allocation.items, allocation.bytes);
        // One past the item and byte targets still charges in full; the
        // string itself stays under its own target and records nothing.
        allocation
            .string("x")
            .expect("one past the caps still charges");
        assert_eq!(
            (allocation.items, allocation.bytes),
            (retained.0 + 1, retained.1 + 1)
        );
        let record = allocation.exceedances();
        assert_eq!(record.first().unwrap().name, super::REQUEST_ITEMS_RESOURCE);
        assert_exceeded(record, super::REQUEST_ITEMS_RESOURCE, 1, 2);
        assert_exceeded(record, super::REQUEST_BYTES_RESOURCE, 3, 4);
        assert!(record.axis(super::REQUEST_STRING_RESOURCE).is_none());

        // The exceeding account transfers with its charges and its record.
        let mut destination = super::RequestAllocation::new(&limits);
        destination
            .transfer_with_items(allocation, 1)
            .expect("the exceeding account still transfers");
        assert_eq!((destination.items, destination.bytes), (3, 4));
        assert_exceeded(
            destination.exceedances(),
            super::REQUEST_ITEMS_RESOURCE,
            1,
            3,
        );
        assert_exceeded(
            destination.exceedances(),
            super::REQUEST_BYTES_RESOURCE,
            3,
            4,
        );
    }

    #[test]
    fn authority_render_and_internal_profile_facts_charge_before_ownership() {
        let authority = "[profile.dev]\nopt-level = 1\n";
        let exercise = |items, bytes| {
            let mut limits = super::Limits::standard();
            limits.items = items;
            limits.manifest_bytes = bytes;
            let mut allocation = super::RequestAllocation::new(&limits);
            allocation.string("profile")?;
            let rendered = allocation.formatted(format_args!(
                "{authority}[profile.dev.package.\"path+file:///rawr#rawr@1.0.0\"]\ndebug = 0\n"
            ))?;
            Ok::<_, crate::Error>((allocation, rendered))
        };
        let (usage, rendered) = exercise(usize::MAX, usize::MAX)
            .expect("unbounded authority render reveals exact counters");
        let (exact, exact_rendered) = exercise(usage.items, usage.bytes)
            .expect("exact authority/internal render boundary seals");
        assert_eq!(rendered, exact_rendered);
        assert_eq!((usage.items, usage.bytes), (exact.items, exact.bytes));
        assert!(!exact.exceedances().is_exceeded());
        // One unit under the exact total still renders the same text and
        // records the lowered axis with the exact total as its high water.
        let (item_floor, item_floor_rendered) =
            exercise(usage.items.saturating_sub(1), usage.bytes)
                .expect("one item under the exact total still renders");
        assert_eq!(item_floor_rendered, rendered);
        assert_exceeded(
            item_floor.exceedances(),
            super::REQUEST_ITEMS_RESOURCE,
            usage.items - 1,
            usage.items,
        );
        let (byte_floor, byte_floor_rendered) =
            exercise(usage.items, usage.bytes.saturating_sub(1))
                .expect("one byte under the exact total still renders");
        assert_eq!(byte_floor_rendered, rendered);
        assert_exceeded(
            byte_floor.exceedances(),
            super::REQUEST_BYTES_RESOURCE,
            usage.bytes - 1,
            usage.bytes,
        );
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
        let limits = super::Limits::standard();
        let mut allocation = super::RequestAllocation::new(&limits);

        let mut bare = super::Authority::load(None, &mut allocation).expect("generated authority");
        bare.inherit_patches(rawr, needed, &mut allocation)
            .expect("patches merge");
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

        let mut own = super::Authority::load(None, &mut allocation).expect("generated authority");
        own.retained.insert(
            "patch".into(),
            "[patch.crates-io]\ncosmic-text = { path = \"/own\" }\n"
                .parse::<toml_edit::DocumentMut>()
                .expect("project patch parses")["patch"]
                .clone(),
        );
        own.inherit_patches(rawr, needed, &mut allocation)
            .expect("patches merge");
        let own = own.retained["patch"]["crates-io"]["cosmic-text"]
            .get("path")
            .and_then(|item| item.as_str());
        assert_eq!(own, Some("/own"));
    }

    #[test]
    fn maximal_authority_evidence_uses_exact_caps_before_final_ownership() {
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
        let exercise = |limits: &super::Limits| {
            let mut allocation = super::RequestAllocation::new(limits);
            super::OWNED_AUTHORITIES.with(|owned| owned.set(0));
            let authority = super::Authority::load(Some(&manifest), &mut allocation)?;
            Ok::<_, crate::Error>((allocation, authority))
        };
        let standard = super::Limits::standard();
        let (usage, _) = exercise(&standard).expect("standard authority ledger measures");
        let mut exact = standard;
        exact.items = usage.items;
        exact.manifest_bytes = usage.bytes;
        let (exact_usage, _) = exercise(&exact).expect("exact authority ledger succeeds");
        assert_eq!(super::OWNED_AUTHORITIES.with(std::cell::Cell::get), 1);
        assert!(!exact_usage.exceedances().is_exceeded());
        // One unit under the exact ledger refuses nothing and still owns
        // the authority; the account records the lowered axis at the exact total.
        let mut item_floor = exact;
        item_floor.items = item_floor.items.saturating_sub(1);
        let (item_usage, _) =
            exercise(&item_floor).expect("one item under the exact ledger still loads");
        assert_eq!(super::OWNED_AUTHORITIES.with(std::cell::Cell::get), 1);
        assert_exceeded(
            item_usage.exceedances(),
            super::REQUEST_ITEMS_RESOURCE,
            item_floor.items,
            usage.items,
        );
        let mut byte_floor = exact;
        byte_floor.manifest_bytes = byte_floor.manifest_bytes.saturating_sub(1);
        let (byte_usage, _) =
            exercise(&byte_floor).expect("one byte under the exact ledger still loads");
        assert_eq!(super::OWNED_AUTHORITIES.with(std::cell::Cell::get), 1);
        assert_exceeded(
            byte_usage.exceedances(),
            super::REQUEST_BYTES_RESOURCE,
            byte_floor.manifest_bytes,
            usage.bytes,
        );
        fs::remove_dir_all(root).expect("authority fixture is removed");
    }

    #[test]
    fn policy_deep_config_authority_and_internal_share_one_request_lineage() {
        let root = std::env::temp_dir().join(format!(
            "rawr-combined-request-ledger-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        fs::create_dir(&root).expect("combined request fixture root is created");
        let manifest = root.join("Cargo.toml");
        fs::write(
            &manifest,
            "[workspace]\nresolver = \"3\"\n[profile.dev]\nopt-level = 1\n",
        )
        .expect("combined authority manifest is written");
        fs::write(root.join("Cargo.lock"), b"version = 4\n")
            .expect("combined authority lock is written");
        let manifest = fs::canonicalize(manifest).expect("combined authority canonicalizes");

        let exercise = |limits: &super::Limits| {
            let span = ManifestSpan::new("Cargo.toml", 1, 2, 1, 1, 1, 2, limits)?;
            let policy = Policy::manifest(limits)
                .exclusion("catalog", ["api", "Thing"], "not public", span)?
                .seal();
            let mut allocation = super::RequestAllocation::new(limits);
            allocation.transfer_policy(&policy)?;
            for candidate in [
                Path::new("/deep/.cargo/config.toml"),
                Path::new("/deep/a/.cargo/config.toml"),
                Path::new("/deep/a/b/.cargo/config.toml"),
                Path::new("/deep/a/b/c/.cargo/config.toml"),
            ] {
                allocation.items(1)?;
                allocation.path(candidate)?;
                allocation.retained(8, candidate.as_os_str().len())?;
            }
            let authority = super::Authority::load(Some(&manifest), &mut allocation)?;
            let internal =
                super::internal::Seed::transform(&crate::host::root()?, &mut allocation)?;
            Ok::<_, crate::Error>((allocation, policy, authority, internal))
        };

        let standard = super::Limits::standard();
        let (usage, ..) = exercise(&standard).expect("the combined request lineage measures");
        let mut exact = standard;
        exact.items = usage.items;
        exact.manifest_bytes = usage.bytes;
        let (exact_usage, ..) = exercise(&exact).expect("the exact combined request lineage seals");
        assert!(!exact_usage.exceedances().is_exceeded());
        // One unit under the combined lineage refuses nothing; the one
        // request account records the lowered axis at the exact total.
        let mut item_floor = exact;
        item_floor.items = item_floor.items.saturating_sub(1);
        let (item_usage, ..) =
            exercise(&item_floor).expect("one item under the lineage still seals");
        assert_exceeded(
            item_usage.exceedances(),
            super::REQUEST_ITEMS_RESOURCE,
            item_floor.items,
            usage.items,
        );
        let mut byte_floor = exact;
        byte_floor.manifest_bytes = byte_floor.manifest_bytes.saturating_sub(1);
        let (byte_usage, ..) =
            exercise(&byte_floor).expect("one byte under the lineage still seals");
        assert_exceeded(
            byte_usage.exceedances(),
            super::REQUEST_BYTES_RESOURCE,
            byte_floor.manifest_bytes,
            usage.bytes,
        );
        fs::remove_dir_all(root).expect("combined request fixture is retired");
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
        assert!(matches!(
            error,
            Error::Owner {
                state: "cache busy"
            }
        ));
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
                assert!(matches!(
                    acquire_process_owner(),
                    Err(Error::Owner {
                        state: "process busy"
                    })
                ));
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

/// The Cargo JSON accounting target and the physical child capture bound
/// are separate numbers with separate consequences at the `run_cargo` boundary.
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

    /// A document one byte past the configured JSON target: a JSON array whose
    /// elements stay far inside every other accounting target.
    fn document(bytes: usize) -> Vec<u8> {
        assert!(bytes >= 3);
        let mut document = Vec::with_capacity(bytes);
        document.extend_from_slice(b"[1");
        while document.len() + 2 < bytes {
            document.extend_from_slice(b",1");
        }
        if document.len() + 1 < bytes {
            // One byte short: the last element becomes the two-digit `10`.
            document.push(b'0');
        }
        document.push(b']');
        assert_eq!(document.len(), bytes);
        document
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

    /// A child emitting one byte past the JSON target is captured whole and
    /// returned byte-identical with `Ok`; parsing it records the `Cargo JSON
    /// bytes` exceedance on the Cargo JSON epoch, published for exactly one
    /// warning. Nothing constructs `Error::Limit`.
    #[test]
    fn target_plus_one_is_captured_whole_and_recorded_as_an_exceedance() {
        let root = Root::new();
        let mut limits = super::Limits::standard();
        limits.configure("json-bytes", 65).unwrap();
        let emitted = document(66);
        assert_eq!(emitted.len(), limits.json_bytes + 1);
        let native = process::Native::fixture();
        let bytes = root
            .script(&limits, Reply::stdout(emitted.clone()))
            .run(|| super::run_cargo(&native.production(), &command(), &limits))
            .unwrap()
            .expect("an accounting target never refuses a captured document");
        assert_eq!(bytes.as_ref(), emitted.as_slice(), "byte-identical capture");

        let allocation = super::unit::preflight("Cargo metadata", &bytes, &limits)
            .expect("an accounting target never refuses the parse");
        let record = allocation.exceedances();
        let axis = record
            .axis("Cargo JSON bytes")
            .expect("the JSON target crossing is recorded on the Cargo JSON epoch");
        assert_eq!(
            (axis.target, axis.first_requested, axis.high_water),
            (65, 66, 66)
        );
        // The account's own record is the proof; the shared fixed-capacity
        // process registry is reporting state and is not asserted here.
        assert_eq!(
            crate::exceedance::warnings(
                &[crate::exceedance::Exceedance {
                    account: crate::exceedance::Account::CargoJson,
                    id: allocation.id(),
                    record,
                }],
                0
            )
            .len(),
            1,
            "one warning per account"
        );

        // Exactly the target is not an exceedance, and the capture is unchanged.
        let exact = document(65);
        let bytes = root
            .script(&limits, Reply::stdout(exact.clone()))
            .run(|| super::run_cargo(&native.production(), &command(), &limits))
            .unwrap()
            .unwrap();
        assert_eq!(bytes.as_ref(), exact.as_slice());
        assert!(!super::unit::preflight("Cargo metadata", &bytes, &limits)
            .unwrap()
            .exceedances()
            .is_exceeded());
    }

    /// A child emitting one byte past the physical capture bound is refused as
    /// `Error::Limit` at the capture itself, whatever the JSON target says.
    #[test]
    fn capture_plus_one_is_physically_refused() {
        let root = Root::new();
        let mut limits = super::Limits::standard();
        limits
            .configure("json-bytes", process::CAPTURE_BYTES as u64)
            .unwrap();
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
