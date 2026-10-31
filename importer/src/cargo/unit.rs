//! Canonical Cargo metadata and command-specific unit graphs.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Weak};

#[cfg(unix)]
use std::os::unix::ffi::OsStringExt as _;

use semver::VersionReq;
use serde::de::{DeserializeSeed, Error as _, MapAccess, SeqAccess, Visitor};
use serde_json::{Map, Value};
use sha2::{Digest as _, Sha256};

use super::config::{self, Selected, Source};
use super::{canonical_file, digest, Digest, Limits, Selection};
use crate::{Error, Result};

const HARNESS_PACKAGE: &str = "rawr-resolution-harness";

#[cfg(test)]
std::thread_local! {
    static OWNED_JSON_VALUES: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    static OWNED_LOCK_DOCUMENTS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    static OWNED_REACHABILITY_NODES: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

#[derive(Clone, Debug)]
pub(crate) struct Metadata {
    raw: RawDocument,
    limits: Limits,
    canonical: Box<[u8]>,
    root: usize,
    selected: BTreeMap<Box<str>, usize>,
    declared: BTreeMap<Box<str>, usize>,
    aliases: BTreeMap<Box<str>, Box<[Box<str>]>>,
    libraries: BTreeSet<Box<str>>,
    packages: BTreeMap<Arc<str>, Package>,
}

#[derive(Clone, Debug)]
pub(crate) struct BuildGraph {
    raw: RawDocument,
    _allocation: JsonAllocation,
    limits: Limits,
    canonical: Digest,
    artifacts: Box<[Output]>,
    scripts: Box<[BuildScript]>,
    #[cfg(feature = "rustc-driver")]
    extraction: Box<[Extraction]>,
}

#[derive(serde::Deserialize)]
struct OwnedUnitDocument {
    units: Vec<OwnedUnit>,
    #[serde(rename = "roots")]
    _roots: Vec<usize>,
    #[serde(rename = "version")]
    _version: u32,
}

#[derive(serde::Deserialize)]
struct OwnedUnit {
    pkg_id: String,
    target: OwnedTarget,
    profile: OwnedProfile,
    platform: Option<String>,
    mode: String,
    features: Vec<String>,
    dependencies: Vec<OwnedDependency>,
    #[serde(default)]
    is_std: Option<bool>,
}

#[derive(serde::Deserialize)]
struct OwnedTarget {
    kind: Vec<String>,
    crate_types: Vec<String>,
    name: String,
    src_path: String,
    edition: String,
}

#[derive(serde::Deserialize)]
struct OwnedProfile {
    name: String,
    opt_level: String,
    debuginfo: Value,
    debug_assertions: bool,
    overflow_checks: bool,
}

#[derive(serde::Deserialize)]
struct OwnedDependency {
    index: usize,
    extern_crate_name: Option<String>,
}

struct PreparedOutput {
    unit: usize,
    package_id: String,
    target: PathBuf,
    target_kinds: Vec<String>,
    crate_types: Vec<String>,
    crate_name: String,
    edition: String,
    features: Vec<String>,
    profile: OwnedProfile,
    platform: Option<String>,
    mode: String,
    dependencies: Vec<usize>,
    extraction_dependencies: Option<Vec<OwnedDependency>>,
}

struct PreparedGraph {
    outputs: Vec<PreparedOutput>,
    scripts: Vec<Run>,
}

#[derive(Eq, Ord, PartialEq, PartialOrd)]
struct Run {
    unit: usize,
    package_id: String,
    platform: Option<String>,
    compiler: usize,
    dependencies: Vec<usize>,
}

struct UnitMasks {
    flags: Box<[u8]>,
}

const RETAINED_UNIT: u8 = 1;
const EXTRACTION_UNIT: u8 = 2;

struct FinishedGraph {
    artifacts: Box<[Output]>,
    scripts: Box<[BuildScript]>,
    #[cfg(feature = "rustc-driver")]
    extraction: Box<[Extraction]>,
}

/// One monotonic allocation authority for a complete Cargo equality epoch.
///
/// The private ledger prevents independent metadata/unit proofs from each
/// beginning at an empty budget while an earlier proof is still retained.
#[derive(Debug)]
pub(crate) struct Allocation {
    retained: JsonAllocation,
    limits: Limits,
    epoch: Arc<()>,
    provisional: Option<Arc<()>>,
}

/// One non-forgeable, one-use checkpoint for the fresh provisional resolution
/// phase. It carries identity only; it grants no additional allocation.
pub(super) struct Provisional {
    epoch: Weak<()>,
    scope: Option<Arc<()>>,
    retained: JsonOwnership,
}

/// Exact transport bytes retained only as a post-command equality witness.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct RawDocument {
    bytes: usize,
    digest: Digest,
}

/// Exact command bytes moved into their original Cargo allocation epoch.
pub(super) struct Transport(Box<[u8]>);

impl Transport {
    pub(super) fn bytes(&self) -> &[u8] {
        &self.0
    }
}

/// One non-sysroot compiler artifact required from public Build JSON.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) struct Output {
    package_id: Box<str>,
    target: PathBuf,
    target_kinds: Box<[Box<str>]>,
    crate_types: Box<[Box<str>]>,
    crate_name: Box<str>,
    features: Box<[Box<str>]>,
    artifact_profile: ArtifactProfile,
    platform: Option<Box<str>>,
    mode: Box<str>,
    #[cfg(feature = "rustc-driver")]
    edition: Box<str>,
    #[cfg(feature = "rustc-driver")]
    profile_name: Box<str>,
    #[cfg(feature = "rustc-driver")]
    dependencies: Box<[Arc<str>]>,
    semantic_key: Arc<str>,
}

/// Exact public Cargo compiler-artifact profile projection.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) struct ArtifactProfile(Box<[u8]>);

/// One exact direct Cargo edge of a selected normal-library unit.
#[cfg(feature = "rustc-driver")]
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) struct Dependency {
    extern_alias: Option<Box<str>>,
    semantic_key: Arc<str>,
    package_id: Box<str>,
    target: PathBuf,
    target_kinds: Box<[Box<str>]>,
    crate_types: Box<[Box<str>]>,
    crate_name: Box<str>,
    features: Box<[Box<str>]>,
}

/// One exact run-custom-build unit whose execution must appear in Build JSON.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) struct BuildScript {
    package_id: Box<str>,
    platform: Option<Box<str>>,
    compiler: Arc<str>,
    semantic_key: Arc<str>,
    dependencies: Box<[Arc<str>]>,
}

/// Plan-ready identity retained from one selected normal Build unit.
#[cfg(feature = "rustc-driver")]
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) struct Extraction {
    semantic_key: Arc<str>,
    aliases: Box<[Box<str>]>,
    package_id: Box<str>,
    package_name: Box<str>,
    package_version: Box<str>,
    package_source: Box<str>,
    target: Box<str>,
    target_kinds: Box<[Box<str>]>,
    crate_types: Box<[Box<str>]>,
    crate_name: Box<str>,
    edition: Box<str>,
    profile: Box<str>,
    mode: Box<str>,
    features: Box<[Box<str>]>,
    dependencies: Box<[Dependency]>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct Package {
    id: Arc<str>,
    name: Arc<str>,
    version: Arc<str>,
    manifest: PathBuf,
    root: Arc<Path>,
    source: PackageSource,
    features: Option<Box<[Box<str>]>>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum PackageSource {
    Path {
        cargo_id: Arc<str>,
    },
    Registry {
        cargo_id: Arc<str>,
        checksum: Option<[u8; 32]>,
    },
    Git {
        cargo_id: Arc<str>,
        oid: Arc<str>,
    },
}

impl Package {
    pub(crate) fn id(&self) -> &str {
        &self.id
    }

    pub(crate) fn name(&self) -> &str {
        &self.name
    }

    pub(crate) fn version(&self) -> &str {
        &self.version
    }

    pub(crate) fn manifest(&self) -> &Path {
        &self.manifest
    }

    pub(crate) fn root(&self) -> &Path {
        &self.root
    }

    pub(crate) const fn source(&self) -> &PackageSource {
        &self.source
    }

    pub(super) fn shared_name(&self) -> Arc<str> {
        Arc::clone(&self.name)
    }

    pub(super) fn shared_version(&self) -> Arc<str> {
        Arc::clone(&self.version)
    }

    pub(super) fn shared_root(&self) -> Arc<Path> {
        Arc::clone(&self.root)
    }

    #[cfg(test)]
    pub(crate) fn fixture(
        id: &str,
        name: &str,
        version: &str,
        root: PathBuf,
        source: PackageSource,
    ) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            version: version.into(),
            manifest: root.join("Cargo.toml"),
            root: root.into(),
            source,
            features: Some(Box::default()),
        }
    }
}

impl PackageSource {
    pub(crate) fn cargo_id(&self) -> &str {
        match self {
            Self::Path { cargo_id }
            | Self::Registry { cargo_id, .. }
            | Self::Git { cargo_id, .. } => cargo_id,
        }
    }

    pub(super) fn shared_universe_source(&self) -> Option<Arc<str>> {
        match self {
            Self::Path { .. } => None,
            Self::Registry { cargo_id, .. } | Self::Git { cargo_id, .. } => {
                Some(Arc::clone(cargo_id))
            }
        }
    }
}

impl Metadata {
    pub(crate) fn workspace_manifests(&self) -> Result<impl Iterator<Item = &Path>> {
        let root = self.root_id()?;
        Ok(self.packages.iter().filter_map(move |(id, package)| {
            (id.as_ref() != root
                && matches!(
                    package.source(),
                    PackageSource::Path { .. } | PackageSource::Git { .. }
                ))
            .then_some(package.manifest())
        }))
    }
    fn package_id(&self, index: usize) -> Result<&str> {
        self.packages
            .keys()
            .nth(index)
            .map(AsRef::as_ref)
            .ok_or_else(|| schema("Cargo metadata", "retained package ordinal is absent"))
    }

    fn root_id(&self) -> Result<&str> {
        self.package_id(self.root)
    }

    #[cfg(feature = "rustc-driver")]
    fn extracts_alias(&self, alias: &str) -> bool {
        self.libraries.contains(alias)
    }

    pub(crate) fn package_identity(&self, package_id: &str) -> Option<(&str, &str, &str)> {
        self.packages.get(package_id).map(|package| {
            (
                package.name.as_ref(),
                package.version.as_ref(),
                package.source.cargo_id(),
            )
        })
    }

    pub(crate) fn package(&self, package_id: &str) -> Option<&Package> {
        self.packages.get(package_id)
    }

    pub(super) fn package_entries(&self) -> impl Iterator<Item = (&Arc<str>, &Package)> {
        self.packages.iter()
    }

    pub(super) fn package_count(&self) -> usize {
        self.packages.len()
    }

    #[cfg(test)]
    pub(crate) fn fixture_packages(packages: impl IntoIterator<Item = Package>) -> Self {
        let packages = packages
            .into_iter()
            .map(|package| (Arc::clone(&package.id), package))
            .collect();
        Self {
            raw: RawDocument::from_bytes(&[]),
            limits: Limits::standard(),
            canonical: Box::default(),
            root: 0,
            selected: BTreeMap::new(),
            declared: BTreeMap::new(),
            aliases: BTreeMap::new(),
            libraries: BTreeSet::new(),
            packages,
        }
    }

    pub(crate) fn bind_lock(
        mut self,
        bytes: &[u8],
        limits: &Limits,
        allocation: &mut Allocation,
    ) -> Result<Self> {
        allocation.require(limits)?;
        let mut next = allocation.retained;
        // The document size is an accounting target recorded on the
        // JSON epoch; refusal is reserved for the physical capture bound.
        next.exceeded("Cargo lock bytes", limits.json_bytes, bytes.len());
        // A byte is a conservative upper bound for every TOML-owned node and
        // byte.  Charge the complete DOM before `DocumentMut` can own it.
        next.nodes("Cargo lock TOML nodes", bytes.len(), limits)?;
        next.retained_bytes(
            "Cargo lock TOML bytes",
            bytes.len(),
            limits.json_bytes,
            limits,
        )?;
        let text = std::str::from_utf8(bytes).map_err(|error| Error::Schema {
            document: "Cargo lock",
            reason: error.to_string().into_boxed_str(),
        })?;
        let document = text
            .parse::<toml_edit::DocumentMut>()
            .map_err(|error| Error::Schema {
                document: "Cargo lock",
                reason: error.to_string().into_boxed_str(),
            })?;
        #[cfg(test)]
        OWNED_LOCK_DOCUMENTS.with(|owned| owned.set(owned.get().saturating_add(1)));
        let locked = document
            .get("package")
            .and_then(toml_edit::Item::as_array_of_tables)
            .ok_or_else(|| schema("Cargo lock", "package array is absent"))?;
        for package in self.packages.values_mut() {
            let PackageSource::Registry { cargo_id, checksum } = &mut package.source else {
                continue;
            };
            let mut matches = locked.iter().filter(|entry| {
                entry.get("name").and_then(toml_edit::Item::as_str) == Some(package.name.as_ref())
                    && entry.get("version").and_then(toml_edit::Item::as_str)
                        == Some(package.version.as_ref())
                    && entry.get("source").and_then(toml_edit::Item::as_str)
                        == Some(cargo_id.as_ref())
            });
            let entry = matches
                .next()
                .ok_or_else(|| schema("Cargo lock", "registry package authority is absent"))?;
            if matches.next().is_some() {
                return Err(schema(
                    "Cargo lock",
                    "registry package authority is ambiguous",
                ));
            }
            let locked = entry
                .get("checksum")
                .and_then(toml_edit::Item::as_str)
                .ok_or_else(|| schema("Cargo lock", "registry checksum is absent"))?;
            let locked = decode_checksum(locked)?;
            if checksum.is_some_and(|metadata| metadata != locked) {
                return Err(schema(
                    "Cargo metadata/lock",
                    "registry checksum authorities disagree",
                ));
            }
            *checksum = Some(locked);
        }
        allocation.retained = next;
        Ok(self)
    }

    pub(crate) fn parse(
        bytes: &[u8],
        dependencies: &[Selected],
        limits: &Limits,
        schema_version: u32,
        command_target: &Path,
        epoch: &mut Allocation,
    ) -> Result<Self> {
        Self::parse_observed(
            bytes,
            dependencies,
            limits,
            schema_version,
            command_target,
            command_target,
            epoch,
            None,
        )
        .map(|(metadata, _)| metadata)
    }

    pub(crate) fn parse_versioned(
        bytes: &[u8],
        dependencies: &[Selected],
        limits: &Limits,
        schema_version: u32,
        command_target: &Path,
        epoch: &mut Allocation,
        definition: &super::universe::Definition,
    ) -> Result<(Self, super::universe::MetadataObservation)> {
        let (metadata, observation) = Self::parse_observed(
            bytes,
            dependencies,
            limits,
            schema_version,
            command_target,
            command_target,
            epoch,
            Some(definition),
        )?;
        Ok((
            metadata,
            observation
                .ok_or_else(|| schema("Cargo metadata", "universe observation is absent"))?,
        ))
    }

    #[cfg(feature = "rustc-driver")]
    pub(crate) fn parse_in(
        bytes: &[u8],
        dependencies: &[Selected],
        limits: &Limits,
        schema_version: u32,
        paths: &super::resolution::layout::Cargo,
        epoch: &mut Allocation,
    ) -> Result<Self> {
        Self::parse_observed(
            bytes,
            dependencies,
            limits,
            schema_version,
            paths.target(),
            paths.build(),
            epoch,
            None,
        )
        .map(|(metadata, _)| metadata)
    }

    #[cfg(feature = "rustc-driver")]
    pub(crate) fn parse_versioned_in(
        bytes: &[u8],
        dependencies: &[Selected],
        limits: &Limits,
        schema_version: u32,
        paths: &super::resolution::layout::Cargo,
        epoch: &mut Allocation,
        definition: &super::universe::Definition,
    ) -> Result<(Self, super::universe::MetadataObservation)> {
        let (metadata, observation) = Self::parse_observed(
            bytes,
            dependencies,
            limits,
            schema_version,
            paths.target(),
            paths.build(),
            epoch,
            Some(definition),
        )?;
        Ok((
            metadata,
            observation
                .ok_or_else(|| schema("Cargo metadata", "universe observation is absent"))?,
        ))
    }

    #[allow(
        clippy::too_many_arguments,
        reason = "one parse optionally produces the authenticated universe observation"
    )]
    fn parse_observed(
        bytes: &[u8],
        dependencies: &[Selected],
        limits: &Limits,
        schema_version: u32,
        command_target: &Path,
        command_build: &Path,
        epoch: &mut Allocation,
        definition: Option<&super::universe::Definition>,
    ) -> Result<(Self, Option<super::universe::MetadataObservation>)> {
        epoch.require(limits)?;
        let raw = RawDocument::from_bytes(bytes);
        let mut allocation = preflight_from("Cargo metadata", bytes, limits, epoch.retained)?;
        let value = parse_owned_json("Cargo metadata", bytes)?;
        let document_ownership = JsonOwnership::tree(&value)?;
        let object = value
            .as_object()
            .ok_or_else(|| schema("Cargo metadata", "root is not an object"))?;
        exact_schema(object, "Cargo metadata", schema_version)?;
        for key in [
            "packages",
            "workspace_members",
            "workspace_default_members",
            "resolve",
            "workspace_root",
            "target_directory",
            "build_directory",
            "version",
            "metadata",
        ] {
            if !object.contains_key(key) {
                return Err(schema("Cargo metadata", &format!("missing `{key}`")));
            }
        }
        validate_command_directory_pair(object, command_target, command_build)?;

        let packages = array(object, "packages", "Cargo metadata")?;
        allocation.exceeded("Cargo packages", limits.packages, packages.len());
        let resolve = object
            .get("resolve")
            .and_then(Value::as_object)
            .ok_or_else(|| schema("Cargo metadata", "missing resolved graph"))?;
        let root_value = string(resolve, "root", "Cargo metadata")?;
        let root_package = packages
            .iter()
            .find(|package| value_string(package, "id") == Some(root_value))
            .ok_or_else(|| schema("Cargo metadata", "resolved root package is absent"))?;
        if value_string(root_package, "name") != Some(HARNESS_PACKAGE) {
            return Err(schema(
                "Cargo metadata",
                "resolved root is not the Rawr-owned harness",
            ));
        }

        let declarations = root_package
            .get("dependencies")
            .and_then(Value::as_array)
            .ok_or_else(|| schema("Cargo metadata", "root dependency declarations are absent"))?;
        allocation.nodes(
            "Cargo metadata dependency group nodes",
            dependencies.len().saturating_mul(2),
            limits,
        )?;
        let mut groups = Vec::<Vec<&Selected>>::new();
        groups
            .try_reserve_exact(dependencies.len())
            .map_err(|source| Error::Allocation {
                operation: "Cargo metadata",
                source,
            })?;
        for dependency in dependencies {
            match groups
                .iter_mut()
                .find(|group| group[0].shares_cargo_unit(dependency))
            {
                Some(group) => {
                    group.try_reserve(1).map_err(|source| Error::Allocation {
                        operation: "Cargo metadata",
                        source,
                    })?;
                    group.push(dependency);
                }
                None => {
                    let mut group = Vec::new();
                    group
                        .try_reserve_exact(1)
                        .map_err(|source| Error::Allocation {
                            operation: "Cargo metadata",
                            source,
                        })?;
                    group.push(dependency);
                    groups.push(group);
                }
            }
        }
        if declarations.len() != groups.len() {
            return Err(Error::ResolutionDrift {
                command: "metadata",
                field: "selected dependency declarations",
            });
        }
        let nodes = array(resolve, "nodes", "Cargo metadata")?;
        allocation.exceeded("Cargo resolve nodes", limits.packages, nodes.len());
        let resolve_edges = nodes.iter().try_fold(0_usize, |edges, node| {
            let dependencies = node
                .get("deps")
                .and_then(Value::as_array)
                .ok_or_else(|| schema("Cargo metadata", "resolve-node dependencies are absent"))?;
            Ok::<_, Error>(edges.saturating_add(dependencies.len()))
        })?;
        allocation.exceeded("Cargo resolve edges", limits.edges, resolve_edges);
        let root_node = nodes
            .iter()
            .find(|node| value_string(node, "id") == Some(root_value))
            .ok_or_else(|| schema("Cargo metadata", "resolved root node is absent"))?;
        let node_dependencies = root_node
            .get("deps")
            .and_then(Value::as_array)
            .ok_or_else(|| schema("Cargo metadata", "root resolved dependencies are absent"))?;
        if node_dependencies.len() != groups.len() {
            return Err(Error::ResolutionDrift {
                command: "metadata",
                field: "selected resolved dependencies",
            });
        }

        // Validate source selection against the original command document,
        // before canonicalization changes any spelling or array order.
        for group in &groups {
            let dependency = group[0];
            validate_declaration(dependency, declarations, &mut allocation, limits)?;
            let mut matches = node_dependencies
                .iter()
                .filter(|candidate| value_string(candidate, "name") == Some(dependency.alias()));
            let resolved = matches.next().ok_or(Error::ResolutionDrift {
                command: "metadata",
                field: "dependency alias resolution",
            })?;
            if matches.next().is_some() {
                return Err(Error::ResolutionDrift {
                    command: "metadata",
                    field: "dependency alias resolution",
                });
            }
            let package_id = resolved
                .get("pkg")
                .and_then(Value::as_str)
                .ok_or_else(|| schema("Cargo metadata", "resolved dependency lacks package ID"))?;
            let package = packages
                .iter()
                .find(|package| value_string(package, "id") == Some(package_id))
                .ok_or_else(|| schema("Cargo metadata", "selected package is absent"))?;
            validate_package(dependency, package)?;
            for dependency in group {
                validate_package(dependency, package)?;
            }
        }

        let mut document = Budgeted::from_retained(value, document_ownership);
        let canonical = canonical_metadata_bytes_budgeted(&mut document, &mut allocation, limits)?;
        let (canonical, _) = canonical.into_parts();
        let records = compact_metadata_packages(
            &mut document,
            definition.is_some(),
            &mut allocation,
            limits,
        )?;
        let object = document
            .value()
            .as_object()
            .ok_or_else(|| schema("Cargo metadata", "root is not an object"))?;
        let packages = array(object, "packages", "Cargo metadata")?;
        let resolve = object
            .get("resolve")
            .and_then(Value::as_object)
            .ok_or_else(|| schema("Cargo metadata", "missing resolved graph"))?;
        let root_value = string(resolve, "root", "Cargo metadata")?;
        let nodes = array(resolve, "nodes", "Cargo metadata")?;
        let root_node = nodes
            .iter()
            .find(|node| value_string(node, "id") == Some(root_value))
            .ok_or_else(|| schema("Cargo metadata", "resolved root node is absent"))?;
        let node_dependencies = root_node
            .get("deps")
            .and_then(Value::as_array)
            .ok_or_else(|| schema("Cargo metadata", "root resolved dependencies are absent"))?;

        let mut selected = BTreeMap::new();
        let mut declared = BTreeMap::new();
        let mut aliases = BTreeMap::new();
        let library_count = dependencies
            .iter()
            .filter(|dependency| dependency.roles().contains(config::Role::Library))
            .count();
        allocation.member_nodes("Cargo metadata library nodes", library_count, limits)?;
        let mut libraries = BTreeSet::new();
        for dependency in dependencies
            .iter()
            .filter(|dependency| dependency.roles().contains(config::Role::Library))
        {
            allocation.retained_string(
                "Cargo metadata retained string bytes",
                dependency.alias().len(),
                limits,
            )?;
            libraries.insert(Box::<str>::from(dependency.alias()));
        }
        allocation.member_nodes("Cargo metadata package-map nodes", packages.len(), limits)?;
        let mut all_packages = BTreeMap::new();
        for package in packages {
            let (package_id, package) = parse_package_budgeted(package, &mut allocation, limits)?;
            if all_packages.insert(package_id, package).is_some() {
                return Err(schema("Cargo metadata", "duplicate Cargo PackageId"));
            }
        }
        if all_packages.len() != packages.len() {
            return Err(schema("Cargo metadata", "duplicate Cargo PackageId"));
        }
        for node in nodes {
            let package_id = value_string(node, "id")
                .ok_or_else(|| schema("Cargo metadata", "resolve-node package ID is absent"))?;
            let feature_values = node
                .get("features")
                .and_then(Value::as_array)
                .ok_or_else(|| schema("Cargo metadata", "resolved features are absent"))?;
            allocation.nodes("Cargo metadata feature nodes", feature_values.len(), limits)?;
            let mut features = Vec::new();
            features
                .try_reserve_exact(feature_values.len())
                .map_err(|source| Error::Allocation {
                    operation: "Cargo metadata",
                    source,
                })?;
            for feature in feature_values {
                let feature = feature
                    .as_str()
                    .ok_or_else(|| schema("Cargo metadata", "resolved feature is not a string"))?;
                allocation.retained_string(
                    "Cargo metadata retained string bytes",
                    feature.len(),
                    limits,
                )?;
                features.push(Box::from(feature));
            }
            features.sort_unstable();
            if features.windows(2).any(|pair| pair[0] == pair[1]) {
                return Err(schema("Cargo metadata", "duplicate resolved feature"));
            }
            let package = all_packages
                .get_mut(package_id)
                .ok_or_else(|| schema("Cargo metadata", "resolve-node package is absent"))?;
            if package
                .features
                .replace(features.into_boxed_slice())
                .is_some()
            {
                return Err(schema("Cargo metadata", "duplicate resolve-node PackageId"));
            }
        }
        if all_packages
            .values()
            .any(|package| package.features.is_none())
        {
            return Err(schema("Cargo metadata", "resolve-node PackageId is absent"));
        }
        let root = all_packages
            .keys()
            .position(|package_id| package_id.as_ref() == root_value)
            .ok_or_else(|| schema("Cargo metadata", "retained root package is absent"))?;
        for group in groups {
            let dependency = group[0];
            let mut matches = node_dependencies
                .iter()
                .filter(|candidate| value_string(candidate, "name") == Some(dependency.alias()));
            let Some(resolved) = matches.next() else {
                return Err(Error::ResolutionDrift {
                    command: "metadata",
                    field: "dependency alias resolution",
                });
            };
            if matches.next().is_some() {
                return Err(Error::ResolutionDrift {
                    command: "metadata",
                    field: "dependency alias resolution",
                });
            }
            let package_id = resolved
                .get("pkg")
                .and_then(Value::as_str)
                .ok_or_else(|| schema("Cargo metadata", "resolved dependency lacks package ID"))?;
            if all_packages
                .get(package_id)
                .is_none_or(|package| package.features.is_none())
            {
                return Err(schema("Cargo metadata", "selected resolve node is absent"));
            }
            let package_ordinal = all_packages
                .keys()
                .position(|candidate| candidate.as_ref() == package_id)
                .ok_or_else(|| schema("Cargo metadata", "selected package ordinal is absent"))?;
            if declared
                .insert(
                    {
                        allocation.member_nodes("Cargo metadata declared-map nodes", 1, limits)?;
                        allocation.retained_string(
                            "Cargo metadata retained string bytes",
                            dependency.alias().len(),
                            limits,
                        )?;
                        dependency.alias().into()
                    },
                    package_ordinal,
                )
                .is_some()
            {
                return Err(schema("Cargo metadata", "duplicate declared alias"));
            }
            allocation.nodes("Cargo metadata alias-group nodes", group.len(), limits)?;
            let mut group_aliases = Vec::new();
            group_aliases
                .try_reserve_exact(group.len())
                .map_err(|source| Error::Allocation {
                    operation: "Cargo metadata",
                    source,
                })?;
            for selected_dependency in group {
                allocation.member_nodes("Cargo metadata selected-map nodes", 1, limits)?;
                allocation.retained_string(
                    "Cargo metadata retained string bytes",
                    selected_dependency.alias().len(),
                    limits,
                )?;
                if selected
                    .insert(selected_dependency.alias().into(), package_ordinal)
                    .is_some()
                {
                    return Err(schema("Cargo metadata", "duplicate selected alias"));
                }
                allocation.retained_string(
                    "Cargo metadata retained string bytes",
                    selected_dependency.alias().len(),
                    limits,
                )?;
                group_aliases.push(selected_dependency.alias().into());
            }
            allocation.member_nodes("Cargo metadata alias-map nodes", 1, limits)?;
            allocation.retained_string(
                "Cargo metadata retained string bytes",
                dependency.alias().len(),
                limits,
            )?;
            aliases.insert(dependency.alias().into(), group_aliases.into_boxed_slice());
        }

        let parsed = Self {
            raw,
            limits: *limits,
            canonical,
            root,
            selected,
            declared,
            aliases,
            libraries,
            packages: all_packages,
        };
        let observation = match definition {
            Some(definition) => Some(definition.observe_metadata_records(
                document,
                records.ok_or_else(|| {
                    schema("Cargo metadata", "complete package records are absent")
                })?,
                Some(&parsed),
                limits,
                &mut allocation,
            )?),
            None => {
                document.drop_from(&mut allocation)?;
                None
            }
        };
        epoch.retained = allocation;
        Ok((parsed, observation))
    }

    pub(crate) fn require_exact_raw(&self, bytes: &[u8], command: &'static str) -> Result<()> {
        self.raw
            .require(bytes, command, "exact post-build Cargo metadata")
    }

    pub(crate) fn selections(
        &self,
        dependencies: &[Selected],
        epoch: &mut Allocation,
    ) -> Result<Box<[Selection]>> {
        epoch.require(&self.limits)?;
        let mut allocation = epoch.retained;
        allocation.nodes("Cargo selection nodes", dependencies.len(), &self.limits)?;
        let mut selections = Vec::new();
        selections
            .try_reserve_exact(dependencies.len())
            .map_err(|source| Error::Allocation {
                operation: "Cargo metadata",
                source,
            })?;
        for dependency in dependencies {
            let package_id = self
                .selected
                .get(dependency.alias())
                .ok_or_else(|| schema("Cargo metadata", "selected alias proof is absent"))?;
            let package_id = self.package_id(*package_id)?;
            allocation.retained_string(
                "Cargo selection string bytes",
                dependency.alias().len(),
                &self.limits,
            )?;
            allocation.retained_string(
                "Cargo selection string bytes",
                package_id.len(),
                &self.limits,
            )?;
            selections.push(Selection {
                alias: dependency.alias().into(),
                package_id: package_id.into(),
                roles: dependency.roles(),
            });
        }
        epoch.retained = allocation;
        Ok(selections.into_boxed_slice())
    }

    pub(crate) fn digest(&self) -> Digest {
        digest(&self.canonical)
    }
}

impl BuildGraph {
    #[allow(
        clippy::too_many_arguments,
        reason = "unit-graph parsing binds every pinned Cargo command authority in one step"
    )]
    pub(crate) fn parse(
        bytes: &[u8],
        metadata: &Metadata,
        limits: &Limits,
        schema_version: u32,
        no_build_std: config::NoBuildStd,
        profile: &str,
        selected_target: &str,
        epoch: &mut Allocation,
    ) -> Result<Self> {
        Self::parse_observed(
            bytes,
            metadata,
            limits,
            schema_version,
            no_build_std,
            profile,
            selected_target,
            epoch,
            None,
        )
        .map(|(graph, _)| graph)
    }

    #[allow(
        clippy::too_many_arguments,
        reason = "one unit parse completes both Cargo and universe proof observations"
    )]
    pub(crate) fn parse_versioned(
        bytes: &[u8],
        metadata: &Metadata,
        limits: &Limits,
        schema_version: u32,
        no_build_std: config::NoBuildStd,
        profile: &str,
        selected_target: &str,
        epoch: &mut Allocation,
        definition: &super::universe::Definition,
        observation: super::universe::MetadataObservation,
    ) -> Result<(
        Self,
        super::universe::MetadataProof,
        super::universe::UnitProof,
    )> {
        let (graph, proofs) = Self::parse_observed(
            bytes,
            metadata,
            limits,
            schema_version,
            no_build_std,
            profile,
            selected_target,
            epoch,
            Some((definition, observation)),
        )?;
        let (metadata, unit) =
            proofs.ok_or_else(|| schema("Cargo unit graph", "universe proofs are absent"))?;
        Ok((graph, metadata, unit))
    }

    #[allow(
        clippy::too_many_arguments,
        reason = "the unit parser retains one cumulative authority and optional universe join"
    )]
    fn parse_observed(
        bytes: &[u8],
        metadata: &Metadata,
        limits: &Limits,
        schema_version: u32,
        no_build_std: config::NoBuildStd,
        profile: &str,
        selected_target: &str,
        epoch: &mut Allocation,
        universe: Option<(
            &super::universe::Definition,
            super::universe::MetadataObservation,
        )>,
    ) -> Result<(
        Self,
        Option<(super::universe::MetadataProof, super::universe::UnitProof)>,
    )> {
        #[cfg(not(feature = "rustc-driver"))]
        let _ = selected_target;
        epoch.require(limits)?;
        let raw = RawDocument::from_bytes(bytes);
        let mut allocation = preflight_from("Cargo unit graph", bytes, limits, epoch.retained)?;
        let value = parse_owned_json("Cargo unit graph", bytes)?;
        let document_ownership = JsonOwnership::tree(&value)?;
        let object = value
            .as_object()
            .ok_or_else(|| schema("Cargo unit graph", "root is not an object"))?;
        exact_schema(object, "Cargo unit graph", schema_version)?;
        let units = array(object, "units", "Cargo unit graph")?;
        allocation.exceeded("Cargo units", limits.units, units.len());
        let roots = array(object, "roots", "Cargo unit graph")?;
        if roots.is_empty() {
            return Err(schema("Cargo unit graph", "has no command roots"));
        }

        let mut edges = 0_usize;
        for unit in units {
            let _ = is_std(unit, no_build_std)?;
            let dependencies = unit
                .get("dependencies")
                .and_then(Value::as_array)
                .ok_or_else(|| schema("Cargo unit graph", "unit dependencies are absent"))?;
            edges = edges.saturating_add(dependencies.len());
            allocation.exceeded("Cargo unit edges", limits.edges, edges);
            for dependency in dependencies {
                let index = dependency
                    .get("index")
                    .and_then(Value::as_u64)
                    .and_then(|index| usize::try_from(index).ok())
                    .ok_or_else(|| schema("Cargo unit graph", "dependency index is invalid"))?;
                if index >= units.len() {
                    return Err(schema(
                        "Cargo unit graph",
                        "dependency index is out of range",
                    ));
                }
            }
        }

        // The metadata observation exists only to authenticate the protected
        // metadata/unit cohort. Consume that proof before retaining the
        // ordinary BuildGraph fanout so the same epoch never overlaps two
        // independent representations whose authority join is already done.
        let proofs = universe
            .map(|(definition, observation)| {
                definition.authenticate_value(&value, observation, limits, &mut allocation)
            })
            .transpose()?;

        let validation_start = allocation.retained_ownership();
        let mut selected = BTreeMap::new();
        #[cfg(feature = "rustc-driver")]
        let mut extraction_indices = BTreeMap::<&str, usize>::new();
        #[cfg(not(feature = "rustc-driver"))]
        let extraction_indices = BTreeMap::<&str, usize>::new();
        for root in roots {
            let mut root_selected = BTreeMap::<&str, BTreeSet<usize>>::new();
            let index = root
                .as_u64()
                .and_then(|index| usize::try_from(index).ok())
                .filter(|index| *index < units.len())
                .ok_or_else(|| schema("Cargo unit graph", "root index is out of range"))?;
            let unit = &units[index];
            if value_string(unit, "pkg_id") != Some(metadata.root_id()?) {
                return Err(schema(
                    "Cargo unit graph",
                    "root unit package is not the harness",
                ));
            }
            let mode = value_string(unit, "mode")
                .ok_or_else(|| schema("Cargo unit graph", "root unit mode is absent"))?;
            if !["build", "check", "run-custom-build"].contains(&mode) {
                return Err(Error::ResolutionDrift {
                    command: "build",
                    field: "root unit mode",
                });
            }
            validate_profile(unit, profile, "build")?;
            let dependencies = unit
                .get("dependencies")
                .and_then(Value::as_array)
                .ok_or_else(|| schema("Cargo unit graph", "root dependencies are absent"))?;
            for dependency in dependencies {
                let Some(alias) = dependency.get("extern_crate_name").and_then(Value::as_str)
                else {
                    continue;
                };
                let Some(expected_package) = metadata.declared.get(alias) else {
                    return Err(Error::ResolutionDrift {
                        command: "build",
                        field: "unexpected root unit dependency",
                    });
                };
                let expected_package = metadata.package_id(*expected_package)?;
                let selected_aliases = metadata
                    .aliases
                    .get(alias)
                    .ok_or_else(|| schema("Cargo metadata", "declared alias map is absent"))?;
                let index = dependency
                    .get("index")
                    .and_then(Value::as_u64)
                    .and_then(|index| usize::try_from(index).ok())
                    .ok_or_else(|| schema("Cargo unit graph", "dependency index is invalid"))?;
                let target = &units[index];
                if value_string(target, "pkg_id") != Some(expected_package) {
                    return Err(Error::ResolutionDrift {
                        command: "build",
                        field: "selected package unit identity",
                    });
                }
                let kinds = target
                    .get("target")
                    .and_then(|target| target.get("kind"))
                    .and_then(Value::as_array)
                    .ok_or_else(|| schema("Cargo unit graph", "selected target kind is absent"))?;
                let library = kinds.iter().any(|kind| {
                    kind.as_str()
                        .is_some_and(|kind| matches!(kind, "lib" | "rlib"))
                });
                if !library {
                    return Err(Error::ResolutionDrift {
                        command: "build",
                        field: "selected library target kind",
                    });
                }
                validate_profile(target, profile, "build")?;
                let expected_features = metadata
                    .packages
                    .get(expected_package)
                    .and_then(|package| package.features.as_deref())
                    .ok_or_else(|| schema("Cargo metadata", "selected feature proof is absent"))?;
                let feature_values = target
                    .get("features")
                    .and_then(Value::as_array)
                    .ok_or_else(|| {
                        schema("Cargo unit graph", "selected unit features are absent")
                    })?;
                validate_unit_features(feature_values, expected_features, &mut allocation, limits)?;
                for selected_alias in selected_aliases {
                    if !root_selected.contains_key(selected_alias.as_ref()) {
                        allocation.member_nodes("Cargo unit root-selected map nodes", 1, limits)?;
                    }
                    allocation.nodes("Cargo unit root-selected set nodes", 1, limits)?;
                    if !root_selected
                        .entry(selected_alias)
                        .or_default()
                        .insert(index)
                    {
                        return Err(Error::ResolutionDrift {
                            command: "build",
                            field: "duplicate root unit dependency",
                        });
                    }
                    #[cfg(feature = "rustc-driver")]
                    if metadata.extracts_alias(selected_alias) {
                        match extraction_indices.get(selected_alias.as_ref()) {
                            Some(previous) if *previous != index => {
                                return Err(Error::ResolutionDrift {
                                    command: "build",
                                    field: "ambiguous selected extraction unit",
                                });
                            }
                            Some(_) => {}
                            None => {
                                allocation.member_nodes(
                                    "Cargo unit extraction-index nodes",
                                    1,
                                    limits,
                                )?;
                                extraction_indices.insert(selected_alias, index);
                            }
                        }
                    }
                }
            }
            for alias in root_selected.into_keys() {
                let package = metadata
                    .selected
                    .get(alias)
                    .ok_or_else(|| schema("Cargo metadata", "selected alias proof is absent"))?;
                let package = metadata.package_id(*package)?;
                allocation.member_nodes("Cargo unit selected-map nodes", 1, limits)?;
                if selected
                    .insert(alias, package)
                    .is_some_and(|previous| previous != package)
                {
                    return Err(Error::ResolutionDrift {
                        command: "build",
                        field: "conflicting root unit dependency",
                    });
                }
            }
        }
        // Rawr's own crate is a build dependency of the harness, which has
        // no build script, so the graph has no unit of it (D609.2).
        let unbuilt = usize::from(
            metadata
                .selected
                .contains_key(super::config::INTERNAL_ALIAS)
                && !selected.contains_key(super::config::INTERNAL_ALIAS),
        );
        if selected.len() + unbuilt != metadata.selected.len() {
            return Err(Error::ResolutionDrift {
                command: "build",
                field: "complete selected unit set",
            });
        }
        let validation_ownership = allocation
            .retained_ownership()
            .checked_sub(validation_start)?;

        let canonical =
            canonical_unit_digest_budgeted(&value, no_build_std, &mut allocation, limits)?;
        let digests = unit_key_digests(&value, no_build_std, &mut allocation, limits)?;
        let masks = unit_masks(
            &value,
            &extraction_indices,
            no_build_std,
            &mut allocation,
            limits,
        )?;
        let document =
            serde_json::from_value::<OwnedUnitDocument>(value).map_err(|error| Error::Schema {
                document: "Cargo unit graph",
                reason: error.to_string().into_boxed_str(),
            })?;
        // The prepared script vector reserves one slot per document unit.
        // Admit the newly retained compiler index before allocating that vector.
        let script_indices = JsonOwnership::buffer(
            document
                .units
                .len()
                .checked_mul(std::mem::size_of::<usize>())
                .ok_or_else(|| schema("Cargo unit graph", "script index storage overflow"))?,
        );
        allocation.acquire(script_indices, limits)?;
        let prepared = prepare_owned_graph(
            document,
            masks.value(),
            selected_target,
            no_build_std,
            limits,
            &mut allocation,
        )?;
        let graph_ownership = prepared_graph_ownership(
            &prepared,
            masks.value(),
            &extraction_indices,
            metadata,
            limits,
            &mut allocation,
        )?;
        allocation.replace_owned(document_ownership, graph_ownership, limits)?;
        let finished = finish_prepared_graph(
            prepared,
            digests.value(),
            masks.value(),
            &extraction_indices,
            metadata,
            limits,
            &mut allocation,
        )?;
        allocation.release(script_indices)?;
        digests.drop_from(&mut allocation)?;
        masks.drop_from(&mut allocation)?;
        drop(selected);
        drop(extraction_indices);
        allocation.release(validation_ownership)?;
        let graph = Self {
            raw,
            canonical,
            _allocation: allocation,
            limits: *limits,
            artifacts: finished.artifacts,
            scripts: finished.scripts,
            #[cfg(feature = "rustc-driver")]
            extraction: finished.extraction,
        };
        epoch.retained = allocation;
        Ok((graph, proofs))
    }

    pub(crate) fn digest(&self) -> Digest {
        self.canonical
    }

    #[cfg(test)]
    pub(crate) fn equals(&self, other: &Self) -> bool {
        self.canonical == other.canonical
            && self.artifacts == other.artifacts
            && self.scripts == other.scripts
    }

    pub(crate) fn require_exact_raw(&self, bytes: &[u8], command: &'static str) -> Result<()> {
        self.raw
            .require(bytes, command, "exact post-build Cargo unit graph")
    }

    pub(crate) fn artifacts(&self) -> &[Output] {
        &self.artifacts
    }

    pub(crate) fn scripts(&self) -> &[BuildScript] {
        &self.scripts
    }

    #[cfg(feature = "rustc-driver")]
    pub(crate) fn extraction(&self) -> &[Extraction] {
        &self.extraction
    }

    #[cfg(test)]
    pub(crate) fn with_fixture_scripts(mut self, scripts: Box<[BuildScript]>) -> Self {
        self.scripts = scripts;
        self
    }

    #[cfg(test)]
    pub(crate) fn fixture_artifacts(artifacts: Box<[Output]>) -> Self {
        #[cfg(feature = "rustc-driver")]
        let extraction = artifacts
            .iter()
            .map(|artifact| Extraction {
                semantic_key: artifact.semantic_key.clone(),
                aliases: Box::default(),
                package_id: artifact.package_id.clone(),
                package_name: artifact.crate_name.clone(),
                package_version: "0.1.0".into(),
                package_source: "path+file:///fixture".into(),
                target: "aarch64-apple-darwin".into(),
                target_kinds: artifact.target_kinds.clone(),
                crate_types: artifact.crate_types.clone(),
                crate_name: artifact.crate_name.clone(),
                edition: "2024".into(),
                profile: "dev".into(),
                mode: artifact.mode.clone(),
                features: artifact.features.clone(),
                dependencies: Box::default(),
            })
            .collect::<Vec<_>>()
            .into_boxed_slice();
        Self {
            raw: RawDocument::from_bytes(&[]),
            _allocation: JsonAllocation::empty(),
            limits: super::Limits::standard(),
            canonical: digest(&[]),
            artifacts,
            scripts: Box::default(),
            #[cfg(feature = "rustc-driver")]
            extraction,
        }
    }

    /// One selected fixture unit with the same source path as its Build output.
    #[cfg(all(test, feature = "rustc-driver"))]
    pub(crate) fn fixture_selected_artifact(artifact: Output, alias: &str) -> Self {
        let mut graph = Self::fixture_artifacts(Box::from([artifact]));
        let selected = &mut graph.extraction[0];
        selected.target = graph.artifacts[0]
            .target
            .to_str()
            .expect("selected fixture source path is UTF-8")
            .into();
        selected.aliases = Box::from([Box::<str>::from(alias)]);
        graph
    }
}

fn validate_unit_features(
    feature_values: &[Value],
    expected_features: &[Box<str>],
    allocation: &mut JsonAllocation,
    limits: &Limits,
) -> Result<()> {
    let pointer_bytes = feature_values
        .len()
        .checked_mul(std::mem::size_of::<&str>())
        .ok_or_else(|| overflow("Cargo unit selected-feature scratch bytes"))?;
    let scratch = JsonOwnership::collection()
        .checked_add(JsonOwnership::nodes(feature_values.len()))?
        .checked_add(JsonOwnership::buffer(pointer_bytes))?;
    let authorized = allocation.with_scratch(scratch, limits, || {
        let mut actual_features = Vec::<&str>::new();
        actual_features
            .try_reserve_exact(feature_values.len())
            .map_err(|source| Error::Allocation {
                operation: "Cargo unit graph",
                source,
            })?;
        for feature in feature_values {
            actual_features.push(
                feature
                    .as_str()
                    .ok_or_else(|| schema("Cargo unit graph", "unit feature is not a string"))?,
            );
        }
        actual_features.sort_unstable();
        if actual_features.windows(2).any(|pair| pair[0] == pair[1]) {
            return Err(schema("Cargo unit graph", "duplicate unit feature"));
        }
        Ok(unit_features_authorized(
            &actual_features,
            expected_features,
        ))
    })?;
    if !authorized {
        return Err(Error::ResolutionDrift {
            command: "build",
            field: "selected unit features",
        });
    }
    Ok(())
}

fn unit_features_authorized(actual: &[&str], resolved: &[Box<str>]) -> bool {
    actual.iter().all(|feature| {
        resolved
            .binary_search_by(|candidate| candidate.as_ref().cmp(feature))
            .is_ok()
    })
}

#[cfg(test)]
mod feature_authority_tests {
    use serde_json::json;

    #[test]
    fn locked_unit_accepts_a_metadata_superset_from_disabled_platform_features() {
        let actual = ["advanced", "tiny-skia"];
        let resolved = [
            Box::<str>::from("advanced"),
            Box::<str>::from("tiny-skia"),
            Box::<str>::from("wayland"),
            Box::<str>::from("x11"),
        ];

        assert!(super::unit_features_authorized(&actual, &resolved));
    }

    #[test]
    fn locked_unit_rejects_a_feature_absent_from_metadata_authority() {
        let actual = ["advanced", "forged"];
        let resolved = [Box::<str>::from("advanced"), Box::<str>::from("x11")];

        assert!(!super::unit_features_authorized(&actual, &resolved));
    }

    #[test]
    fn borrowed_feature_scratch_has_exact_pointer_item_container_and_byte_caps() {
        let value = json!(["advanced", "std"]);
        let features = value.as_array().expect("feature fixture is an array");
        let expected = [Box::<str>::from("advanced"), Box::<str>::from("std")];
        let standard = crate::cargo::Limits::standard();
        let mut allocation = super::JsonAllocation::empty();
        super::validate_unit_features(features, &expected, &mut allocation, &standard)
            .expect("standard borrowed scratch validates");
        assert_eq!(allocation.full_usage(), (0, 0, 0, 0));
        let peak = allocation.peak_usage();
        assert_eq!(peak.2, 0, "borrowed scratch owns no string bytes");

        let mut exact = standard;
        exact.items = peak.0;
        exact.json_bytes = peak.1;
        exact.string_bytes = 0;
        exact.containers = peak.3;
        let exercise = |limits: &crate::cargo::Limits| {
            let mut allocation = super::JsonAllocation::empty();
            let result =
                super::validate_unit_features(features, &expected, &mut allocation, limits);
            (result, allocation)
        };
        let (result, allocation) = exercise(&exact);
        result.expect("exact borrowed scratch caps validate");
        assert_eq!(allocation.full_usage(), (0, 0, 0, 0));
        assert!(!allocation.exceedances().is_exceeded());

        // One below each exact scratch cap still validates and releases
        // the borrowed scratch; the crossed axis records the exact scratch peak
        // and the record outlives the release.
        let exceeded = |limits: &crate::cargo::Limits, axis: &str, high_water: usize| {
            let (result, allocation) = exercise(limits);
            result.expect("the scratch floor admits and validates");
            assert_eq!(allocation.full_usage(), (0, 0, 0, 0));
            let record = allocation.exceedances();
            let found = record
                .axis(axis)
                .unwrap_or_else(|| panic!("`{axis}` records an exceedance: {record:?}"));
            assert_eq!(found.high_water, high_water as u64, "`{axis}` high water");
            found.target
        };
        let mut pointer_floor = exact;
        pointer_floor.json_bytes = pointer_floor
            .json_bytes
            .saturating_sub(std::mem::size_of::<&str>());
        assert_eq!(
            exceeded(&pointer_floor, super::AGGREGATE_BYTES, peak.1),
            pointer_floor.json_bytes as u64
        );
        let mut item_floor = exact;
        item_floor.items = item_floor.items.saturating_sub(1);
        assert_eq!(
            exceeded(&item_floor, super::AGGREGATE_ITEMS, peak.0),
            item_floor.items as u64
        );
        let mut container_floor = exact;
        container_floor.containers = container_floor.containers.saturating_sub(1);
        assert_eq!(
            exceeded(&container_floor, super::RETAINED_CONTAINERS, peak.3),
            container_floor.containers as u64
        );
        let mut byte_floor = exact;
        byte_floor.json_bytes = byte_floor.json_bytes.saturating_sub(1);
        assert_eq!(
            exceeded(&byte_floor, super::AGGREGATE_BYTES, peak.1),
            byte_floor.json_bytes as u64
        );
    }

    #[test]
    fn borrowed_feature_scratch_succeeds_at_the_existing_string_cap_and_releases() {
        let value = json!(["default", "rust-import", "std", "sync"]);
        let features = value.as_array().expect("feature fixture is an array");
        let expected = [
            Box::<str>::from("default"),
            Box::<str>::from("rust-import"),
            Box::<str>::from("std"),
            Box::<str>::from("sync"),
        ];
        let limits = crate::cargo::Limits::standard();
        let mut allocation = super::JsonAllocation::empty();
        let live = allocation
            .retain_with(
                super::JsonOwnership::string(limits.string_bytes),
                &limits,
                || {
                    String::from_utf8(vec![b'l'; limits.string_bytes])
                        .map(String::into_boxed_str)
                        .map_err(|error| crate::Error::Schema {
                            document: "live string-cap fixture",
                            reason: error.to_string().into_boxed_str(),
                        })
                },
            )
            .expect("the exact existing string cap is live");
        let before = allocation.full_usage();
        super::validate_unit_features(features, &expected, &mut allocation, &limits)
            .expect("borrowed features add no string ownership at the exact cap");
        assert_eq!(allocation.full_usage(), before);
        assert_eq!(allocation.strings, limits.string_bytes);
        live.drop_from(&mut allocation)
            .expect("the live string-cap fixture releases once");
        assert_eq!(allocation.full_usage(), (0, 0, 0, 0));
    }

    #[test]
    fn borrowed_feature_scratch_releases_on_duplicate_and_unauthorized_errors() {
        let limits = crate::cargo::Limits::standard();
        let expected = [Box::<str>::from("advanced"), Box::<str>::from("std")];
        for value in [json!(["std", "std"]), json!(["forged"])] {
            let features = value.as_array().expect("feature fixture is an array");
            let mut allocation = super::JsonAllocation::empty();
            let before = allocation.full_usage();
            assert!(
                super::validate_unit_features(features, &expected, &mut allocation, &limits)
                    .is_err()
            );
            assert_eq!(
                allocation.full_usage(),
                before,
                "scratch releases on every validation error"
            );
        }
    }
}

impl Output {
    pub(crate) fn package_id(&self) -> &str {
        &self.package_id
    }

    pub(crate) fn target(&self) -> &Path {
        &self.target
    }

    pub(crate) fn target_kinds(&self) -> &[Box<str>] {
        &self.target_kinds
    }

    pub(crate) fn crate_types(&self) -> &[Box<str>] {
        &self.crate_types
    }

    pub(crate) fn crate_name(&self) -> &str {
        &self.crate_name
    }

    pub(crate) fn features(&self) -> &[Box<str>] {
        &self.features
    }

    pub(crate) const fn artifact_profile(&self) -> &ArtifactProfile {
        &self.artifact_profile
    }

    #[cfg(feature = "rustc-driver")]
    pub(crate) fn dependency_keys(&self) -> &[Arc<str>] {
        &self.dependencies
    }

    pub(crate) fn semantic_key(&self) -> &str {
        &self.semantic_key
    }

    pub(crate) fn platform(&self) -> Option<&str> {
        self.platform.as_deref()
    }

    pub(crate) fn mode(&self) -> &str {
        &self.mode
    }

    #[cfg(test)]
    pub(crate) fn fixture(
        package_id: &str,
        target: &str,
        crate_name: &str,
        target_kind: &str,
        crate_type: &str,
    ) -> Self {
        Self {
            package_id: package_id.into(),
            target: PathBuf::from(target),
            target_kinds: Box::from([Box::from(target_kind)]),
            crate_types: Box::from([Box::from(crate_type)]),
            crate_name: crate_name.into(),
            features: Box::default(),
            artifact_profile: ArtifactProfile::fixture(),
            platform: Some("aarch64-apple-darwin".into()),
            mode: "build".into(),
            #[cfg(feature = "rustc-driver")]
            edition: "2024".into(),
            #[cfg(feature = "rustc-driver")]
            profile_name: "dev".into(),
            #[cfg(feature = "rustc-driver")]
            dependencies: Box::default(),
            semantic_key: Arc::from(format!("fixture:{package_id}:{target}:{crate_name}")),
        }
    }

    #[cfg(all(test, feature = "rustc-driver"))]
    pub(crate) fn with_fixture_semantic_key(mut self, semantic_key: &str) -> Self {
        self.semantic_key = semantic_key.into();
        self
    }

    #[cfg(all(test, feature = "rustc-driver"))]
    fn with_fixture_dependencies(mut self, dependencies: Box<[Box<str>]>) -> Self {
        self.dependencies = dependencies
            .into_vec()
            .into_iter()
            .map(Arc::from)
            .collect::<Vec<_>>()
            .into_boxed_slice();
        self
    }
}

impl ArtifactProfile {
    /// Parse a Cargo build-message profile. Its string sizes are accounting
    /// targets recorded on `account`, the account that owns the message;
    /// the schema check is the only refusal.
    pub(crate) fn from_build_measured(
        value: &Value,
        limits: &Limits,
        account: &mut dyn Measure,
    ) -> Result<Self> {
        let object = value
            .as_object()
            .ok_or_else(|| schema("Cargo build JSON", "artifact profile is absent"))?;
        let expected = [
            "opt_level",
            "debuginfo",
            "debug_assertions",
            "overflow_checks",
            "test",
        ];
        if object.len() != expected.len() || expected.iter().any(|key| !object.contains_key(*key)) {
            return Err(schema(
                "Cargo build JSON",
                "artifact profile does not match the pinned five-field schema",
            ));
        }
        Self::from_fields(
            "Cargo build JSON",
            object.get("opt_level"),
            object.get("debuginfo"),
            object.get("debug_assertions"),
            object.get("overflow_checks"),
            object.get("test"),
            limits,
            None,
            account,
        )
    }

    /// Fixture parse with no owning account; the string targets are measured
    /// into a record that is discarded.
    #[cfg(test)]
    pub(crate) fn from_build(value: &Value, limits: &Limits) -> Result<Self> {
        Self::from_build_measured(value, limits, &mut crate::exceedance::Record::new())
    }

    fn from_owned(
        profile: &OwnedProfile,
        mode: &str,
        limits: &Limits,
        account: &mut dyn Measure,
    ) -> Result<Self> {
        account.measured(
            "artifact opt_level",
            limits.string_bytes,
            profile.opt_level.len(),
        );
        Self::validate_debuginfo("Cargo unit graph", &profile.debuginfo, limits, account)?;
        Self::encode(
            "Cargo unit graph",
            profile.opt_level.as_str(),
            &profile.debuginfo,
            profile.debug_assertions,
            profile.overflow_checks,
            matches!(mode, "test" | "bench"),
            limits,
            None,
            account,
        )
    }

    fn owned_size(
        profile: &OwnedProfile,
        mode: &str,
        limits: &Limits,
        account: &mut dyn Measure,
    ) -> Result<usize> {
        account.measured(
            "artifact opt_level",
            limits.string_bytes,
            profile.opt_level.len(),
        );
        Self::validate_debuginfo("Cargo unit graph", &profile.debuginfo, limits, account)?;
        Self::encoded_size(
            "Cargo unit graph",
            profile.opt_level.as_str(),
            &profile.debuginfo,
            profile.debug_assertions,
            profile.overflow_checks,
            matches!(mode, "test" | "bench"),
            limits,
            account,
        )
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "the profile constructor validates the complete fixed Cargo profile schema in one transformation"
    )]
    fn from_fields(
        document: &'static str,
        opt_level: Option<&Value>,
        debuginfo: Option<&Value>,
        debug_assertions: Option<&Value>,
        overflow_checks: Option<&Value>,
        test: Option<&Value>,
        limits: &Limits,
        allocation: Option<&mut JsonAllocation>,
        account: &mut dyn Measure,
    ) -> Result<Self> {
        let opt_level = opt_level
            .and_then(Value::as_str)
            .ok_or_else(|| schema(document, "artifact opt_level is not a string"))?;
        account.measured("artifact opt_level", limits.string_bytes, opt_level.len());
        let debuginfo =
            debuginfo.ok_or_else(|| schema(document, "artifact debuginfo is absent"))?;
        Self::validate_debuginfo(document, debuginfo, limits, account)?;
        let debug_assertions = debug_assertions
            .and_then(Value::as_bool)
            .ok_or_else(|| schema(document, "artifact debug_assertions is not a boolean"))?;
        let overflow_checks = overflow_checks
            .and_then(Value::as_bool)
            .ok_or_else(|| schema(document, "artifact overflow_checks is not a boolean"))?;
        let test = test
            .and_then(Value::as_bool)
            .ok_or_else(|| schema(document, "artifact test is not a boolean"))?;
        Self::encode(
            document,
            opt_level,
            debuginfo,
            debug_assertions,
            overflow_checks,
            test,
            limits,
            allocation,
            account,
        )
    }

    fn validate_debuginfo(
        document: &'static str,
        debuginfo: &Value,
        limits: &Limits,
        account: &mut dyn Measure,
    ) -> Result<()> {
        match debuginfo {
            Value::Null => {}
            Value::Number(number) if number.as_u64().is_some_and(|value| value <= 2) => {}
            Value::String(value) => {
                account.measured("artifact debuginfo", limits.string_bytes, value.len());
            }
            _ => return Err(schema(document, "artifact debuginfo is invalid")),
        }
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn encoded_size(
        document: &'static str,
        opt_level: &str,
        debuginfo: &Value,
        debug_assertions: bool,
        overflow_checks: bool,
        test: bool,
        limits: &Limits,
        account: &mut dyn Measure,
    ) -> Result<usize> {
        #[derive(serde::Serialize)]
        struct Projection<'value> {
            debug_assertions: bool,
            debuginfo: &'value Value,
            opt_level: &'value str,
            overflow_checks: bool,
            test: bool,
        }
        struct Counter(usize);
        impl std::io::Write for Counter {
            fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
                self.0 = self
                    .0
                    .checked_add(bytes.len())
                    .ok_or_else(|| std::io::Error::other("artifact profile size overflow"))?;
                Ok(bytes.len())
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }
        let projection = Projection {
            debug_assertions,
            debuginfo,
            opt_level,
            overflow_checks,
            test,
        };
        let mut counter = Counter(0);
        serde_json::to_writer(&mut counter, &projection).map_err(|error| Error::Schema {
            document,
            reason: error.to_string().into_boxed_str(),
        })?;
        account.measured("artifact profile bytes", limits.string_bytes, counter.0);
        Ok(counter.0)
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "the canonical Cargo profile is a fixed five-field projection"
    )]
    fn encode(
        document: &'static str,
        opt_level: &str,
        debuginfo: &Value,
        debug_assertions: bool,
        overflow_checks: bool,
        test: bool,
        limits: &Limits,
        allocation: Option<&mut JsonAllocation>,
        account: &mut dyn Measure,
    ) -> Result<Self> {
        #[derive(serde::Serialize)]
        struct Projection<'value> {
            debug_assertions: bool,
            debuginfo: &'value Value,
            opt_level: &'value str,
            overflow_checks: bool,
            test: bool,
        }
        let projection = Projection {
            debug_assertions,
            debuginfo,
            opt_level,
            overflow_checks,
            test,
        };
        let size = Self::encoded_size(
            document,
            opt_level,
            debuginfo,
            debug_assertions,
            overflow_checks,
            test,
            limits,
            account,
        )?;
        if let Some(allocation) = allocation {
            allocation.retained_bytes(
                "Cargo artifact profile retained bytes",
                size,
                limits.string_bytes,
                limits,
            )?;
        }
        let mut bytes = Vec::new();
        bytes
            .try_reserve_exact(size)
            .map_err(|source| Error::Allocation {
                operation: document,
                source,
            })?;
        serde_json::to_writer(&mut bytes, &projection).map_err(|error| Error::Schema {
            document,
            reason: error.to_string().into_boxed_str(),
        })?;
        Ok(Self(bytes.into_boxed_slice()))
    }

    pub(crate) fn as_bytes(&self) -> &[u8] {
        &self.0
    }

    /// Move admitted historical bytes only when they equal this validated profile.
    /// Matching profile data does not establish artifact or current owner identity.
    pub(crate) fn retain_exact(&self, bytes: Box<[u8]>) -> Result<Self> {
        if bytes.as_ref() != self.as_bytes() {
            return Err(Error::ResolutionDrift {
                command: "retained Build decode",
                field: "artifact profile",
            });
        }
        Ok(Self(bytes))
    }

    #[cfg(test)]
    fn fixture() -> Self {
        Self(
            br#"{"debug_assertions":true,"debuginfo":2,"opt_level":"0","overflow_checks":true,"test":false}"#
                .as_slice()
                .into(),
        )
    }
}

impl BuildGraph {
    /// Rebind transient reachability work to every proof retained after this
    /// graph was parsed, without retaining an independently spendable epoch.
    pub(crate) fn bind_reachability(&mut self, epoch: &Allocation, limits: &Limits) -> Result<()> {
        epoch.require(limits)?;
        self._allocation = epoch.retained;
        Ok(())
    }
}

#[cfg(feature = "rustc-driver")]
impl BuildGraph {
    pub(crate) fn artifact_reachable(&self, roots: &BTreeSet<&str>, target: &str) -> Result<bool> {
        let mut allocation = self._allocation;
        self.artifact_reachable_budgeted(roots, target, &mut allocation)
    }

    fn artifact_reachable_budgeted(
        &self,
        roots: &BTreeSet<&str>,
        target: &str,
        allocation: &mut JsonAllocation,
    ) -> Result<bool> {
        allocation.member_nodes(
            "Cargo reachability artifact-index nodes",
            self.artifacts.len(),
            &self.limits,
        )?;
        let mut artifacts = BTreeMap::new();
        for artifact in &self.artifacts {
            artifacts.insert(artifact.semantic_key(), artifact);
            #[cfg(test)]
            OWNED_REACHABILITY_NODES.with(|owned| owned.set(owned.get().saturating_add(1)));
        }
        allocation.nodes(
            "Cargo reachability pending nodes",
            roots.len(),
            &self.limits,
        )?;
        let mut pending = Vec::new();
        pending
            .try_reserve_exact(roots.len())
            .map_err(|source| Error::Allocation {
                operation: "Cargo artifact reachability",
                source,
            })?;
        for root in roots {
            pending.push(*root);
            #[cfg(test)]
            OWNED_REACHABILITY_NODES.with(|owned| owned.set(owned.get().saturating_add(1)));
        }
        let mut visited = BTreeSet::new();
        while let Some(key) = pending.pop() {
            if key == target {
                return Ok(true);
            }
            if visited.contains(key) {
                continue;
            }
            allocation.member_nodes("Cargo reachability visited nodes", 1, &self.limits)?;
            visited.insert(key);
            #[cfg(test)]
            OWNED_REACHABILITY_NODES.with(|owned| owned.set(owned.get().saturating_add(1)));
            if let Some(artifact) = artifacts.get(key) {
                for dependency in artifact.dependency_keys() {
                    allocation.nodes("Cargo reachability pending nodes", 1, &self.limits)?;
                    pending.try_reserve(1).map_err(|source| Error::Allocation {
                        operation: "Cargo artifact reachability",
                        source,
                    })?;
                    pending.push(dependency);
                    #[cfg(test)]
                    OWNED_REACHABILITY_NODES.with(|owned| owned.set(owned.get().saturating_add(1)));
                }
            }
        }
        Ok(false)
    }

    #[cfg(test)]
    fn artifact_reachable_allocation(
        &self,
        roots: &BTreeSet<&str>,
        target: &str,
    ) -> Result<JsonAllocation> {
        let mut allocation = self._allocation;
        let _ = self.artifact_reachable_budgeted(roots, target, &mut allocation)?;
        Ok(allocation)
    }

    pub(crate) fn artifact_selected_reachable(&self, target: &str) -> Result<bool> {
        let mut allocation = self._allocation;
        let mut roots = BTreeSet::new();
        for extraction in &self.extraction {
            let key = extraction.semantic_key();
            if !roots.contains(key) {
                allocation.member_nodes(
                    "Cargo reachability selected-root nodes",
                    1,
                    &self.limits,
                )?;
                roots.insert(key);
            }
        }
        self.artifact_reachable_budgeted(&roots, target, &mut allocation)
    }
}

impl BuildScript {
    #[cfg(feature = "rustc-driver")]
    pub(crate) fn semantic_key(&self) -> &str {
        &self.semantic_key
    }

    #[cfg(feature = "rustc-driver")]
    pub(crate) fn dependency_keys(&self) -> &[Arc<str>] {
        &self.dependencies
    }

    pub(crate) fn compiler(&self) -> &str {
        &self.compiler
    }

    pub(crate) fn platform(&self) -> Option<&str> {
        self.platform.as_deref()
    }

    pub(crate) fn package_id(&self) -> &str {
        &self.package_id
    }

    #[cfg(test)]
    pub(crate) fn fixture_compiler(output: &Output, platform: Option<&str>) -> Self {
        Self {
            package_id: output.package_id().into(),
            platform: platform.map(Into::into),
            compiler: output.semantic_key.clone(),
            semantic_key: Arc::from(format!("fixture-script:{}", output.semantic_key())),
            dependencies: Box::from([output.semantic_key.clone()]),
        }
    }

    #[cfg(test)]
    pub(crate) fn fixture(package_id: &str, context: &str) -> Self {
        Self {
            package_id: package_id.into(),
            platform: Some(context.into()),
            compiler: Arc::from("fixture-unbound"),
            semantic_key: Arc::from("fixture-script-unbound"),
            dependencies: Box::default(),
        }
    }
}

impl UnitMasks {
    fn retained(&self, index: usize) -> bool {
        self.flags
            .get(index)
            .is_some_and(|flags| flags & RETAINED_UNIT != 0)
    }

    fn extraction(&self, index: usize) -> bool {
        self.flags
            .get(index)
            .is_some_and(|flags| flags & EXTRACTION_UNIT != 0)
    }

    fn retained_count(&self) -> usize {
        self.flags
            .iter()
            .filter(|flags| **flags & RETAINED_UNIT != 0)
            .count()
    }

    fn extraction_count(&self) -> usize {
        self.flags
            .iter()
            .filter(|flags| **flags & EXTRACTION_UNIT != 0)
            .count()
    }
}

fn unit_masks(
    value: &Value,
    extraction_indices: &BTreeMap<&str, usize>,
    no_build_std: config::NoBuildStd,
    allocation: &mut JsonAllocation,
    limits: &Limits,
) -> Result<Budgeted<UnitMasks>> {
    let units = value
        .get("units")
        .and_then(Value::as_array)
        .ok_or_else(|| schema("Cargo unit graph", "units are absent"))?;
    let ownership = JsonOwnership::collection()
        .checked_add(JsonOwnership::nodes(units.len()))?
        .checked_add(JsonOwnership::buffer(units.len()))?;
    allocation.retain_with(ownership, limits, || {
        let mut flags = vec![0_u8; units.len()];
        for (index, unit) in units.iter().enumerate() {
            let mode = value_string(unit, "mode")
                .ok_or_else(|| schema("Cargo unit graph", "unit mode is absent"))?;
            if !matches!(mode, "build" | "check") || is_std(unit, no_build_std)? {
                continue;
            }
            flags[index] |= RETAINED_UNIT;
            let dependencies = unit
                .get("dependencies")
                .and_then(Value::as_array)
                .ok_or_else(|| schema("Cargo unit graph", "unit dependencies are absent"))?;
            for dependency in dependencies {
                let dependency = dependency
                    .get("index")
                    .and_then(Value::as_u64)
                    .and_then(|index| usize::try_from(index).ok())
                    .filter(|index| *index < flags.len())
                    .ok_or_else(|| schema("Cargo unit graph", "dependency index is invalid"))?;
                flags[dependency] |= RETAINED_UNIT;
            }
        }
        for index in extraction_indices.values().copied() {
            let flags = flags.get_mut(index).ok_or_else(|| {
                schema("Cargo unit graph", "selected extraction index is invalid")
            })?;
            *flags |= RETAINED_UNIT | EXTRACTION_UNIT;
        }
        Ok(UnitMasks {
            flags: flags.into_boxed_slice(),
        })
    })
}

fn owned_is_std(unit: &OwnedUnit, no_build_std: config::NoBuildStd) -> Result<bool> {
    match unit.is_std {
        None if no_build_std.admits_pinned_omission() => Ok(false),
        None => Err(schema(
            "Cargo unit graph",
            "unit is_std is absent without no-build-std authority",
        )),
        Some(false) => Ok(false),
        Some(true) => Err(schema(
            "Cargo unit graph",
            "build-std/sysroot Cargo unit is unsupported",
        )),
    }
}

fn script_compiler(units: &[OwnedUnit], unit: &OwnedUnit) -> Result<usize> {
    let mut compiler = None;
    for dependency in &unit.dependencies {
        let candidate = units
            .get(dependency.index)
            .ok_or_else(|| schema("Cargo unit graph", "script dependency index is invalid"))?;
        if candidate.pkg_id == unit.pkg_id
            && candidate.mode == "build"
            && candidate.platform.is_none()
            && candidate
                .target
                .kind
                .iter()
                .any(|kind| kind == "custom-build")
            && compiler.replace(dependency.index).is_some()
        {
            return Err(schema(
                "Cargo unit graph",
                "script compiler dependency is ambiguous",
            ));
        }
    }
    compiler.ok_or_else(|| {
        schema(
            "Cargo unit graph",
            "script host compiler dependency is absent",
        )
    })
}

fn prepare_owned_graph(
    document: OwnedUnitDocument,
    masks: &UnitMasks,
    selected_target: &str,
    no_build_std: config::NoBuildStd,
    limits: &Limits,
    allocation: &mut JsonAllocation,
) -> Result<PreparedGraph> {
    let OwnedUnitDocument {
        mut units,
        _roots: _,
        _version: _,
    } = document;
    let unit_count = units.len();
    let mut outputs = Vec::new();
    let mut scripts = Vec::new();
    outputs
        .try_reserve_exact(unit_count)
        .map_err(|source| Error::Allocation {
            operation: "Cargo unit graph",
            source,
        })?;
    scripts
        .try_reserve_exact(unit_count)
        .map_err(|source| Error::Allocation {
            operation: "Cargo unit graph",
            source,
        })?;
    // Resolve the actual dependency before moving the owned unit documents.
    // Moving the run's strings preserves their existing ownership allocation.
    for index in 0..units.len() {
        let unit = &units[index];
        if unit.mode != "run-custom-build" || owned_is_std(unit, no_build_std)? {
            continue;
        }
        let compiler = script_compiler(&units, unit)?;
        // The unit target is an accounting target recorded on the epoch.
        allocation.exceeded(
            "normal Build script units",
            limits.units,
            scripts.len().saturating_add(1),
        );
        let unit = &mut units[index];
        scripts.push(Run {
            unit: index,
            package_id: std::mem::take(&mut unit.pkg_id),
            platform: unit.platform.take(),
            compiler,
            dependencies: unit
                .dependencies
                .iter()
                .map(|dependency| dependency.index)
                .collect(),
        });
    }
    // A library's own build-script run is an ordering edge, not an extern crate.
    let script_runs = units
        .iter()
        .map(|unit| unit.mode == "run-custom-build")
        .collect::<Vec<_>>();
    for (index, unit) in units.into_iter().enumerate() {
        let compiler_shipped = owned_is_std(&unit, no_build_std)?;
        match unit.mode.as_str() {
            "run-custom-build" if !compiler_shipped => {
                if !unit.target.kind.iter().any(|kind| kind == "custom-build") {
                    return Err(schema(
                        "Cargo unit graph",
                        "run-custom-build unit lacks custom-build target",
                    ));
                }
            }
            "build" | "check" if !compiler_shipped => {
                if masks.extraction(index)
                    && (unit.platform.as_deref() != Some(selected_target)
                        || !unit
                            .target
                            .kind
                            .iter()
                            .any(|kind| matches!(kind.as_str(), "lib" | "rlib")))
                {
                    return Err(Error::ResolutionDrift {
                        command: "build",
                        field: "selected target normal-library unit",
                    });
                }
                allocation.exceeded(
                    "Build compiler artifact units",
                    limits.units,
                    outputs.len().saturating_add(1),
                );
                let target = canonical_file(
                    "Build target source",
                    Path::new(unit.target.src_path.as_str()),
                )?;
                let dependencies = unit
                    .dependencies
                    .iter()
                    .map(|dependency| dependency.index)
                    .collect();
                let extraction_dependencies = masks.extraction(index).then(|| {
                    unit.dependencies
                        .into_iter()
                        .filter(|dependency| script_runs.get(dependency.index) != Some(&true))
                        .collect()
                });
                outputs.push(PreparedOutput {
                    unit: index,
                    package_id: unit.pkg_id,
                    target,
                    target_kinds: unit.target.kind,
                    crate_types: unit.target.crate_types,
                    crate_name: unit.target.name,
                    edition: unit.target.edition,
                    features: unit.features,
                    profile: unit.profile,
                    platform: unit.platform,
                    mode: unit.mode,
                    dependencies,
                    extraction_dependencies,
                });
            }
            "build" | "check" | "run-custom-build" => {}
            mode => {
                return Err(schema(
                    "Cargo unit graph",
                    &format!("unsupported normal build unit mode `{mode}`"),
                ));
            }
        }
    }
    outputs.sort_unstable_by_key(|output| output.unit);
    scripts.sort_unstable();
    Ok(PreparedGraph { outputs, scripts })
}

fn add_ownership(ownership: &mut JsonOwnership, added: JsonOwnership) -> Result<()> {
    *ownership = ownership.checked_add(added)?;
    Ok(())
}

fn own_string(ownership: &mut JsonOwnership, value: &str) -> Result<()> {
    add_ownership(ownership, JsonOwnership::string(value.len()))
}

fn own_slice(ownership: &mut JsonOwnership, len: usize) -> Result<()> {
    add_ownership(ownership, JsonOwnership::collection())?;
    add_ownership(ownership, JsonOwnership::nodes(len))
}

fn prepared_output_ownership(
    output: &PreparedOutput,
    ownership: &mut JsonOwnership,
    limits: &Limits,
    account: &mut dyn Measure,
) -> Result<()> {
    add_ownership(ownership, JsonOwnership::nodes(1))?;
    for value in [
        output.package_id.as_str(),
        output
            .target
            .to_str()
            .ok_or_else(|| schema("Cargo unit graph", "Build target source is not UTF-8"))?,
        output.crate_name.as_str(),
        output.edition.as_str(),
        output.profile.name.as_str(),
        output.platform.as_deref().unwrap_or_default(),
        output.mode.as_str(),
    ] {
        if !value.is_empty() {
            own_string(ownership, value)?;
        }
    }
    for values in [
        output.target_kinds.as_slice(),
        output.crate_types.as_slice(),
        output.features.as_slice(),
    ] {
        own_slice(ownership, values.len())?;
        for value in values {
            own_string(ownership, value)?;
        }
    }
    own_slice(ownership, output.dependencies.len())?;
    add_ownership(
        ownership,
        JsonOwnership::buffer(ArtifactProfile::owned_size(
            &output.profile,
            output.mode.as_str(),
            limits,
            account,
        )?),
    )
}

fn prepared_graph_ownership(
    prepared: &PreparedGraph,
    masks: &UnitMasks,
    extraction_indices: &BTreeMap<&str, usize>,
    metadata: &Metadata,
    limits: &Limits,
    account: &mut dyn Measure,
) -> Result<JsonOwnership> {
    #[cfg(not(feature = "rustc-driver"))]
    let _ = (extraction_indices, metadata);
    let mut ownership = JsonOwnership::empty();
    own_slice(&mut ownership, prepared.outputs.len())?;
    own_slice(&mut ownership, prepared.scripts.len())?;
    // Arc slots own references, while the shared key payloads are charged below.
    add_ownership(
        &mut ownership,
        JsonOwnership::buffer(
            prepared
                .scripts
                .len()
                .checked_mul(2 * std::mem::size_of::<Arc<str>>())
                .ok_or_else(|| schema("Cargo unit graph", "script compiler storage overflow"))?,
        ),
    )?;
    #[cfg(not(feature = "rustc-driver"))]
    add_ownership(
        &mut ownership,
        JsonOwnership::buffer(
            prepared
                .outputs
                .len()
                .checked_mul(std::mem::size_of::<Arc<str>>())
                .ok_or_else(|| {
                    schema("Cargo unit graph", "output semantic key storage overflow")
                })?,
        ),
    )?;
    #[cfg(feature = "rustc-driver")]
    own_slice(&mut ownership, masks.extraction_count())?;
    for output in &prepared.outputs {
        prepared_output_ownership(output, &mut ownership, limits, account)?;
    }
    for script in &prepared.scripts {
        add_ownership(&mut ownership, JsonOwnership::nodes(1))?;
        own_string(&mut ownership, &script.package_id)?;
        own_slice(&mut ownership, script.dependencies.len())?;
        add_ownership(
            &mut ownership,
            JsonOwnership::buffer(
                script
                    .dependencies
                    .len()
                    .checked_mul(std::mem::size_of::<Arc<str>>())
                    .ok_or_else(|| {
                        schema("Cargo unit graph", "script dependency storage overflow")
                    })?,
            ),
        )?;
        if let Some(platform) = &script.platform {
            own_string(&mut ownership, platform)?;
        }
    }
    for _ in 0..masks.retained_count() {
        add_ownership(&mut ownership, JsonOwnership::string(64))?;
    }
    #[cfg(feature = "rustc-driver")]
    for index in (0..masks.flags.len()).filter(|index| masks.extraction(*index)) {
        let output = prepared
            .outputs
            .binary_search_by_key(&index, |output| output.unit)
            .ok()
            .and_then(|index| prepared.outputs.get(index))
            .ok_or_else(|| schema("Cargo unit graph", "selected Build output is absent"))?;
        let package = metadata
            .packages
            .get(output.package_id.as_str())
            .ok_or_else(|| schema("Cargo metadata", "closure package evidence is absent"))?;
        add_ownership(&mut ownership, JsonOwnership::nodes(1))?;
        let aliases = extraction_indices
            .iter()
            .filter(|(_, selected)| **selected == index)
            .map(|(alias, _)| *alias);
        own_slice(&mut ownership, aliases.clone().count())?;
        for alias in aliases {
            own_string(&mut ownership, alias)?;
        }
        for value in [
            output.package_id.as_str(),
            package.name.as_ref(),
            package.version.as_ref(),
            package.source.cargo_id(),
            output
                .target
                .to_str()
                .ok_or_else(|| schema("Cargo unit graph", "selected target is not UTF-8"))?,
            output.crate_name.as_str(),
            output.edition.as_str(),
            output.profile.name.as_str(),
            output.mode.as_str(),
        ] {
            own_string(&mut ownership, value)?;
        }
        for values in [
            output.target_kinds.as_slice(),
            output.crate_types.as_slice(),
            output.features.as_slice(),
        ] {
            own_slice(&mut ownership, values.len())?;
            for value in values {
                own_string(&mut ownership, value)?;
            }
        }
        let dependencies = output
            .extraction_dependencies
            .as_deref()
            .ok_or_else(|| schema("Cargo unit graph", "selected dependency proof is absent"))?;
        own_slice(&mut ownership, dependencies.len())?;
        for dependency in dependencies {
            let target = prepared
                .outputs
                .binary_search_by_key(&dependency.index, |output| output.unit)
                .ok()
                .and_then(|index| prepared.outputs.get(index))
                .ok_or_else(|| schema("Cargo unit graph", "dependency Build output is absent"))?;
            add_ownership(&mut ownership, JsonOwnership::nodes(1))?;
            if let Some(alias) = &dependency.extern_crate_name {
                own_string(&mut ownership, alias)?;
            }
            for value in [
                target.package_id.as_str(),
                target
                    .target
                    .to_str()
                    .ok_or_else(|| schema("Cargo unit graph", "dependency target is not UTF-8"))?,
                target.crate_name.as_str(),
            ] {
                own_string(&mut ownership, value)?;
            }
            for values in [
                target.target_kinds.as_slice(),
                target.crate_types.as_slice(),
                target.features.as_slice(),
            ] {
                own_slice(&mut ownership, values.len())?;
                for value in values {
                    own_string(&mut ownership, value)?;
                }
            }
        }
    }
    Ok(ownership)
}

fn semantic_key(digest: Digest) -> Result<Arc<str>> {
    let mut key = String::new();
    key.try_reserve_exact(64)
        .map_err(|source| Error::Allocation {
            operation: "Cargo unit graph",
            source,
        })?;
    for byte in digest.as_bytes() {
        use std::fmt::Write as _;
        write!(key, "{byte:02x}").map_err(|error| Error::Schema {
            document: "Cargo unit graph",
            reason: error.to_string().into_boxed_str(),
        })?;
    }
    Ok(Arc::from(key))
}

fn unit_key_digests(
    value: &Value,
    no_build_std: config::NoBuildStd,
    allocation: &mut JsonAllocation,
    limits: &Limits,
) -> Result<Budgeted<Box<[Digest]>>> {
    let object = value
        .as_object()
        .ok_or_else(|| schema("Cargo unit graph", "root is not an object"))?;
    let units = array(object, "units", "Cargo unit graph")?;
    let buffer_bytes = std::mem::size_of::<Digest>()
        .checked_mul(units.len())
        .ok_or_else(|| overflow("Cargo semantic unit digest bytes"))?;
    let ownership = JsonOwnership::collection()
        .checked_add(JsonOwnership::nodes(units.len()))?
        .checked_add(JsonOwnership::buffer(buffer_bytes))?;
    allocation.transform_owned(JsonOwnership::empty(), |allocation| {
        allocation.acquire(ownership, limits)?;
        let mut digests = Vec::new();
        digests
            .try_reserve_exact(units.len())
            .map_err(|source| Error::Allocation {
                operation: "Cargo unit graph",
                source,
            })?;
        for key in unit_key_buffers(units, no_build_std, allocation, limits)? {
            digests.push(digest(key.value()));
            key.drop_from(allocation)?;
        }
        Ok(Budgeted::from_retained(
            digests.into_boxed_slice(),
            ownership,
        ))
    })
}

#[cfg(feature = "rustc-driver")]
fn cloned_strings(values: &[String]) -> Box<[Box<str>]> {
    values
        .iter()
        .map(|value| value.as_str().into())
        .collect::<Vec<_>>()
        .into_boxed_slice()
}

fn finish_prepared_graph(
    prepared: PreparedGraph,
    digests: &[Digest],
    masks: &UnitMasks,
    extraction_indices: &BTreeMap<&str, usize>,
    metadata: &Metadata,
    limits: &Limits,
    allocation: &mut JsonAllocation,
) -> Result<FinishedGraph> {
    #[cfg(not(feature = "rustc-driver"))]
    let _ = (extraction_indices, metadata);
    let scratch = JsonOwnership::collection()
        .checked_add(JsonOwnership::nodes(digests.len()))?
        .checked_add(JsonOwnership::buffer(
            std::mem::size_of::<Option<Arc<str>>>()
                .checked_mul(digests.len())
                .ok_or_else(|| overflow("Cargo semantic unit index bytes"))?,
        ))?;
    // The epoch is borrowed by the scratch transaction below, so the profile
    // string targets measured inside it accumulate in a detached record that
    // is folded into the epoch once the transaction returns.
    let mut measured = crate::exceedance::Record::new();
    let finished = allocation.with_scratch(scratch, limits, || {
        let mut keys = vec![None; digests.len()];
        for index in (0..digests.len()).filter(|index| masks.retained(*index)) {
            let slot = keys
                .get_mut(index)
                .ok_or_else(|| schema("Cargo unit graph", "semantic unit index is invalid"))?;
            *slot = Some(semantic_key(digests[index])?);
        }

        #[cfg(feature = "rustc-driver")]
        let mut extraction = Vec::new();
        #[cfg(feature = "rustc-driver")]
        for index in (0..masks.flags.len()).filter(|index| masks.extraction(*index)) {
            let output = prepared
                .outputs
                .binary_search_by_key(&index, |output| output.unit)
                .ok()
                .and_then(|index| prepared.outputs.get(index))
                .ok_or_else(|| schema("Cargo unit graph", "selected Build output is absent"))?;
            let package = metadata
                .packages
                .get(output.package_id.as_str())
                .ok_or_else(|| schema("Cargo metadata", "closure package evidence is absent"))?;
            let mut aliases = extraction_indices
                .iter()
                .filter(|(_, selected)| **selected == index)
                .map(|(alias, _)| Box::<str>::from(*alias))
                .collect::<Vec<_>>();
            aliases.sort_unstable();
            let dependencies = output
                .extraction_dependencies
                .as_deref()
                .ok_or_else(|| schema("Cargo unit graph", "selected dependencies are absent"))?
                .iter()
                .map(|dependency| {
                    let target = prepared
                        .outputs
                        .binary_search_by_key(&dependency.index, |output| output.unit)
                        .ok()
                        .and_then(|index| prepared.outputs.get(index))
                        .ok_or_else(|| {
                            schema("Cargo unit graph", "dependency Build output is absent")
                        })?;
                    Ok(Dependency {
                        extern_alias: dependency
                            .extern_crate_name
                            .as_deref()
                            .map(Box::<str>::from),
                        semantic_key: keys[dependency.index]
                            .as_ref()
                            .ok_or_else(|| {
                                schema("Cargo unit graph", "dependency semantic key is absent")
                            })?
                            .clone(),
                        package_id: target.package_id.as_str().into(),
                        target: target.target.clone(),
                        target_kinds: cloned_strings(&target.target_kinds),
                        crate_types: cloned_strings(&target.crate_types),
                        crate_name: target.crate_name.as_str().into(),
                        features: cloned_strings(&target.features),
                    })
                })
                .collect::<Result<Vec<_>>>()?;
            extraction.push(Extraction {
                semantic_key: keys[index]
                    .as_ref()
                    .ok_or_else(|| schema("Cargo unit graph", "selected semantic key is absent"))?
                    .clone(),
                aliases: aliases.into_boxed_slice(),
                package_id: output.package_id.as_str().into(),
                package_name: package.name.as_ref().into(),
                package_version: package.version.as_ref().into(),
                package_source: package.source.cargo_id().into(),
                target: output
                    .target
                    .to_str()
                    .ok_or_else(|| schema("Cargo unit graph", "selected target is not UTF-8"))?
                    .into(),
                target_kinds: cloned_strings(&output.target_kinds),
                crate_types: cloned_strings(&output.crate_types),
                crate_name: output.crate_name.as_str().into(),
                edition: output.edition.as_str().into(),
                profile: output.profile.name.as_str().into(),
                mode: output.mode.as_str().into(),
                features: cloned_strings(&output.features),
                dependencies: dependencies.into_boxed_slice(),
            });
        }
        #[cfg(feature = "rustc-driver")]
        extraction.sort_unstable();

        let mut artifacts = Vec::new();
        artifacts
            .try_reserve_exact(prepared.outputs.len())
            .map_err(|source| Error::Allocation {
                operation: "Cargo unit graph",
                source,
            })?;
        for output in prepared.outputs {
            let semantic_key = keys[output.unit]
                .as_ref()
                .ok_or_else(|| schema("Cargo unit graph", "Build semantic key is absent"))?
                .clone();
            #[cfg(feature = "rustc-driver")]
            let dependencies = output
                .dependencies
                .iter()
                .map(|index| {
                    keys.get(*index)
                        .and_then(Option::as_ref)
                        .cloned()
                        .ok_or_else(|| {
                            schema(
                                "Cargo unit graph",
                                "Build dependency semantic key is absent",
                            )
                        })
                })
                .collect::<Result<Vec<_>>>()?;
            artifacts.push(Output {
                package_id: output.package_id.into_boxed_str(),
                target: output.target,
                target_kinds: output
                    .target_kinds
                    .into_iter()
                    .map(String::into_boxed_str)
                    .collect::<Vec<_>>()
                    .into_boxed_slice(),
                crate_types: output
                    .crate_types
                    .into_iter()
                    .map(String::into_boxed_str)
                    .collect::<Vec<_>>()
                    .into_boxed_slice(),
                crate_name: output.crate_name.into_boxed_str(),
                features: output
                    .features
                    .into_iter()
                    .map(String::into_boxed_str)
                    .collect::<Vec<_>>()
                    .into_boxed_slice(),
                artifact_profile: ArtifactProfile::from_owned(
                    &output.profile,
                    output.mode.as_str(),
                    limits,
                    &mut measured,
                )?,
                platform: output.platform.map(String::into_boxed_str),
                mode: output.mode.into_boxed_str(),
                #[cfg(feature = "rustc-driver")]
                edition: output.edition.into_boxed_str(),
                #[cfg(feature = "rustc-driver")]
                profile_name: output.profile.name.into_boxed_str(),
                #[cfg(feature = "rustc-driver")]
                dependencies: dependencies.into_boxed_slice(),
                semantic_key,
            });
        }
        artifacts.sort_unstable();
        artifacts.dedup();
        let mut scripts = prepared
            .scripts
            .into_iter()
            .map(|script| {
                Ok(BuildScript {
                    package_id: script.package_id.into_boxed_str(),
                    platform: script.platform.map(String::into_boxed_str),
                    semantic_key: keys
                        .get(script.unit)
                        .and_then(Option::as_ref)
                        .ok_or_else(|| {
                            schema("Cargo unit graph", "script run semantic key is absent")
                        })?
                        .clone(),
                    dependencies: script
                        .dependencies
                        .into_iter()
                        .map(|index| {
                            keys.get(index)
                                .and_then(Option::as_ref)
                                .cloned()
                                .ok_or_else(|| {
                                    schema(
                                        "Cargo unit graph",
                                        "script dependency semantic key is absent",
                                    )
                                })
                        })
                        .collect::<Result<Vec<_>>>()?
                        .into_boxed_slice(),
                    compiler: keys
                        .get(script.compiler)
                        .and_then(Option::as_ref)
                        .ok_or_else(|| {
                            schema("Cargo unit graph", "script compiler semantic key is absent")
                        })?
                        .clone(),
                })
            })
            .collect::<Result<Vec<_>>>()?;
        scripts.sort_unstable();
        Ok(FinishedGraph {
            artifacts: artifacts.into_boxed_slice(),
            scripts: scripts.into_boxed_slice(),
            #[cfg(feature = "rustc-driver")]
            extraction: extraction.into_boxed_slice(),
        })
    });
    allocation.record(&measured);
    finished
}

#[cfg(feature = "rustc-driver")]
impl Extraction {
    pub(crate) fn aliases(&self) -> &[Box<str>] {
        &self.aliases
    }

    pub(crate) fn semantic_key(&self) -> &str {
        &self.semantic_key
    }

    pub(crate) fn package_id(&self) -> &str {
        &self.package_id
    }

    pub(crate) fn package_name(&self) -> &str {
        &self.package_name
    }

    pub(crate) fn package_version(&self) -> &str {
        &self.package_version
    }

    pub(crate) fn package_source(&self) -> &str {
        &self.package_source
    }

    pub(crate) fn target(&self) -> &str {
        &self.target
    }

    pub(crate) fn target_kinds(&self) -> &[Box<str>] {
        &self.target_kinds
    }

    pub(crate) fn crate_types(&self) -> &[Box<str>] {
        &self.crate_types
    }

    pub(crate) fn crate_name(&self) -> &str {
        &self.crate_name
    }

    pub(crate) fn edition(&self) -> &str {
        &self.edition
    }

    pub(crate) fn profile(&self) -> &str {
        &self.profile
    }

    pub(crate) fn features(&self) -> &[Box<str>] {
        &self.features
    }

    pub(crate) fn dependencies(&self) -> &[Dependency] {
        &self.dependencies
    }
}

#[cfg(feature = "rustc-driver")]
impl Dependency {
    pub(crate) fn semantic_key(&self) -> &str {
        &self.semantic_key
    }
}

fn is_std(unit: &Value, no_build_std: config::NoBuildStd) -> Result<bool> {
    match unit.get("is_std") {
        None if no_build_std.admits_pinned_omission() => Ok(false),
        None => Err(schema(
            "Cargo unit graph",
            "unit is_std is absent without no-build-std authority",
        )),
        Some(Value::Bool(false)) => Ok(false),
        Some(Value::Bool(true)) => Err(schema(
            "Cargo unit graph",
            "build-std/sysroot Cargo unit is unsupported",
        )),
        Some(
            Value::Null | Value::Number(_) | Value::String(_) | Value::Array(_) | Value::Object(_),
        ) => Err(schema("Cargo unit graph", "unit is_std is not a boolean")),
    }
}

fn validate_profile(unit: &Value, expected: &str, command: &'static str) -> Result<()> {
    let actual = unit
        .get("profile")
        .and_then(|profile| profile.get("name"))
        .and_then(Value::as_str)
        .ok_or_else(|| schema("Cargo unit graph", "unit profile name is absent"))?;
    if actual == expected {
        Ok(())
    } else {
        Err(Error::ResolutionDrift {
            command,
            field: "selected unit profile",
        })
    }
}

#[cfg(test)]
fn parse_package(package: &Value) -> Result<(Arc<str>, Package)> {
    parse_package_budgeted(package, &mut JsonAllocation::empty(), &Limits::standard())
}

/// Transient complete package records, never a persisted format or authority.
pub(super) type MetadataRecords = Budgeted<Vec<Budgeted<Box<[u8]>>>>;

pub(super) fn compact_metadata_packages(
    document: &mut Budgeted<Value>,
    retain_records: bool,
    allocation: &mut JsonAllocation,
    limits: &Limits,
) -> Result<Option<MetadataRecords>> {
    document.transform(|value, document_ownership| {
        let packages = value
            .get_mut("packages")
            .and_then(Value::as_array_mut)
            .ok_or_else(|| schema("Cargo metadata", "packages are invalid"))?;
        let mut records = Vec::new();
        let mut retained = JsonOwnership::empty();
        if retain_records {
            let bytes = packages
                .len()
                .checked_mul(std::mem::size_of::<Budgeted<Box<[u8]>>>())
                .ok_or_else(|| schema("Cargo metadata", "package record storage overflow"))?;
            retained = JsonOwnership::collection()
                .checked_add(JsonOwnership::nodes(packages.len()))?
                .checked_add(JsonOwnership::buffer(bytes))?;
            allocation.acquire(retained, limits)?;
            records
                .try_reserve_exact(packages.len())
                .map_err(|source| Error::Allocation {
                    operation: "Cargo metadata",
                    source,
                })?;
        }
        for package in packages {
            // Save every field, including unknown fields, before discarding the
            // DOM fields that typed identity construction does not consume.
            if retain_records {
                let record = canonical_json_bytes_budgeted(
                    "Cargo metadata package record",
                    package,
                    allocation,
                    limits,
                )?;
                retained = retained.checked_add(record.ownership())?;
                records.push(record);
            }
            let object = package
                .as_object_mut()
                .ok_or_else(|| schema("Cargo metadata", "package is not an object"))?;
            let keep = |key: &str| {
                matches!(
                    key,
                    "id" | "name" | "version" | "manifest_path" | "source" | "checksum"
                )
            };
            let mut discarded = JsonOwnership::empty();
            for (key, value) in object.iter().filter(|(key, _)| !keep(key)) {
                discarded = discarded.checked_add(JsonOwnership::object_field(key, value)?)?;
            }
            object.retain(|key, _| keep(key));
            allocation.release(discarded)?;
            *document_ownership = document_ownership.checked_sub(discarded)?;
        }
        Ok(retain_records.then(|| Budgeted::from_retained(records, retained)))
    })
}

fn parse_package_budgeted(
    package: &Value,
    allocation: &mut JsonAllocation,
    limits: &Limits,
) -> Result<(Arc<str>, Package)> {
    let package_id = value_string(package, "id")
        .ok_or_else(|| schema("Cargo metadata", "package ID is absent"))?;
    let package_name = value_string(package, "name")
        .ok_or_else(|| schema("Cargo metadata", "package name is absent"))?;
    let package_version = value_string(package, "version")
        .ok_or_else(|| schema("Cargo metadata", "package version is absent"))?;
    let manifest_value = value_string(package, "manifest_path")
        .ok_or_else(|| schema("Cargo metadata", "package manifest path is absent"))?;
    allocation.nodes("Cargo metadata package nodes", 1, limits)?;
    for bytes in [
        package_id.len(),
        package_name.len(),
        package_version.len(),
        manifest_value.len().saturating_mul(2),
    ] {
        allocation.retained_string("Cargo metadata retained string bytes", bytes, limits)?;
    }
    let manifest = canonical_file("Cargo metadata package manifest", Path::new(manifest_value))?;
    let root = manifest
        .parent()
        .ok_or_else(|| schema("Cargo metadata", "package manifest has no root"))?
        .to_path_buf();
    let checksum = package.get("checksum").unwrap_or(&Value::Null);
    let source = match package.get("source") {
        Some(Value::Null) => {
            if !checksum.is_null() {
                return Err(schema(
                    "Cargo metadata",
                    "path package source/checksum does not match its PackageId",
                ));
            }
            let cargo_id = checked_path_package_id(
                package_id,
                package_name,
                package_version,
                &root,
                allocation,
                limits,
            )?;
            allocation.retained_string(
                "Cargo metadata retained string bytes",
                cargo_id.len(),
                limits,
            )?;
            PackageSource::Path {
                cargo_id: cargo_id.into(),
            }
        }
        Some(Value::String(source)) if source.starts_with("registry+") => {
            checked_nonpath_package_id(package_id, source, package_name, package_version)?;
            allocation.retained_string(
                "Cargo metadata retained string bytes",
                source.len(),
                limits,
            )?;
            let checksum = match checksum {
                Value::Null => None,
                Value::String(checksum) => Some(decode_checksum(checksum)?),
                Value::Bool(_) | Value::Number(_) | Value::Array(_) | Value::Object(_) => {
                    return Err(schema(
                        "Cargo metadata",
                        "registry package checksum is malformed",
                    ));
                }
            };
            PackageSource::Registry {
                cargo_id: source.as_str().into(),
                checksum,
            }
        }
        Some(Value::String(source)) if source.starts_with("git+") => {
            if !checksum.is_null() {
                return Err(schema(
                    "Cargo metadata",
                    "Git source/checksum does not match its PackageId",
                ));
            }
            let (source_base, oid) = source
                .rsplit_once('#')
                .ok_or_else(|| schema("Cargo metadata", "Git source has no precise revision"))?;
            if oid.len() != 40
                || !oid
                    .bytes()
                    .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
            {
                return Err(schema(
                    "Cargo metadata",
                    "Git source revision is not an exact OID",
                ));
            }
            checked_nonpath_package_id(package_id, source_base, package_name, package_version)?;
            allocation.retained_string(
                "Cargo metadata retained string bytes",
                source.len().saturating_add(oid.len()),
                limits,
            )?;
            PackageSource::Git {
                cargo_id: source.as_str().into(),
                oid: oid.into(),
            }
        }
        Some(Value::String(_)) | Some(_) | None => {
            return Err(schema("Cargo metadata", "unknown package source shape"));
        }
    };
    let package_id: Arc<str> = package_id.into();
    Ok((
        Arc::clone(&package_id),
        Package {
            id: package_id,
            name: package_name.into(),
            version: package_version.into(),
            manifest,
            root: root.into(),
            source,
            features: None,
        },
    ))
}

fn checked_nonpath_package_id(
    package_id: &str,
    source_base: &str,
    name: &str,
    version: &str,
) -> Result<()> {
    let suffix = package_id
        .strip_prefix(source_base)
        .and_then(|suffix| suffix.strip_prefix('#'))
        .ok_or_else(|| {
            schema(
                "Cargo metadata",
                "source/name/version do not reconstruct the exact PackageId",
            )
        })?;
    let named = suffix
        .split_once('@')
        .is_some_and(|(actual_name, actual_version)| {
            actual_name == name && actual_version == version
        });
    let compact_git = suffix == version
        && source_base.starts_with("git+")
        && source_base
            .strip_prefix("git+")
            .and_then(|source| source.split('?').next())
            .and_then(|source| source.trim_end_matches('/').rsplit('/').next())
            .map(|repository| repository.trim_end_matches(".git"))
            == Some(name);
    let exact = named || compact_git;
    if exact {
        Ok(())
    } else {
        Err(schema(
            "Cargo metadata",
            "source/name/version do not reconstruct the exact PackageId",
        ))
    }
}

fn checked_path_package_id<'package>(
    package_id: &'package str,
    name: &str,
    version: &str,
    root: &Path,
    allocation: &mut JsonAllocation,
    limits: &Limits,
) -> Result<&'package str> {
    let (source_base, suffix) = package_id
        .rsplit_once('#')
        .ok_or_else(|| schema("Cargo metadata", "path PackageId suffix is absent"))?;
    let named = suffix
        .split_once('@')
        .is_some_and(|(actual_name, actual_version)| {
            actual_name == name && actual_version == version
        });
    let compact = suffix == version
        && root
            .file_name()
            .and_then(std::ffi::OsStr::to_str)
            .is_some_and(|directory| directory == name);
    if !named && !compact {
        return Err(schema(
            "Cargo metadata",
            "path PackageId has the wrong exact Cargo name/version suffix",
        ));
    }
    let encoded = source_base
        .strip_prefix("path+file://")
        .ok_or_else(|| schema("Cargo metadata", "path PackageId is not a file URL"))?;
    if !encoded.starts_with('/') || encoded.contains(['?', '#']) {
        return Err(schema(
            "Cargo metadata",
            "path PackageId file URL is not an absolute local path",
        ));
    }
    let decoded = decode_file_url_path(encoded, allocation, limits)?;
    if decoded != root.as_os_str() {
        return Err(schema(
            "Cargo metadata",
            "path PackageId file URL does not identify the canonical package root",
        ));
    }
    Ok(source_base)
}

fn decode_file_url_path(
    value: &str,
    allocation: &mut JsonAllocation,
    limits: &Limits,
) -> Result<std::ffi::OsString> {
    allocation.nodes("Cargo file-URL decoder bytes", value.len(), limits)?;
    let mut decoded = Vec::new();
    decoded
        .try_reserve_exact(value.len())
        .map_err(|source| Error::Allocation {
            operation: "Cargo metadata",
            source,
        })?;
    let bytes = value.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            let pair = bytes
                .get(index + 1..index + 3)
                .ok_or_else(|| schema("Cargo metadata", "path PackageId has truncated escaping"))?;
            let text = std::str::from_utf8(pair)
                .map_err(|_| schema("Cargo metadata", "path PackageId escape is not ASCII"))?;
            decoded.push(u8::from_str_radix(text, 16).map_err(|_| {
                schema("Cargo metadata", "path PackageId escape is not hexadecimal")
            })?);
            index += 3;
        } else {
            if bytes[index] == b'\\' || bytes[index] == 0 {
                return Err(schema(
                    "Cargo metadata",
                    "path PackageId contains an invalid file-URL byte",
                ));
            }
            decoded.push(bytes[index]);
            index += 1;
        }
    }
    #[cfg(unix)]
    {
        Ok(std::ffi::OsString::from_vec(decoded))
    }
    #[cfg(not(unix))]
    {
        String::from_utf8(decoded)
            .map(std::ffi::OsString::from)
            .map_err(|_| schema("Cargo metadata", "path PackageId file URL is not UTF-8"))
    }
}

fn decode_checksum(value: &str) -> Result<[u8; 32]> {
    if value.len() != 64 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(schema(
            "Cargo metadata",
            "registry checksum is not 64 hexadecimal digits",
        ));
    }
    let mut checksum = [0_u8; 32];
    for (index, pair) in value.as_bytes().as_chunks::<2>().0.iter().enumerate() {
        let text = std::str::from_utf8(pair)
            .map_err(|_| schema("Cargo metadata", "registry checksum is not ASCII"))?;
        checksum[index] = u8::from_str_radix(text, 16)
            .map_err(|_| schema("Cargo metadata", "registry checksum is malformed"))?;
    }
    Ok(checksum)
}

fn validate_declaration(
    dependency: &Selected,
    declarations: &[Value],
    allocation: &mut JsonAllocation,
    limits: &Limits,
) -> Result<()> {
    let mut matches = declarations.iter().filter(|declaration| {
        let name = value_string(declaration, "name");
        let rename = declaration.get("rename").and_then(Value::as_str);
        rename.or(name) == Some(dependency.alias())
    });
    let declaration = matches.next().ok_or(Error::ResolutionDrift {
        command: "metadata",
        field: "dependency declaration alias",
    })?;
    if matches.next().is_some() {
        return Err(Error::ResolutionDrift {
            command: "metadata",
            field: "duplicate dependency declaration alias",
        });
    }
    let expected_package = dependency.package().unwrap_or(dependency.alias());
    if value_string(declaration, "name") != Some(expected_package) {
        return Err(Error::ResolutionDrift {
            command: "metadata",
            field: "dependency package rename",
        });
    }
    let default_features = declaration
        .get("uses_default_features")
        .and_then(Value::as_bool)
        .ok_or_else(|| schema("Cargo metadata", "default-feature policy is absent"))?;
    if default_features != dependency.default_features() {
        return Err(Error::ResolutionDrift {
            command: "metadata",
            field: "dependency default features",
        });
    }
    let feature_values = declaration
        .get("features")
        .and_then(Value::as_array)
        .ok_or_else(|| schema("Cargo metadata", "dependency features are absent"))?;
    allocation.nodes(
        "Cargo metadata declaration-feature nodes",
        feature_values.len(),
        limits,
    )?;
    let mut features = Vec::new();
    features
        .try_reserve_exact(feature_values.len())
        .map_err(|source| Error::Allocation {
            operation: "Cargo metadata",
            source,
        })?;
    for feature in feature_values {
        features.push(
            feature
                .as_str()
                .ok_or_else(|| schema("Cargo metadata", "dependency feature is not a string"))?,
        );
    }
    features.sort_unstable();
    if features.len() != dependency.features().len()
        || features
            .iter()
            .zip(dependency.features())
            .any(|(actual, expected)| *actual != expected.as_ref())
    {
        return Err(Error::ResolutionDrift {
            command: "metadata",
            field: "dependency feature request",
        });
    }
    match dependency.source() {
        Source::Path(path) => {
            let declared =
                declaration
                    .get("path")
                    .and_then(Value::as_str)
                    .ok_or(Error::ResolutionDrift {
                        command: "metadata",
                        field: "path dependency source",
                    })?;
            let declared = super::canonical_directory("metadata dependency", Path::new(declared))?;
            if &declared != path {
                return Err(Error::ResolutionDrift {
                    command: "metadata",
                    field: "path dependency source",
                });
            }
        }
        Source::Registry { requirement, .. } => {
            let actual = value_string(declaration, "req")
                .ok_or(Error::ResolutionDrift {
                    command: "metadata",
                    field: "registry version requirement",
                })
                .and_then(|actual| {
                    VersionReq::parse(actual).map_err(|_| Error::ResolutionDrift {
                        command: "metadata",
                        field: "registry version requirement",
                    })
                })?;
            if &actual != requirement {
                return Err(Error::ResolutionDrift {
                    command: "metadata",
                    field: "registry version requirement",
                });
            }
        }
        Source::Git { url, rev } => {
            if value_string(declaration, "source")
                .is_none_or(|source| !git_source_matches(source, url, rev))
            {
                return Err(Error::ResolutionDrift {
                    command: "metadata",
                    field: "Git dependency source",
                });
            }
        }
    }
    Ok(())
}

fn validate_package(dependency: &Selected, package: &Value) -> Result<()> {
    let expected_name = dependency.package().unwrap_or(dependency.alias());
    if value_string(package, "name") != Some(expected_name) {
        return Err(Error::ResolutionDrift {
            command: "metadata",
            field: "selected package name",
        });
    }
    match dependency.source() {
        Source::Path(path) => {
            if !package.get("source").is_some_and(Value::is_null) {
                return Err(Error::ResolutionDrift {
                    command: "metadata",
                    field: "selected path package source",
                });
            }
            let manifest = package
                .get("manifest_path")
                .and_then(Value::as_str)
                .ok_or_else(|| schema("Cargo metadata", "selected manifest path is absent"))?;
            let manifest = canonical_file("selected manifest", Path::new(manifest))?;
            if manifest.parent() != Some(path.as_path()) {
                return Err(Error::ResolutionDrift {
                    command: "metadata",
                    field: "selected path package identity",
                });
            }
        }
        Source::Registry { .. } => {
            if value_string(package, "source").is_none_or(|source| !source.starts_with("registry+"))
            {
                return Err(Error::ResolutionDrift {
                    command: "metadata",
                    field: "selected registry package source",
                });
            }
        }
        Source::Git { url, rev } => {
            let Some(source) = value_string(package, "source") else {
                return Err(Error::ResolutionDrift {
                    command: "metadata",
                    field: "selected Git package source",
                });
            };
            if !git_source_matches(source, url, rev) {
                return Err(Error::ResolutionDrift {
                    command: "metadata",
                    field: "selected Git package revision",
                });
            }
        }
    }
    Ok(())
}

#[derive(Clone, Copy, Debug)]
pub(super) struct JsonAllocation {
    items: usize,
    total_bytes: usize,
    values: usize,
    members: usize,
    elements: usize,
    strings: usize,
    containers: usize,
    bytes: usize,
    /// Process-unique account identity for exceedance reporting.
    id: u64,
    exceeded: crate::exceedance::Record,
    #[cfg(test)]
    peak: JsonPeak,
}

const AGGREGATE_ITEMS: &str = "Cargo aggregate retained items";
const AGGREGATE_BYTES: &str = "Cargo aggregate retained bytes";
const RETAINED_VALUES: &str = "Cargo JSON retained values";
const RETAINED_MEMBERS: &str = "Cargo JSON retained members";
const RETAINED_ELEMENTS: &str = "Cargo JSON retained elements";
const RETAINED_STRINGS: &str = "Cargo JSON retained string bytes";
const RETAINED_CONTAINERS: &str = "Cargo JSON retained containers";
const RETAINED_BUFFER: &str = "Cargo cumulative retained buffer bytes";
const JSON_DOCUMENT_BYTES: &str = "Cargo JSON bytes";

/// An account that records accounting-target crossings measured on its
/// behalf. Cargo document parsers compare a string or count against a target
/// and hand the measurement to the account that owns the parsed work instead
/// of refusing it.
pub(crate) trait Measure {
    /// Record `actual` against `target` for `resource` when it crosses. The
    /// work was already admitted and continues; nothing refuses or latches.
    fn measured(&mut self, resource: &'static str, target: usize, actual: usize);
}

impl Measure for JsonAllocation {
    fn measured(&mut self, resource: &'static str, target: usize, actual: usize) {
        self.exceeded(resource, target, actual);
    }
}

/// A detached record for a measuring site that cannot borrow the owning
/// account; the owner folds it in afterwards (`JsonAllocation::record`).
impl Measure for crate::exceedance::Record {
    fn measured(&mut self, resource: &'static str, target: usize, actual: usize) {
        if actual > target {
            self.exceeded(resource, target as u64, actual as u64, None);
        }
    }
}

/// Exact logical ownership transferred into or out of one JSON allocation epoch.
///
/// This is a charge delta, never an independently spendable limit authority.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(super) struct JsonOwnership {
    items: usize,
    total_bytes: usize,
    values: usize,
    members: usize,
    elements: usize,
    strings: usize,
    containers: usize,
    bytes: usize,
}

/// One value whose exact logical ownership is already present in the epoch.
pub(super) struct Budgeted<T> {
    value: T,
    ownership: JsonOwnership,
}

#[cfg(test)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct JsonPeak {
    items: usize,
    total_bytes: usize,
    strings: usize,
    containers: usize,
}

impl Allocation {
    /// Admit transferred transport ownership separately from the parsed JSON tree.
    pub(super) fn retain_transport(&mut self, bytes: Box<[u8]>) -> Result<Transport> {
        self.retained
            .acquire(JsonOwnership::buffer(bytes.len()), &self.limits)?;
        Ok(Transport(bytes))
    }

    /// Begin the one allocation epoch that accompanies a resolution attempt.
    pub(crate) fn begin(limits: &Limits) -> Self {
        Self {
            retained: JsonAllocation::empty(),
            limits: *limits,
            epoch: Arc::new(()),
            provisional: None,
        }
    }

    /// Open the only temporary ownership interval in a fresh resolution.
    pub(super) fn begin_provisional(&mut self) -> Result<Provisional> {
        if self.provisional.is_some() {
            return Err(Error::Request {
                field: "Cargo provisional allocation scope",
                reason: "is already active".into(),
            });
        }
        let scope = Arc::new(());
        self.provisional = Some(Arc::clone(&scope));
        Ok(Provisional {
            epoch: Arc::downgrade(&self.epoch),
            scope: Some(scope),
            retained: self.retained.retained_ownership(),
        })
    }

    /// Consume one provisional checkpoint after every value born inside it has
    /// been dropped, releasing exactly that interval's still-charged delta.
    pub(super) fn release_provisional(&mut self, checkpoint: &mut Provisional) -> Result<()> {
        let scope = checkpoint.scope.take().ok_or_else(|| Error::Request {
            field: "Cargo provisional allocation scope",
            reason: "was already consumed".into(),
        })?;
        if !Weak::ptr_eq(&checkpoint.epoch, &Arc::downgrade(&self.epoch)) {
            return Err(Error::Request {
                field: "Cargo provisional allocation scope",
                reason: "belongs to a different allocation epoch".into(),
            });
        }
        let active = self.provisional.as_ref().ok_or_else(|| Error::Request {
            field: "Cargo provisional allocation scope",
            reason: "is not active".into(),
        })?;
        if !Arc::ptr_eq(active, &scope) {
            return Err(Error::Request {
                field: "Cargo provisional allocation scope",
                reason: "was completed out of order".into(),
            });
        }
        let delta = self
            .retained
            .retained_ownership()
            .checked_sub(checkpoint.retained)?;
        self.retained.release(delta)?;
        self.provisional = None;
        Ok(())
    }

    pub(super) fn require(&self, limits: &Limits) -> Result<()> {
        let expected = &self.limits;
        if expected.json_bytes == limits.json_bytes
            && expected.manifest_bytes == limits.manifest_bytes
            && expected.packages == limits.packages
            && expected.units == limits.units
            && expected.edges == limits.edges
            && expected.items == limits.items
            && expected.containers == limits.containers
            && expected.string_bytes == limits.string_bytes
            && expected.depth == limits.depth
        {
            Ok(())
        } else {
            Err(Error::Request {
                field: "Cargo allocation epoch",
                reason: "was opened under a different limits profile".into(),
            })
        }
    }

    #[cfg(test)]
    fn usage(&self) -> JsonAllocation {
        self.retained
    }
}

struct JsonSeed<'limits> {
    allocation: &'limits mut JsonAllocation,
    limits: &'limits Limits,
    depth: usize,
}

struct JsonStringSeed<'limits> {
    allocation: &'limits mut JsonAllocation,
    limits: &'limits Limits,
}

#[cfg(test)]
pub(super) fn preflight(
    document: &'static str,
    bytes: &[u8],
    limits: &Limits,
) -> Result<JsonAllocation> {
    preflight_from(document, bytes, limits, JsonAllocation::empty())
}

fn preflight_from(
    document: &'static str,
    bytes: &[u8],
    limits: &Limits,
    mut allocation: JsonAllocation,
) -> Result<JsonAllocation> {
    let result = (|| {
        allocation.exceeded(JSON_DOCUMENT_BYTES, limits.json_bytes, bytes.len());
        preflight_raw_string_bytes(bytes, limits, &mut allocation)?;
        let mut deserializer = serde_json::Deserializer::from_slice(bytes);
        JsonSeed {
            allocation: &mut allocation,
            limits,
            depth: 0,
        }
        .deserialize(&mut deserializer)
        .map_err(|error| Error::Schema {
            document,
            reason: error.to_string().into_boxed_str(),
        })?;
        deserializer.end().map_err(|error| Error::Schema {
            document,
            reason: error.to_string().into_boxed_str(),
        })
    })();
    #[cfg(test)]
    allocation.record_peak();
    result.map(|()| allocation)
}

fn parse_owned_json(document: &'static str, bytes: &[u8]) -> Result<Value> {
    let value = serde_json::from_slice(bytes).map_err(|error| Error::Schema {
        document,
        reason: error.to_string().into_boxed_str(),
    })?;
    #[cfg(test)]
    OWNED_JSON_VALUES.with(|owned| owned.set(owned.get().saturating_add(1)));
    Ok(value)
}

/// Decode one already byte-bounded projection part into this same allocation
/// epoch. The streaming preflight charges the complete tree before serde owns
/// it, so callers can replace a retained byte buffer without post-allocation
/// reconciliation or a second live document ledger.
pub(super) fn parse_budgeted_json(
    document: &'static str,
    bytes: &[u8],
    limits: &Limits,
    allocation: &mut JsonAllocation,
) -> Result<Budgeted<Value>> {
    let next = preflight_from(document, bytes, limits, *allocation)?;
    let value = parse_owned_json(document, bytes)?;
    let ownership = JsonOwnership::tree(&value)?;
    let expected = allocation.retained_ownership().checked_add(ownership)?;
    if next.retained_ownership() != expected {
        return Err(schema(
            document,
            "preflight ownership disagrees with the decoded JSON tree",
        ));
    }
    *allocation = next;
    Ok(Budgeted::from_retained(value, ownership))
}

/// Parse one additional private proof projection under the same raw JSON
/// preflight authority used by Cargo metadata and unit-graph decoding.
#[cfg(test)]
pub(crate) fn bounded_json(document: &'static str, bytes: &[u8], limits: &Limits) -> Result<Value> {
    let _allocation = preflight_from(document, bytes, limits, JsonAllocation::empty())?;
    parse_owned_json(document, bytes)
}

/// Canonicalize one locked dependency-free Cargo metadata result under the
/// ordinary JSON bounds. This intentionally admits only the first DAG slice:
/// one root package whose complete resolution authority is its manifest and
/// lock file.
pub(super) fn canonical_leaf_metadata(
    bytes: &[u8],
    manifest: &Path,
    project: &Path,
    limits: &Limits,
) -> Result<Box<[u8]>> {
    let preflight = preflight_from(
        "locked leaf Cargo metadata",
        bytes,
        limits,
        JsonAllocation::empty(),
    )?;
    let value = parse_owned_json("locked leaf Cargo metadata", bytes)?;
    validate_leaf_metadata(&value, manifest, project)?;
    canonicalize_owned_metadata(value, preflight, limits)
}

/// Revalidate bytes already emitted by [`canonical_leaf_metadata`] without
/// requiring the pre-normalization absolute paths that were intentionally
/// removed from the stored value.
pub(super) fn canonical_metadata_replay(bytes: &[u8], limits: &Limits) -> Result<Box<[u8]>> {
    let preflight = preflight_from(
        "stored canonical Cargo metadata",
        bytes,
        limits,
        JsonAllocation::empty(),
    )?;
    let value = parse_owned_json("stored canonical Cargo metadata", bytes)?;
    canonicalize_owned_metadata(value, preflight, limits)
}

/// Extract the exact package-manifest paths read by one Cargo metadata result.
/// The returned paths are evidence inputs for the path-bound cached resolution
/// node; the ordinary semantic parser still validates the document afterward.
pub(super) fn metadata_manifest_paths(bytes: &[u8], limits: &Limits) -> Result<Box<[PathBuf]>> {
    let mut preflight = preflight_from(
        "cached locked Cargo metadata",
        bytes,
        limits,
        JsonAllocation::empty(),
    )?;
    let value = parse_owned_json("cached locked Cargo metadata", bytes)?;
    let object = value
        .as_object()
        .ok_or_else(|| schema("cached locked Cargo metadata", "root is not an object"))?;
    let packages = array(object, "packages", "cached locked Cargo metadata")?;
    preflight.exceeded(
        "cached Cargo metadata packages",
        limits.packages,
        packages.len(),
    );
    let mut paths = Vec::new();
    paths
        .try_reserve_exact(packages.len())
        .map_err(|source| Error::Allocation {
            operation: "cached locked Cargo metadata",
            source,
        })?;
    let mut path_bytes = 0_usize;
    for package in packages {
        let package = package
            .as_object()
            .ok_or_else(|| schema("cached locked Cargo metadata", "package is not an object"))?;
        let path = string(package, "manifest_path", "cached locked Cargo metadata")?;
        path_bytes = owned_add(
            path_bytes,
            path.len(),
            "cached Cargo metadata manifest-path bytes",
        )?;
        preflight.exceeded(
            "cached Cargo metadata manifest-path bytes",
            limits.string_bytes,
            path_bytes,
        );
        let path = PathBuf::from(path);
        if !path.is_absolute() {
            return Err(schema(
                "cached locked Cargo metadata",
                "package manifest path is not absolute",
            ));
        }
        paths.push(path);
    }
    paths.sort_unstable();
    if paths.windows(2).any(|pair| pair[0] == pair[1]) {
        return Err(schema(
            "cached locked Cargo metadata",
            "package manifest paths are not unique",
        ));
    }
    Ok(paths.into_boxed_slice())
}

/// Admit the bounded transport shape before a Cargo unit graph can enter the
/// shared store. The existing metadata-bound parser still proves all semantic
/// unit, feature, role, and target facts after lookup.
pub(super) fn validate_unit_graph_transport(bytes: &[u8], limits: &Limits) -> Result<()> {
    let mut preflight = preflight_from(
        "cached Cargo unit graph",
        bytes,
        limits,
        JsonAllocation::empty(),
    )?;
    let value = parse_owned_json("cached Cargo unit graph", bytes)?;
    let object = value
        .as_object()
        .ok_or_else(|| schema("cached Cargo unit graph", "root is not an object"))?;
    let units = array(object, "units", "cached Cargo unit graph")?;
    preflight.exceeded("cached Cargo unit graph units", limits.units, units.len());
    let roots = array(object, "roots", "cached Cargo unit graph")?;
    if roots.is_empty() {
        return Err(schema("cached Cargo unit graph", "has no command roots"));
    }
    Ok(())
}

fn canonicalize_owned_metadata(
    value: Value,
    preflight: JsonAllocation,
    limits: &Limits,
) -> Result<Box<[u8]>> {
    let ownership = JsonOwnership::tree(&value)?;
    let mut allocation = preflight;
    let mut value = Budgeted::from_retained(value, ownership);
    let canonical = canonical_metadata_bytes_budgeted(&mut value, &mut allocation, limits)?;
    value.drop_from(&mut allocation)?;
    Ok(canonical.into_parts().0)
}

fn validate_leaf_metadata(value: &Value, manifest: &Path, project: &Path) -> Result<()> {
    let object = value
        .as_object()
        .ok_or_else(|| schema("locked leaf Cargo metadata", "root is not an object"))?;
    let packages = array(object, "packages", "locked leaf Cargo metadata")?;
    let [package] = packages else {
        return Err(schema(
            "locked leaf Cargo metadata",
            "resolution is not exactly one package",
        ));
    };
    let package = package.as_object().ok_or_else(|| {
        schema(
            "locked leaf Cargo metadata",
            "the root package is not an object",
        )
    })?;
    let package_id = string(package, "id", "locked leaf Cargo metadata")?;
    if string(package, "manifest_path", "locked leaf Cargo metadata")?
        != manifest
            .as_os_str()
            .to_str()
            .ok_or_else(|| schema("locked leaf Cargo metadata", "manifest path is not Unicode"))?
        || !array(package, "dependencies", "locked leaf Cargo metadata")?.is_empty()
        || string(object, "workspace_root", "locked leaf Cargo metadata")?
            != project.as_os_str().to_str().ok_or_else(|| {
                schema(
                    "locked leaf Cargo metadata",
                    "workspace root is not Unicode",
                )
            })?
    {
        return Err(schema(
            "locked leaf Cargo metadata",
            "root manifest, workspace, or dependency closure changed",
        ));
    }
    for field in ["workspace_members", "workspace_default_members"] {
        let members = array(object, field, "locked leaf Cargo metadata")?;
        if members.len() != 1 || members[0].as_str() != Some(package_id) {
            return Err(schema(
                "locked leaf Cargo metadata",
                "workspace membership is not the exact root package",
            ));
        }
    }
    let resolve = object
        .get("resolve")
        .and_then(Value::as_object)
        .ok_or_else(|| schema("locked leaf Cargo metadata", "resolve graph is absent"))?;
    if resolve.get("root").and_then(Value::as_str) != Some(package_id) {
        return Err(schema(
            "locked leaf Cargo metadata",
            "resolve root is not the exact package",
        ));
    }
    let nodes = array(resolve, "nodes", "locked leaf Cargo metadata")?;
    let [node] = nodes else {
        return Err(schema(
            "locked leaf Cargo metadata",
            "resolve graph is not exactly one node",
        ));
    };
    let node = node.as_object().ok_or_else(|| {
        schema(
            "locked leaf Cargo metadata",
            "the resolve node is not an object",
        )
    })?;
    if string(node, "id", "locked leaf Cargo metadata")? != package_id
        || !array(node, "dependencies", "locked leaf Cargo metadata")?.is_empty()
        || !array(node, "deps", "locked leaf Cargo metadata")?.is_empty()
        || !array(node, "features", "locked leaf Cargo metadata")?.is_empty()
    {
        return Err(schema(
            "locked leaf Cargo metadata",
            "resolve node has non-leaf authority",
        ));
    }
    Ok(())
}

fn preflight_raw_string_bytes(
    bytes: &[u8],
    limits: &Limits,
    allocation: &mut JsonAllocation,
) -> Result<()> {
    const RESOURCE: &str = "Cargo JSON preflight string bytes";
    let mut index = 0_usize;
    let mut strings = 0_usize;
    while index < bytes.len() {
        if bytes[index] != b'"' {
            index = index.saturating_add(1);
            continue;
        }
        index = index.saturating_add(1);
        while index < bytes.len() {
            match bytes[index] {
                b'"' => {
                    index = index.saturating_add(1);
                    break;
                }
                b'\\' => {
                    strings = owned_add(strings, 1, RESOURCE)?;
                    allocation.exceeded(RESOURCE, limits.string_bytes, strings);
                    index = index.saturating_add(1);
                    if index >= bytes.len() {
                        break;
                    }
                    let escaped = bytes[index];
                    strings = owned_add(strings, 1, RESOURCE)?;
                    allocation.exceeded(RESOURCE, limits.string_bytes, strings);
                    index = index.saturating_add(1);
                    if escaped == b'u' {
                        let remaining = bytes.len().saturating_sub(index);
                        let hexadecimal = remaining.min(4);
                        strings = owned_add(strings, hexadecimal, RESOURCE)?;
                        allocation.exceeded(RESOURCE, limits.string_bytes, strings);
                        index = index.saturating_add(hexadecimal);
                    }
                }
                _ => {
                    strings = owned_add(strings, 1, RESOURCE)?;
                    allocation.exceeded(RESOURCE, limits.string_bytes, strings);
                    index = index.saturating_add(1);
                }
            }
        }
    }
    Ok(())
}

impl JsonAllocation {
    pub(super) fn empty() -> Self {
        Self {
            items: 0,
            total_bytes: 0,
            values: 0,
            members: 0,
            elements: 0,
            strings: 0,
            containers: 0,
            bytes: 0,
            id: crate::exceedance::next_id(),
            exceeded: crate::exceedance::Record::new(),
            #[cfg(test)]
            peak: JsonPeak::default(),
        }
    }

    /// The axes of this epoch that crossed their targets so far.
    #[cfg(test)]
    pub(crate) const fn exceedances(&self) -> crate::exceedance::Record {
        self.exceeded
    }

    /// Process-unique identity of this JSON allocation epoch.
    #[cfg(test)]
    pub(crate) const fn id(&self) -> u64 {
        self.id
    }

    /// Crossing an accounting target is recorded and published, never
    /// refused. The charge has already been added to this epoch.
    fn exceeded(&mut self, resource: &'static str, target: usize, actual: usize) {
        if actual > target
            && self
                .exceeded
                .exceeded(resource, target as u64, actual as u64, None)
        {
            crate::exceedance::publish(
                crate::exceedance::Account::CargoJson,
                self.id,
                &self.exceeded,
            );
        }
    }

    /// Fold a detached record of this epoch's own work into the epoch and
    /// publish it. Nothing is reset or refunded.
    fn record(&mut self, record: &crate::exceedance::Record) {
        if record.is_exceeded() {
            self.exceeded.absorb(record);
            crate::exceedance::publish(
                crate::exceedance::Account::CargoJson,
                self.id,
                &self.exceeded,
            );
        }
    }

    #[cfg(test)]
    fn record_peak(&mut self) {
        self.peak.items = self.peak.items.max(self.items);
        self.peak.total_bytes = self.peak.total_bytes.max(self.total_bytes);
        self.peak.strings = self.peak.strings.max(self.strings);
        self.peak.containers = self.peak.containers.max(self.containers);
    }

    /// Retain one already-measured ownership delta before constructing it.
    pub(super) fn retain_with<T>(
        &mut self,
        ownership: JsonOwnership,
        limits: &Limits,
        build: impl FnOnce() -> Result<T>,
    ) -> Result<Budgeted<T>> {
        self.acquire(ownership, limits)?;
        match build() {
            Ok(value) => Ok(Budgeted { value, ownership }),
            Err(error) => {
                self.release(ownership)?;
                Err(error)
            }
        }
    }

    pub(super) fn clone_tree(&mut self, value: &Value, limits: &Limits) -> Result<Budgeted<Value>> {
        let ownership = JsonOwnership::tree(value)?;
        self.retain_with(ownership, limits, || Ok(value.clone()))
    }

    pub(super) fn acquire(&mut self, ownership: JsonOwnership, limits: &Limits) -> Result<()> {
        let next = Self {
            items: owned_add(self.items, ownership.items, AGGREGATE_ITEMS)?,
            total_bytes: owned_add(self.total_bytes, ownership.total_bytes, AGGREGATE_BYTES)?,
            values: owned_add(self.values, ownership.values, RETAINED_VALUES)?,
            members: owned_add(self.members, ownership.members, RETAINED_MEMBERS)?,
            elements: owned_add(self.elements, ownership.elements, RETAINED_ELEMENTS)?,
            strings: owned_add(self.strings, ownership.strings, RETAINED_STRINGS)?,
            containers: owned_add(self.containers, ownership.containers, RETAINED_CONTAINERS)?,
            bytes: owned_add(self.bytes, ownership.bytes, RETAINED_BUFFER)?,
            id: self.id,
            exceeded: self.exceeded,
            #[cfg(test)]
            peak: JsonPeak {
                items: self
                    .peak
                    .items
                    .max(self.items.saturating_add(ownership.items)),
                total_bytes: self
                    .peak
                    .total_bytes
                    .max(self.total_bytes.saturating_add(ownership.total_bytes)),
                strings: self
                    .peak
                    .strings
                    .max(self.strings.saturating_add(ownership.strings)),
                containers: self
                    .peak
                    .containers
                    .max(self.containers.saturating_add(ownership.containers)),
            },
        };
        *self = next;
        #[cfg(test)]
        self.record_peak();
        self.exceeded(AGGREGATE_ITEMS, limits.items, self.items);
        self.exceeded(AGGREGATE_BYTES, limits.json_bytes, self.total_bytes);
        self.exceeded(RETAINED_VALUES, limits.items, self.values);
        self.exceeded(RETAINED_MEMBERS, limits.items, self.members);
        self.exceeded(RETAINED_ELEMENTS, limits.items, self.elements);
        self.exceeded(RETAINED_STRINGS, limits.string_bytes, self.strings);
        self.exceeded(RETAINED_CONTAINERS, limits.containers, self.containers);
        self.exceeded(RETAINED_BUFFER, limits.json_bytes, self.bytes);
        Ok(())
    }

    pub(super) fn release(&mut self, ownership: JsonOwnership) -> Result<()> {
        self.items = subtract_owned(self.items, ownership.items)?;
        self.total_bytes = subtract_owned(self.total_bytes, ownership.total_bytes)?;
        self.values = subtract_owned(self.values, ownership.values)?;
        self.members = subtract_owned(self.members, ownership.members)?;
        self.elements = subtract_owned(self.elements, ownership.elements)?;
        self.strings = subtract_owned(self.strings, ownership.strings)?;
        self.containers = subtract_owned(self.containers, ownership.containers)?;
        self.bytes = subtract_owned(self.bytes, ownership.bytes)?;
        Ok(())
    }

    /// Replace one consumed representation with its pre-measured output in
    /// this same epoch. This is an ownership transfer, not a reset: prior
    /// evidence and the historical high-water marks remain intact.
    fn replace_owned(
        &mut self,
        consumed: JsonOwnership,
        replacement: JsonOwnership,
        limits: &Limits,
    ) -> Result<()> {
        let external = self.retained_ownership().checked_sub(consumed)?;
        let mut next = Self {
            items: external.items,
            total_bytes: external.total_bytes,
            values: external.values,
            members: external.members,
            elements: external.elements,
            strings: external.strings,
            containers: external.containers,
            bytes: external.bytes,
            id: self.id,
            exceeded: self.exceeded,
            #[cfg(test)]
            peak: self.peak,
        };
        next.acquire(replacement, limits)?;
        *self = next;
        Ok(())
    }

    pub(super) fn with_scratch<T>(
        &mut self,
        ownership: JsonOwnership,
        limits: &Limits,
        work: impl FnOnce() -> Result<T>,
    ) -> Result<T> {
        self.acquire(ownership, limits)?;
        let outcome = work();
        self.release(ownership)?;
        outcome
    }

    /// Run one ownership transformation against this epoch. On error, release
    /// the consumed input and every still-live allocation acquired by `work`;
    /// ownership outside the transformation is neither reset nor recharged.
    pub(super) fn transform_owned<T>(
        &mut self,
        consumed: JsonOwnership,
        work: impl FnOnce(&mut Self) -> Result<T>,
    ) -> Result<T> {
        let external = self.retained_ownership().checked_sub(consumed)?;
        match work(self) {
            Ok(value) => Ok(value),
            Err(error) => {
                let remaining = self.retained_ownership().checked_sub(external)?;
                self.release(remaining)?;
                Err(error)
            }
        }
    }

    /// Consume one owned input and discard all transformation-local ownership,
    /// retaining only the returned output bundle in this same epoch.
    #[cfg(test)]
    pub(super) fn transform_owned_retaining<T>(
        &mut self,
        consumed: JsonOwnership,
        work: impl FnOnce(&mut Self) -> Result<Budgeted<T>>,
    ) -> Result<Budgeted<T>> {
        let external = self.retained_ownership().checked_sub(consumed)?;
        match work(self) {
            Ok(output) => {
                let temporary = self
                    .retained_ownership()
                    .checked_sub(external)?
                    .checked_sub(output.ownership())?;
                self.release(temporary)?;
                Ok(output)
            }
            Err(error) => {
                let remaining = self.retained_ownership().checked_sub(external)?;
                self.release(remaining)?;
                Err(error)
            }
        }
    }

    fn retained_ownership(&self) -> JsonOwnership {
        JsonOwnership {
            items: self.items,
            total_bytes: self.total_bytes,
            values: self.values,
            members: self.members,
            elements: self.elements,
            strings: self.strings,
            containers: self.containers,
            bytes: self.bytes,
        }
    }

    fn value<E: serde::de::Error>(&mut self, limits: &Limits) -> std::result::Result<(), E> {
        let values = self
            .values
            .checked_add(1)
            .ok_or_else(|| E::custom("JSON value count overflow"))?;
        let items = self
            .items
            .checked_add(1)
            .ok_or_else(|| E::custom("JSON aggregate item count overflow"))?;
        self.values = values;
        self.items = items;
        self.exceeded(RETAINED_VALUES, limits.items, values);
        self.exceeded(AGGREGATE_ITEMS, limits.items, items);
        Ok(())
    }

    fn string<E: serde::de::Error>(
        &mut self,
        limits: &Limits,
        bytes: usize,
    ) -> std::result::Result<(), E> {
        let strings = self
            .strings
            .checked_add(bytes)
            .ok_or_else(|| E::custom("JSON string bytes overflow"))?;
        let total_bytes = self
            .total_bytes
            .checked_add(bytes)
            .ok_or_else(|| E::custom("JSON aggregate bytes overflow"))?;
        self.strings = strings;
        self.total_bytes = total_bytes;
        self.exceeded(RETAINED_STRINGS, limits.string_bytes, bytes.max(strings));
        self.exceeded(AGGREGATE_BYTES, limits.json_bytes, total_bytes);
        Ok(())
    }

    fn container<E: serde::de::Error>(
        &mut self,
        limits: &Limits,
        depth: usize,
    ) -> std::result::Result<(), E> {
        let containers = self
            .containers
            .checked_add(1)
            .ok_or_else(|| E::custom("JSON container count overflow"))?;
        let items = self
            .items
            .checked_add(1)
            .ok_or_else(|| E::custom("JSON aggregate item count overflow"))?;
        // `depth` bounds serde recursion and is physical: it stays a refusal.
        if depth > limits.depth {
            return Err(E::custom(
                "Cargo JSON containers exceed the recursion depth limit",
            ));
        }
        self.containers = containers;
        self.items = items;
        self.exceeded(RETAINED_CONTAINERS, limits.containers, containers);
        self.exceeded(AGGREGATE_ITEMS, limits.items, items);
        Ok(())
    }

    fn member<E: serde::de::Error>(&mut self, limits: &Limits) -> std::result::Result<(), E> {
        let members = self
            .members
            .checked_add(1)
            .ok_or_else(|| E::custom("JSON member count overflow"))?;
        let items = self
            .items
            .checked_add(1)
            .ok_or_else(|| E::custom("JSON aggregate item count overflow"))?;
        self.members = members;
        self.items = items;
        self.exceeded(RETAINED_MEMBERS, limits.items, members);
        self.exceeded(AGGREGATE_ITEMS, limits.items, items);
        Ok(())
    }

    fn element<E: serde::de::Error>(&mut self, limits: &Limits) -> std::result::Result<(), E> {
        let elements = self
            .elements
            .checked_add(1)
            .ok_or_else(|| E::custom("JSON element count overflow"))?;
        let items = self
            .items
            .checked_add(1)
            .ok_or_else(|| E::custom("JSON aggregate item count overflow"))?;
        self.elements = elements;
        self.items = items;
        self.exceeded(RETAINED_ELEMENTS, limits.items, elements);
        self.exceeded(AGGREGATE_ITEMS, limits.items, items);
        Ok(())
    }

    #[cfg(test)]
    pub(super) fn charge_tree(&mut self, value: &Value, limits: &Limits) -> Result<()> {
        let values = self
            .values
            .checked_add(1)
            .ok_or_else(|| overflow("Cargo JSON retained values"))?;
        let items = self
            .items
            .checked_add(1)
            .ok_or_else(|| overflow("Cargo JSON aggregate retained items"))?;
        self.exceeded("Cargo JSON retained values", limits.items, values);
        self.exceeded("Cargo JSON aggregate retained items", limits.items, items);
        self.values = values;
        self.items = items;
        match value {
            Value::String(string) => {
                let strings = self
                    .strings
                    .checked_add(string.len())
                    .ok_or_else(|| overflow("Cargo JSON retained string bytes"))?;
                let total_bytes = self
                    .total_bytes
                    .checked_add(string.len())
                    .ok_or_else(|| overflow("Cargo JSON aggregate retained bytes"))?;
                self.exceeded(
                    "Cargo JSON retained string bytes",
                    limits.string_bytes,
                    strings,
                );
                self.exceeded(
                    "Cargo JSON aggregate retained bytes",
                    limits.json_bytes,
                    total_bytes,
                );
                self.strings = strings;
                self.total_bytes = total_bytes;
            }
            Value::Array(values) => {
                let containers = self
                    .containers
                    .checked_add(1)
                    .ok_or_else(|| overflow("Cargo JSON retained containers"))?;
                let elements = self
                    .elements
                    .checked_add(values.len())
                    .ok_or_else(|| overflow("Cargo JSON retained elements"))?;
                let aggregate = self
                    .items
                    .checked_add(1_usize.saturating_add(values.len()))
                    .ok_or_else(|| overflow("Cargo JSON aggregate retained items"))?;
                self.exceeded(
                    "Cargo JSON retained containers",
                    limits.containers,
                    containers,
                );
                self.exceeded("Cargo JSON retained elements", limits.items, elements);
                self.exceeded(
                    "Cargo JSON aggregate retained items",
                    limits.items,
                    aggregate,
                );
                self.containers = containers;
                self.elements = elements;
                self.items = aggregate;
                for child in values {
                    self.charge_tree(child, limits)?;
                }
            }
            Value::Object(object) => {
                let containers = self
                    .containers
                    .checked_add(1)
                    .ok_or_else(|| overflow("Cargo JSON retained containers"))?;
                let members = self
                    .members
                    .checked_add(object.len())
                    .ok_or_else(|| overflow("Cargo JSON retained members"))?;
                let aggregate = self
                    .items
                    .checked_add(1_usize.saturating_add(object.len()))
                    .ok_or_else(|| overflow("Cargo JSON aggregate retained items"))?;
                self.exceeded(
                    "Cargo JSON retained containers",
                    limits.containers,
                    containers,
                );
                self.exceeded("Cargo JSON retained members", limits.items, members);
                self.exceeded(
                    "Cargo JSON aggregate retained items",
                    limits.items,
                    aggregate,
                );
                self.containers = containers;
                self.members = members;
                self.items = aggregate;
                for (key, child) in object {
                    let strings = self
                        .strings
                        .checked_add(key.len())
                        .ok_or_else(|| overflow("Cargo JSON retained string bytes"))?;
                    let total_bytes = self
                        .total_bytes
                        .checked_add(key.len())
                        .ok_or_else(|| overflow("Cargo JSON aggregate retained bytes"))?;
                    self.exceeded(
                        "Cargo JSON retained string bytes",
                        limits.string_bytes,
                        strings,
                    );
                    self.exceeded(
                        "Cargo JSON aggregate retained bytes",
                        limits.json_bytes,
                        total_bytes,
                    );
                    self.strings = strings;
                    self.total_bytes = total_bytes;
                    self.charge_tree(child, limits)?;
                }
            }
            Value::Null | Value::Bool(_) | Value::Number(_) => {}
        }
        #[cfg(test)]
        self.record_peak();
        Ok(())
    }

    #[cfg(test)]
    pub(super) const fn peak_usage(self) -> (usize, usize, usize, usize) {
        (
            self.peak.items,
            self.peak.total_bytes,
            self.peak.strings,
            self.peak.containers,
        )
    }

    #[cfg(test)]
    pub(super) const fn full_usage(self) -> (usize, usize, usize, usize) {
        (self.items, self.total_bytes, self.strings, self.containers)
    }

    fn nodes(&mut self, resource: &'static str, count: usize, limits: &Limits) -> Result<()> {
        let elements = owned_add(self.elements, count, resource)?;
        let items = owned_add(self.items, count, AGGREGATE_ITEMS)?;
        self.elements = elements;
        self.items = items;
        self.exceeded(resource, limits.items, elements);
        self.exceeded(AGGREGATE_ITEMS, limits.items, items);
        #[cfg(test)]
        self.record_peak();
        Ok(())
    }

    fn member_nodes(
        &mut self,
        resource: &'static str,
        count: usize,
        limits: &Limits,
    ) -> Result<()> {
        let members = owned_add(self.members, count, resource)?;
        let items = owned_add(self.items, count, AGGREGATE_ITEMS)?;
        self.members = members;
        self.items = items;
        self.exceeded(resource, limits.items, members);
        self.exceeded(AGGREGATE_ITEMS, limits.items, items);
        #[cfg(test)]
        self.record_peak();
        Ok(())
    }

    fn retained_string(
        &mut self,
        resource: &'static str,
        bytes: usize,
        limits: &Limits,
    ) -> Result<()> {
        let strings = owned_add(self.strings, bytes, resource)?;
        let items = owned_add(self.items, 1, AGGREGATE_ITEMS)?;
        let total_bytes = owned_add(self.total_bytes, bytes, AGGREGATE_BYTES)?;
        self.strings = strings;
        self.items = items;
        self.total_bytes = total_bytes;
        self.exceeded(resource, limits.string_bytes, strings);
        self.exceeded(AGGREGATE_ITEMS, limits.items, items);
        self.exceeded(AGGREGATE_BYTES, limits.json_bytes, total_bytes);
        #[cfg(test)]
        self.record_peak();
        Ok(())
    }

    fn retained_bytes(
        &mut self,
        resource: &'static str,
        bytes: usize,
        limit: usize,
        limits: &Limits,
    ) -> Result<()> {
        let retained_bytes = owned_add(self.bytes, bytes, RETAINED_BUFFER)?;
        let items = owned_add(self.items, 1, AGGREGATE_ITEMS)?;
        let total_bytes = owned_add(self.total_bytes, bytes, AGGREGATE_BYTES)?;
        self.bytes = retained_bytes;
        self.items = items;
        self.total_bytes = total_bytes;
        self.exceeded(resource, limit, bytes);
        self.exceeded(RETAINED_BUFFER, limits.json_bytes, retained_bytes);
        self.exceeded(AGGREGATE_ITEMS, limits.items, items);
        self.exceeded(AGGREGATE_BYTES, limits.json_bytes, total_bytes);
        #[cfg(test)]
        self.record_peak();
        Ok(())
    }
}

impl JsonOwnership {
    pub(super) const fn empty() -> Self {
        Self {
            items: 0,
            total_bytes: 0,
            values: 0,
            members: 0,
            elements: 0,
            strings: 0,
            containers: 0,
            bytes: 0,
        }
    }

    pub(super) fn tree(value: &Value) -> Result<Self> {
        let mut ownership = Self::empty();
        ownership.add_tree(value)?;
        Ok(ownership)
    }

    fn add_tree(&mut self, value: &Value) -> Result<()> {
        self.values = owned_sum(self.values, 1)?;
        self.items = owned_sum(self.items, 1)?;
        match value {
            Value::String(string) => {
                self.strings = owned_sum(self.strings, string.len())?;
                self.total_bytes = owned_sum(self.total_bytes, string.len())?;
            }
            Value::Array(values) => {
                self.containers = owned_sum(self.containers, 1)?;
                self.elements = owned_sum(self.elements, values.len())?;
                self.items = owned_sum(self.items, 1_usize.saturating_add(values.len()))?;
                for child in values {
                    self.add_tree(child)?;
                }
            }
            Value::Object(object) => {
                self.containers = owned_sum(self.containers, 1)?;
                self.members = owned_sum(self.members, object.len())?;
                self.items = owned_sum(self.items, 1_usize.saturating_add(object.len()))?;
                for (key, child) in object {
                    self.strings = owned_sum(self.strings, key.len())?;
                    self.total_bytes = owned_sum(self.total_bytes, key.len())?;
                    self.add_tree(child)?;
                }
            }
            Value::Null | Value::Bool(_) | Value::Number(_) => {}
        }
        Ok(())
    }

    pub(super) fn nodes(count: usize) -> Self {
        Self {
            items: count,
            elements: count,
            ..Self::empty()
        }
    }

    pub(super) fn members(count: usize) -> Self {
        Self {
            items: count,
            members: count,
            ..Self::empty()
        }
    }

    pub(super) fn collection() -> Self {
        Self {
            items: 1,
            containers: 1,
            ..Self::empty()
        }
    }

    pub(super) fn string(bytes: usize) -> Self {
        Self {
            items: 1,
            total_bytes: bytes,
            strings: bytes,
            ..Self::empty()
        }
    }

    pub(super) fn buffer(bytes: usize) -> Self {
        Self {
            items: 1,
            total_bytes: bytes,
            bytes,
            ..Self::empty()
        }
    }

    pub(super) fn string_bytes(bytes: usize) -> Self {
        Self {
            total_bytes: bytes,
            strings: bytes,
            ..Self::empty()
        }
    }

    pub(super) fn object_field(key: &str, value: &Value) -> Result<Self> {
        Self::members(1)
            .checked_add(Self::string_bytes(key.len()))?
            .checked_add(Self::tree(value)?)
    }

    pub(super) fn object_string_field(key: &str, value_bytes: usize) -> Result<Self> {
        Self::members(1)
            .checked_add(Self::string_bytes(key.len()))?
            .checked_add(Self {
                items: 1,
                values: 1,
                total_bytes: value_bytes,
                strings: value_bytes,
                ..Self::empty()
            })
    }

    pub(super) const fn object_key(key: &str) -> Self {
        Self {
            total_bytes: key.len(),
            strings: key.len(),
            ..Self::empty()
        }
    }

    pub(super) fn checked_add(self, other: Self) -> Result<Self> {
        Ok(Self {
            items: owned_sum(self.items, other.items)?,
            total_bytes: owned_sum(self.total_bytes, other.total_bytes)?,
            values: owned_sum(self.values, other.values)?,
            members: owned_sum(self.members, other.members)?,
            elements: owned_sum(self.elements, other.elements)?,
            strings: owned_sum(self.strings, other.strings)?,
            containers: owned_sum(self.containers, other.containers)?,
            bytes: owned_sum(self.bytes, other.bytes)?,
        })
    }

    pub(super) fn checked_sub(self, other: Self) -> Result<Self> {
        Ok(Self {
            items: subtract_owned(self.items, other.items)?,
            total_bytes: subtract_owned(self.total_bytes, other.total_bytes)?,
            values: subtract_owned(self.values, other.values)?,
            members: subtract_owned(self.members, other.members)?,
            elements: subtract_owned(self.elements, other.elements)?,
            strings: subtract_owned(self.strings, other.strings)?,
            containers: subtract_owned(self.containers, other.containers)?,
            bytes: subtract_owned(self.bytes, other.bytes)?,
        })
    }

    #[cfg(test)]
    pub(super) const fn usage(self) -> (usize, usize, usize, usize) {
        (self.items, self.total_bytes, self.strings, self.containers)
    }
}

impl<T> Budgeted<T> {
    pub(super) const fn from_retained(value: T, ownership: JsonOwnership) -> Self {
        Self { value, ownership }
    }

    pub(super) const fn value(&self) -> &T {
        &self.value
    }

    pub(super) const fn value_mut(&mut self) -> &mut T {
        &mut self.value
    }

    pub(super) const fn ownership(&self) -> JsonOwnership {
        self.ownership
    }

    pub(super) fn transform<R>(
        &mut self,
        work: impl FnOnce(&mut T, &mut JsonOwnership) -> Result<R>,
    ) -> Result<R> {
        work(&mut self.value, &mut self.ownership)
    }

    pub(super) fn into_parts(self) -> (T, JsonOwnership) {
        (self.value, self.ownership)
    }

    pub(super) fn drop_from(self, allocation: &mut JsonAllocation) -> Result<()> {
        let ownership = self.ownership;
        drop(self.value);
        allocation.release(ownership)
    }
}

/// Checked-arithmetic overflow is a terminal arithmetic failure, never an
/// accounting exceedance and never `Error::Limit`.
fn overflow(operation: &'static str) -> Error {
    Error::Node {
        operation,
        source: Box::new(crate::node::Error::Allocation),
    }
}

fn owned_add(current: usize, added: usize, resource: &'static str) -> Result<usize> {
    current.checked_add(added).ok_or_else(|| overflow(resource))
}

fn owned_sum(left: usize, right: usize) -> Result<usize> {
    owned_add(left, right, "Cargo JSON ownership measurement")
}

fn subtract_owned(current: usize, released: usize) -> Result<usize> {
    current.checked_sub(released).ok_or_else(|| Error::Schema {
        document: "Cargo JSON allocation epoch",
        reason: "released ownership was not retained by this epoch".into(),
    })
}

impl RawDocument {
    fn from_bytes(bytes: &[u8]) -> Self {
        Self {
            bytes: bytes.len(),
            digest: digest(bytes),
        }
    }

    fn require(self, bytes: &[u8], command: &'static str, field: &'static str) -> Result<()> {
        if self == Self::from_bytes(bytes) {
            Ok(())
        } else {
            Err(Error::ResolutionDrift { command, field })
        }
    }
}

impl<'de> DeserializeSeed<'de> for JsonSeed<'_> {
    type Value = ();

    fn deserialize<D: serde::Deserializer<'de>>(
        self,
        deserializer: D,
    ) -> std::result::Result<(), D::Error> {
        self.allocation.value::<D::Error>(self.limits)?;
        deserializer.deserialize_any(self)
    }
}

impl<'de> Visitor<'de> for JsonSeed<'_> {
    type Value = ();

    fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("a bounded JSON value")
    }

    fn visit_bool<E: serde::de::Error>(self, _value: bool) -> std::result::Result<(), E> {
        Ok(())
    }
    fn visit_i64<E: serde::de::Error>(self, _value: i64) -> std::result::Result<(), E> {
        Ok(())
    }
    fn visit_u64<E: serde::de::Error>(self, _value: u64) -> std::result::Result<(), E> {
        Ok(())
    }
    fn visit_f64<E: serde::de::Error>(self, _value: f64) -> std::result::Result<(), E> {
        Ok(())
    }
    fn visit_none<E: serde::de::Error>(self) -> std::result::Result<(), E> {
        Ok(())
    }
    fn visit_unit<E: serde::de::Error>(self) -> std::result::Result<(), E> {
        Ok(())
    }

    fn visit_borrowed_str<E: serde::de::Error>(
        self,
        value: &'de str,
    ) -> std::result::Result<(), E> {
        self.allocation.string::<E>(self.limits, value.len())
    }

    fn visit_str<E: serde::de::Error>(self, value: &str) -> std::result::Result<(), E> {
        self.allocation.string::<E>(self.limits, value.len())
    }

    fn visit_string<E: serde::de::Error>(self, value: String) -> std::result::Result<(), E> {
        self.allocation.string::<E>(self.limits, value.len())
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut sequence: A) -> std::result::Result<(), A::Error> {
        let depth = self
            .depth
            .checked_add(1)
            .ok_or_else(|| A::Error::custom("JSON depth overflow"))?;
        self.allocation.container::<A::Error>(self.limits, depth)?;
        loop {
            let seed = JsonSeed {
                allocation: self.allocation,
                limits: self.limits,
                depth,
            };
            match sequence.next_element_seed(seed)? {
                Some(()) => self.allocation.element::<A::Error>(self.limits)?,
                None => return Ok(()),
            }
        }
    }

    fn visit_map<A: MapAccess<'de>>(self, mut object: A) -> std::result::Result<(), A::Error> {
        let depth = self
            .depth
            .checked_add(1)
            .ok_or_else(|| A::Error::custom("JSON depth overflow"))?;
        self.allocation.container::<A::Error>(self.limits, depth)?;
        loop {
            let key = JsonStringSeed {
                allocation: self.allocation,
                limits: self.limits,
            };
            if object.next_key_seed(key)?.is_none() {
                return Ok(());
            }
            self.allocation.member::<A::Error>(self.limits)?;
            object.next_value_seed(JsonSeed {
                allocation: self.allocation,
                limits: self.limits,
                depth,
            })?;
        }
    }
}

impl<'de> DeserializeSeed<'de> for JsonStringSeed<'_> {
    type Value = ();

    fn deserialize<D: serde::Deserializer<'de>>(
        self,
        deserializer: D,
    ) -> std::result::Result<(), D::Error> {
        deserializer.deserialize_string(self)
    }
}

impl Visitor<'_> for JsonStringSeed<'_> {
    type Value = ();

    fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("a bounded JSON object key")
    }

    fn visit_borrowed_str<E: serde::de::Error>(self, value: &str) -> std::result::Result<(), E> {
        self.allocation.string::<E>(self.limits, value.len())
    }

    fn visit_str<E: serde::de::Error>(self, value: &str) -> std::result::Result<(), E> {
        self.allocation.string::<E>(self.limits, value.len())
    }

    fn visit_string<E: serde::de::Error>(self, value: String) -> std::result::Result<(), E> {
        self.allocation.string::<E>(self.limits, value.len())
    }
}

fn git_source_matches(source: &str, expected_url: &str, expected_rev: &str) -> bool {
    let Some(source) = source.strip_prefix("git+") else {
        return false;
    };
    let (location, precise) = source
        .split_once('#')
        .map_or((source, None), |(location, precise)| {
            (location, Some(precise))
        });
    let Some(marker) = location.rfind("?rev=") else {
        return false;
    };
    let url = &location[..marker];
    let requested = &location[marker + "?rev=".len()..];
    let requested = requested.split('&').next().unwrap_or(requested);
    url == expected_url
        && requested.eq_ignore_ascii_case(expected_rev)
        && precise.is_none_or(|precise| precise.eq_ignore_ascii_case(expected_rev))
}

fn canonical_metadata_bytes_budgeted(
    value: &mut Budgeted<Value>,
    allocation: &mut JsonAllocation,
    limits: &Limits,
) -> Result<Budgeted<Box<[u8]>>> {
    value.transform(|value, ownership| {
        normalize_metadata(value, ownership, allocation, limits)?;
        canonicalize_metadata_objects(value, allocation, limits)
    })?;
    canonical_json_bytes_budgeted("canonical Cargo JSON", value.value(), allocation, limits)
}

fn canonical_json_bytes_budgeted(
    document: &'static str,
    value: &Value,
    allocation: &mut JsonAllocation,
    limits: &Limits,
) -> Result<Budgeted<Box<[u8]>>> {
    struct Counter(usize);
    impl std::io::Write for Counter {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.0 = self
                .0
                .checked_add(bytes.len())
                .ok_or_else(|| std::io::Error::other("canonical JSON byte count overflow"))?;
            Ok(bytes.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    let mut counter = Counter(0);
    serde_json::to_writer(&mut counter, value).map_err(|error| Error::Schema {
        document,
        reason: error.to_string().into_boxed_str(),
    })?;
    allocation.exceeded("canonical Cargo JSON bytes", limits.json_bytes, counter.0);
    allocation.retain_with(JsonOwnership::buffer(counter.0), limits, || {
        let mut bytes = Vec::new();
        bytes
            .try_reserve_exact(counter.0)
            .map_err(|source| Error::Allocation {
                operation: document,
                source,
            })?;
        serde_json::to_writer(&mut bytes, value).map_err(|error| Error::Schema {
            document,
            reason: error.to_string().into_boxed_str(),
        })?;
        Ok(bytes.into_boxed_slice())
    })
}

#[derive(Clone, Copy)]
enum CanonicalPart<'value> {
    Value(&'value Value),
    Bool(bool),
    UnitKey(&'value [u8]),
    Dependencies {
        values: &'value [Value],
        keys: &'value [Budgeted<Box<[u8]>>],
    },
}

fn write_json(
    document: &'static str,
    writer: &mut dyn std::io::Write,
    value: &(impl serde::Serialize + ?Sized),
) -> Result<()> {
    serde_json::to_writer(writer, value).map_err(|error| Error::Schema {
        document,
        reason: error.to_string().into_boxed_str(),
    })
}

fn buffered_canonical(
    document: &'static str,
    allocation: &mut JsonAllocation,
    limits: &Limits,
    write: impl Fn(&mut dyn std::io::Write, &mut JsonAllocation) -> Result<()>,
) -> Result<Budgeted<Box<[u8]>>> {
    struct Counter(usize);
    impl std::io::Write for Counter {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.0 = self
                .0
                .checked_add(bytes.len())
                .ok_or_else(|| std::io::Error::other("canonical unit byte count overflow"))?;
            Ok(bytes.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    let mut counter = Counter(0);
    write(&mut counter, allocation)?;
    allocation.exceeded("canonical Cargo unit bytes", limits.json_bytes, counter.0);
    let ownership = JsonOwnership::buffer(counter.0);
    allocation.acquire(ownership, limits)?;
    let result = (|| {
        let mut bytes = Vec::new();
        bytes
            .try_reserve_exact(counter.0)
            .map_err(|source| Error::Allocation {
                operation: document,
                source,
            })?;
        write(&mut bytes, allocation)?;
        if bytes.len() != counter.0 {
            return Err(schema(
                document,
                "canonical byte count changed while writing",
            ));
        }
        Ok(bytes.into_boxed_slice())
    })();
    match result {
        Ok(bytes) => Ok(Budgeted::from_retained(bytes, ownership)),
        Err(error) => {
            allocation.release(ownership)?;
            Err(error)
        }
    }
}

fn write_sorted_canonical<'value>(
    writer: &mut dyn std::io::Write,
    values: impl ExactSizeIterator<Item = &'value Value>,
    allocation: &mut JsonAllocation,
    limits: &Limits,
) -> Result<()> {
    let count = values.len();
    let buffer_bytes = std::mem::size_of::<Budgeted<Box<[u8]>>>()
        .checked_mul(count)
        .ok_or_else(|| overflow("canonical Cargo unit sort buffer bytes"))?;
    let scratch = JsonOwnership::collection()
        .checked_add(JsonOwnership::nodes(count))?
        .checked_add(JsonOwnership::buffer(buffer_bytes))?;
    allocation.transform_owned(JsonOwnership::empty(), |allocation| {
        allocation.acquire(scratch, limits)?;
        let mut encoded = Vec::new();
        encoded
            .try_reserve_exact(count)
            .map_err(|source| Error::Allocation {
                operation: "canonical Cargo unit graph",
                source,
            })?;
        for value in values {
            encoded.push(buffered_canonical(
                "canonical Cargo unit value",
                allocation,
                limits,
                |writer, allocation| write_canonical_value(writer, value, allocation, limits),
            )?);
        }
        encoded.sort_unstable_by(|left, right| left.value().cmp(right.value()));
        writer
            .write_all(b"[")
            .map_err(|error| schema("canonical Cargo unit graph", &error.to_string()))?;
        for (index, value) in encoded.iter().enumerate() {
            if index != 0 {
                writer
                    .write_all(b",")
                    .map_err(|error| schema("canonical Cargo unit graph", &error.to_string()))?;
            }
            writer
                .write_all(value.value())
                .map_err(|error| schema("canonical Cargo unit graph", &error.to_string()))?;
        }
        writer
            .write_all(b"]")
            .map_err(|error| schema("canonical Cargo unit graph", &error.to_string()))?;
        for value in encoded {
            value.drop_from(allocation)?;
        }
        allocation.release(scratch)
    })
}

fn write_canonical_value(
    writer: &mut dyn std::io::Write,
    value: &Value,
    allocation: &mut JsonAllocation,
    limits: &Limits,
) -> Result<()> {
    match value {
        Value::Array(values) => write_sorted_canonical(writer, values.iter(), allocation, limits),
        Value::Object(object) => {
            writer
                .write_all(b"{")
                .map_err(|error| schema("canonical Cargo unit graph", &error.to_string()))?;
            for (index, (key, value)) in object.iter().enumerate() {
                if index != 0 {
                    writer.write_all(b",").map_err(|error| {
                        schema("canonical Cargo unit graph", &error.to_string())
                    })?;
                }
                write_json("canonical Cargo unit graph", writer, key)?;
                writer
                    .write_all(b":")
                    .map_err(|error| schema("canonical Cargo unit graph", &error.to_string()))?;
                write_canonical_value(writer, value, allocation, limits)?;
            }
            writer
                .write_all(b"}")
                .map_err(|error| schema("canonical Cargo unit graph", &error.to_string()))
        }
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => {
            write_json("canonical Cargo unit graph", writer, value)
        }
    }
}

fn write_part(
    writer: &mut dyn std::io::Write,
    part: CanonicalPart<'_>,
    allocation: &mut JsonAllocation,
    limits: &Limits,
) -> Result<()> {
    match part {
        CanonicalPart::Value(value) => write_canonical_value(writer, value, allocation, limits),
        CanonicalPart::Bool(value) => write_json("canonical Cargo unit graph", writer, &value),
        CanonicalPart::UnitKey(bytes) => writer
            .write_all(bytes)
            .map_err(|error| schema("canonical Cargo unit graph", &error.to_string())),
        CanonicalPart::Dependencies { values, keys } => {
            write_normalized_dependencies(writer, values, keys, allocation, limits)
        }
    }
}

fn write_merged_object(
    writer: &mut dyn std::io::Write,
    object: &Map<String, Value>,
    replacements: &[(&str, &str, CanonicalPart<'_>)],
    allocation: &mut JsonAllocation,
    limits: &Limits,
) -> Result<()> {
    writer
        .write_all(b"{")
        .map_err(|error| schema("canonical Cargo unit graph", &error.to_string()))?;
    let mut first = true;
    let mut retained = object
        .iter()
        .filter(|(key, _)| {
            !replacements
                .iter()
                .any(|(source, output, _)| *source == key.as_str() || *output == key.as_str())
        })
        .peekable();
    let mut replacement = 0;
    loop {
        let retained_key = retained.peek().map(|(key, _)| key.as_str());
        let replacement_key = replacements.get(replacement).map(|(_, output, _)| *output);
        let take_replacement = match (retained_key, replacement_key) {
            (Some(retained), Some(replacement)) => replacement <= retained,
            (None, Some(_)) => true,
            (Some(_), None) => false,
            (None, None) => break,
        };
        if take_replacement {
            if !first {
                writer
                    .write_all(b",")
                    .map_err(|error| schema("canonical Cargo unit graph", &error.to_string()))?;
            }
            first = false;
            write_json(
                "canonical Cargo unit graph",
                writer,
                replacements[replacement].1,
            )?;
            writer
                .write_all(b":")
                .map_err(|error| schema("canonical Cargo unit graph", &error.to_string()))?;
            write_part(writer, replacements[replacement].2, allocation, limits)?;
            replacement += 1;
            continue;
        }
        let (key, value) = retained
            .next()
            .ok_or_else(|| schema("canonical Cargo unit graph", "object merge lost a field"))?;
        if !first {
            writer
                .write_all(b",")
                .map_err(|error| schema("canonical Cargo unit graph", &error.to_string()))?;
        }
        first = false;
        write_json("canonical Cargo unit graph", writer, key)?;
        writer
            .write_all(b":")
            .map_err(|error| schema("canonical Cargo unit graph", &error.to_string()))?;
        write_canonical_value(writer, value, allocation, limits)?;
    }
    writer
        .write_all(b"}")
        .map_err(|error| schema("canonical Cargo unit graph", &error.to_string()))
}

/// The indices a unit depends on.
fn unit_dependency_indices(unit: &Value, count: usize) -> Result<Vec<usize>> {
    unit.get("dependencies")
        .and_then(Value::as_array)
        .ok_or_else(|| schema("Cargo unit graph", "unit dependencies are absent"))?
        .iter()
        .map(|dependency| {
            dependency
                .get("index")
                .and_then(Value::as_u64)
                .and_then(|index| usize::try_from(index).ok())
                .filter(|index| *index < count)
                .ok_or_else(|| schema("Cargo unit graph", "dependency index is invalid"))
        })
        .collect()
}

/// Every unit index after the units it depends on.
pub(super) fn dependency_order(units: &[Value]) -> Result<Vec<usize>> {
    const NEW: u8 = 0;
    const OPEN: u8 = 1;
    const DONE: u8 = 2;
    let mut state = vec![NEW; units.len()];
    let mut order = Vec::with_capacity(units.len());
    for root in 0..units.len() {
        if state[root] != NEW {
            continue;
        }
        state[root] = OPEN;
        let mut stack = vec![(
            root,
            unit_dependency_indices(&units[root], units.len())?,
            0_usize,
        )];
        while let Some((index, dependencies, next)) = stack.last_mut() {
            if let Some(&dependency) = dependencies.get(*next) {
                *next += 1;
                // Cargo's graphs have no cycles; an edge back into the
                // open path adds nothing to the order.
                if state[dependency] == NEW {
                    state[dependency] = OPEN;
                    let nested = unit_dependency_indices(&units[dependency], units.len())?;
                    stack.push((dependency, nested, 0));
                }
            } else {
                state[*index] = DONE;
                order.push(*index);
                stack.pop();
            }
        }
    }
    Ok(order)
}

/// Every unit's key. A key names its dependencies' keys, as Cargo's
/// `-C metadata` hashes its dependencies' hashes: a crate built over two
/// variants of one dependency, such as a host and a target build of a crate
/// over two `libc` feature sets, is two units even without `--target`.
fn unit_key_buffers(
    units: &[Value],
    no_build_std: config::NoBuildStd,
    allocation: &mut JsonAllocation,
    limits: &Limits,
) -> Result<Vec<Budgeted<Box<[u8]>>>> {
    let mut keys = (0..units.len()).map(|_| None).collect::<Vec<_>>();
    let mut digests: Vec<Option<Digest>> = vec![None; units.len()];
    for index in dependency_order(units)? {
        let mut dependencies = unit_dependency_indices(&units[index], units.len())?
            .into_iter()
            .filter_map(|dependency| digests[dependency])
            .collect::<Vec<_>>();
        dependencies.sort_unstable();
        let key = canonical_unit_key_buffer(
            &units[index],
            &dependencies,
            no_build_std,
            allocation,
            limits,
        )?;
        digests[index] = Some(digest(key.value()));
        keys[index] = Some(key);
    }
    keys.into_iter()
        .map(|key| key.ok_or_else(|| schema("Cargo unit graph", "unit key is absent")))
        .collect()
}

fn canonical_unit_key_buffer(
    unit: &Value,
    dependencies: &[Digest],
    no_build_std: config::NoBuildStd,
    allocation: &mut JsonAllocation,
    limits: &Limits,
) -> Result<Budgeted<Box<[u8]>>> {
    let object = unit
        .as_object()
        .ok_or_else(|| schema("Cargo unit graph", "unit is not an object"))?;
    let fields = [
        "features", "is_std", "mode", "pkg_id", "platform", "profile", "target",
    ];
    for field in fields.into_iter().filter(|field| *field != "is_std") {
        if !object.contains_key(field) {
            return Err(schema("Cargo unit graph", &format!("unit lacks `{field}`")));
        }
    }
    let compiler_shipped = is_std(unit, no_build_std)?;
    buffered_canonical(
        "canonical Cargo unit key",
        allocation,
        limits,
        |writer, allocation| {
            writer
                .write_all(b"{\"dependencies\":[")
                .map_err(|error| schema("canonical Cargo unit key", &error.to_string()))?;
            for (index, dependency) in dependencies.iter().enumerate() {
                let separator = if index == 0 { "" } else { "," };
                let mut hex = String::with_capacity(64);
                for byte in dependency.as_bytes() {
                    use std::fmt::Write as _;
                    let _ = write!(hex, "{byte:02x}");
                }
                write!(writer, "{separator}\"{hex}\"")
                    .map_err(|error| schema("canonical Cargo unit key", &error.to_string()))?;
            }
            writer
                .write_all(b"],")
                .map_err(|error| schema("canonical Cargo unit key", &error.to_string()))?;
            for (index, (field, part)) in [
                ("features", CanonicalPart::Value(&object["features"])),
                ("is_std", CanonicalPart::Bool(compiler_shipped)),
                ("mode", CanonicalPart::Value(&object["mode"])),
                ("pkg_id", CanonicalPart::Value(&object["pkg_id"])),
                ("platform", CanonicalPart::Value(&object["platform"])),
                ("profile", CanonicalPart::Value(&object["profile"])),
                ("target", CanonicalPart::Value(&object["target"])),
            ]
            .into_iter()
            .enumerate()
            {
                if index != 0 {
                    writer
                        .write_all(b",")
                        .map_err(|error| schema("canonical Cargo unit key", &error.to_string()))?;
                }
                write_json("canonical Cargo unit key", writer, field)?;
                writer
                    .write_all(b":")
                    .map_err(|error| schema("canonical Cargo unit key", &error.to_string()))?;
                write_part(writer, part, allocation, limits)?;
            }
            writer
                .write_all(b"}")
                .map_err(|error| schema("canonical Cargo unit key", &error.to_string()))
        },
    )
}

fn write_normalized_dependency(
    writer: &mut dyn std::io::Write,
    dependency: &Value,
    keys: &[Budgeted<Box<[u8]>>],
    allocation: &mut JsonAllocation,
    limits: &Limits,
) -> Result<()> {
    let object = dependency
        .as_object()
        .ok_or_else(|| schema("Cargo unit graph", "dependency is not an object"))?;
    let index = object
        .get("index")
        .and_then(Value::as_u64)
        .and_then(|index| usize::try_from(index).ok())
        .filter(|index| *index < keys.len())
        .ok_or_else(|| schema("Cargo unit graph", "dependency index is invalid"))?;
    write_merged_object(
        writer,
        object,
        &[("index", "unit", CanonicalPart::UnitKey(keys[index].value()))],
        allocation,
        limits,
    )
}

fn write_normalized_dependencies(
    writer: &mut dyn std::io::Write,
    dependencies: &[Value],
    keys: &[Budgeted<Box<[u8]>>],
    allocation: &mut JsonAllocation,
    limits: &Limits,
) -> Result<()> {
    let count = dependencies.len();
    let scratch = JsonOwnership::collection()
        .checked_add(JsonOwnership::nodes(count))?
        .checked_add(JsonOwnership::buffer(
            std::mem::size_of::<Budgeted<Box<[u8]>>>()
                .checked_mul(count)
                .ok_or_else(|| overflow("canonical Cargo dependency sort buffer bytes"))?,
        ))?;
    allocation.transform_owned(JsonOwnership::empty(), |allocation| {
        allocation.acquire(scratch, limits)?;
        let mut encoded = Vec::new();
        encoded
            .try_reserve_exact(count)
            .map_err(|source| Error::Allocation {
                operation: "canonical Cargo unit graph",
                source,
            })?;
        for dependency in dependencies {
            encoded.push(buffered_canonical(
                "canonical Cargo dependency",
                allocation,
                limits,
                |writer, allocation| {
                    write_normalized_dependency(writer, dependency, keys, allocation, limits)
                },
            )?);
        }
        encoded.sort_unstable_by(|left, right| left.value().cmp(right.value()));
        writer
            .write_all(b"[")
            .map_err(|error| schema("canonical Cargo unit graph", &error.to_string()))?;
        for (index, value) in encoded.iter().enumerate() {
            if index != 0 {
                writer
                    .write_all(b",")
                    .map_err(|error| schema("canonical Cargo unit graph", &error.to_string()))?;
            }
            writer
                .write_all(value.value())
                .map_err(|error| schema("canonical Cargo unit graph", &error.to_string()))?;
        }
        writer
            .write_all(b"]")
            .map_err(|error| schema("canonical Cargo unit graph", &error.to_string()))?;
        for value in encoded {
            value.drop_from(allocation)?;
        }
        allocation.release(scratch)
    })
}

fn canonical_normalized_unit_buffer(
    unit: &Value,
    keys: &[Budgeted<Box<[u8]>>],
    no_build_std: config::NoBuildStd,
    allocation: &mut JsonAllocation,
    limits: &Limits,
) -> Result<Budgeted<Box<[u8]>>> {
    let object = unit
        .as_object()
        .ok_or_else(|| schema("Cargo unit graph", "unit is not an object"))?;
    let dependencies = object
        .get("dependencies")
        .and_then(Value::as_array)
        .ok_or_else(|| schema("Cargo unit graph", "unit dependencies are absent"))?;
    let compiler_shipped = is_std(unit, no_build_std)?;
    buffered_canonical(
        "canonical Cargo unit",
        allocation,
        limits,
        |writer, allocation| {
            write_merged_object(
                writer,
                object,
                &[
                    (
                        "dependencies",
                        "dependencies",
                        CanonicalPart::Dependencies {
                            values: dependencies,
                            keys,
                        },
                    ),
                    ("is_std", "is_std", CanonicalPart::Bool(compiler_shipped)),
                ],
                allocation,
                limits,
            )
        },
    )
}

fn canonical_unit_write(
    writer: &mut dyn std::io::Write,
    value: &Value,
    no_build_std: config::NoBuildStd,
    allocation: &mut JsonAllocation,
    limits: &Limits,
) -> Result<()> {
    allocation.transform_owned(JsonOwnership::empty(), |allocation| {
        let object = value
            .as_object()
            .ok_or_else(|| schema("Cargo unit graph", "root is not an object"))?;
        let units = array(object, "units", "Cargo unit graph")?;
        let roots = array(object, "roots", "Cargo unit graph")?;
        let vector_bytes = std::mem::size_of::<Budgeted<Box<[u8]>>>()
            .checked_mul(units.len().saturating_mul(2))
            .and_then(|bytes| {
                bytes.checked_add(std::mem::size_of::<&Box<[u8]>>().saturating_mul(units.len()))
            })
            .and_then(|bytes| {
                bytes.checked_add(std::mem::size_of::<&[u8]>().saturating_mul(roots.len()))
            })
            .ok_or_else(|| overflow("canonical Cargo unit vector bytes"))?;
        let scratch = JsonOwnership::collection()
            .checked_add(JsonOwnership::nodes(units.len()))?
            .checked_add(JsonOwnership::collection())?
            .checked_add(JsonOwnership::nodes(units.len()))?
            .checked_add(JsonOwnership::collection())?
            .checked_add(JsonOwnership::nodes(units.len()))?
            .checked_add(JsonOwnership::collection())?
            .checked_add(JsonOwnership::nodes(roots.len()))?
            .checked_add(JsonOwnership::buffer(vector_bytes))?;
        allocation.acquire(scratch, limits)?;
        let mut keys = Vec::new();
        let mut normalized = Vec::new();
        keys.try_reserve_exact(units.len())
            .map_err(|source| Error::Allocation {
                operation: "canonical Cargo unit graph",
                source,
            })?;
        normalized
            .try_reserve_exact(units.len())
            .map_err(|source| Error::Allocation {
                operation: "canonical Cargo unit graph",
                source,
            })?;
        keys.extend(unit_key_buffers(units, no_build_std, allocation, limits)?);
        let mut unique = keys.iter().map(Budgeted::value).collect::<Vec<_>>();
        unique.sort_unstable();
        if unique.windows(2).any(|pair| pair[0] == pair[1]) {
            return Err(schema(
                "Cargo unit graph",
                "ambiguous semantic unit identity",
            ));
        }
        drop(unique);
        for unit in units {
            normalized.push(canonical_normalized_unit_buffer(
                unit,
                &keys,
                no_build_std,
                allocation,
                limits,
            )?);
        }
        normalized.sort_unstable_by(|left, right| left.value().cmp(right.value()));
        let mut root_keys = Vec::new();
        root_keys
            .try_reserve_exact(roots.len())
            .map_err(|source| Error::Allocation {
                operation: "canonical Cargo unit graph",
                source,
            })?;
        for root in roots {
            let index = root
                .as_u64()
                .and_then(|index| usize::try_from(index).ok())
                .filter(|index| *index < keys.len())
                .ok_or_else(|| schema("Cargo unit graph", "root index is invalid"))?;
            root_keys.push(keys[index].value().as_ref());
        }
        root_keys.sort_unstable();
        writer
            .write_all(b"{\"roots\":[")
            .map_err(|error| schema("canonical Cargo unit graph", &error.to_string()))?;
        for (index, root) in root_keys.iter().enumerate() {
            if index != 0 {
                writer
                    .write_all(b",")
                    .map_err(|error| schema("canonical Cargo unit graph", &error.to_string()))?;
            }
            writer
                .write_all(root)
                .map_err(|error| schema("canonical Cargo unit graph", &error.to_string()))?;
        }
        writer
            .write_all(b"],\"units\":[")
            .map_err(|error| schema("canonical Cargo unit graph", &error.to_string()))?;
        for (index, unit) in normalized.iter().enumerate() {
            if index != 0 {
                writer
                    .write_all(b",")
                    .map_err(|error| schema("canonical Cargo unit graph", &error.to_string()))?;
            }
            writer
                .write_all(unit.value())
                .map_err(|error| schema("canonical Cargo unit graph", &error.to_string()))?;
        }
        writer
            .write_all(b"],\"version\":")
            .map_err(|error| schema("canonical Cargo unit graph", &error.to_string()))?;
        let version = object
            .get("version")
            .ok_or_else(|| schema("Cargo unit graph", "version is absent"))?;
        write_canonical_value(writer, version, allocation, limits)?;
        writer
            .write_all(b"}")
            .map_err(|error| schema("canonical Cargo unit graph", &error.to_string()))?;
        for unit in normalized {
            unit.drop_from(allocation)?;
        }
        for key in keys {
            key.drop_from(allocation)?;
        }
        allocation.release(scratch)
    })
}

fn canonical_unit_digest_budgeted(
    value: &Value,
    no_build_std: config::NoBuildStd,
    allocation: &mut JsonAllocation,
    limits: &Limits,
) -> Result<Digest> {
    struct Hashing {
        digest: Sha256,
        bytes: usize,
    }
    impl std::io::Write for Hashing {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.bytes = self
                .bytes
                .checked_add(bytes.len())
                .ok_or_else(|| std::io::Error::other("canonical unit byte count overflow"))?;
            self.digest.update(bytes);
            Ok(bytes.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let mut hashing = Hashing {
        digest: Sha256::new(),
        bytes: 0,
    };
    canonical_unit_write(&mut hashing, value, no_build_std, allocation, limits)?;
    allocation.exceeded(
        "canonical Cargo unit bytes",
        limits.json_bytes,
        hashing.bytes,
    );
    Ok(Digest(hashing.digest.finalize().into()))
}

#[cfg(test)]
fn canonical_unit_bytes_clone_reference(
    value: Budgeted<Value>,
    no_build_std: config::NoBuildStd,
    allocation: &mut JsonAllocation,
    limits: &Limits,
) -> Result<Budgeted<Box<[u8]>>> {
    let consumed = value.ownership();
    allocation.transform_owned_retaining(consumed, |allocation| {
        let (value, _) = value.into_parts();
        let object = value
            .as_object()
            .ok_or_else(|| schema("Cargo unit graph", "root is not an object"))?;
        let units = array(object, "units", "Cargo unit graph")?;
        let mut keys = Vec::new();
        allocation.nodes("canonical Cargo unit key nodes", units.len(), limits)?;
        keys.try_reserve_exact(units.len())
            .map_err(|source| Error::Allocation {
                operation: "canonical Cargo unit graph",
                source,
            })?;
        let mut ordered: Vec<Option<(Value, Digest)>> = (0..units.len()).map(|_| None).collect();
        for index in dependency_order(units)? {
            let mut key = unit_key_budgeted(&units[index], no_build_std, allocation, limits)?;
            let mut dependencies = unit_dependency_indices(&units[index], units.len())?
                .into_iter()
                .filter_map(|dependency| ordered[dependency].as_ref().map(|(_, digest)| *digest))
                .collect::<Vec<_>>();
            dependencies.sort_unstable();
            let hex = dependencies
                .iter()
                .map(|digest| {
                    Value::String(
                        digest
                            .as_bytes()
                            .iter()
                            .map(|byte| format!("{byte:02x}"))
                            .collect(),
                    )
                })
                .collect();
            key.as_object_mut()
                .ok_or_else(|| schema("Cargo unit graph", "unit key is not an object"))?
                .insert("dependencies".to_owned(), Value::Array(hex));
            canonicalize(&mut key, allocation, limits)?;
            let bytes = json_sort_key(&key, allocation, limits)?;
            ordered[index] = Some((key, digest(&bytes)));
        }
        keys.extend(
            ordered
                .into_iter()
                .map(|key| key.expect("every unit is ordered").0),
        );
        let mut unique = BTreeMap::new();
        allocation.member_nodes("canonical Cargo unit uniqueness nodes", keys.len(), limits)?;
        for key in &keys {
            let bytes = json_sort_key(key, allocation, limits)?;
            if unique.insert(bytes, ()).is_some() {
                return Err(schema(
                    "Cargo unit graph",
                    "ambiguous semantic unit identity",
                ));
            }
        }

        let mut normalized_units = Vec::new();
        allocation.nodes("canonical Cargo normalized-unit nodes", units.len(), limits)?;
        normalized_units
            .try_reserve_exact(units.len())
            .map_err(|source| Error::Allocation {
                operation: "canonical Cargo unit graph",
                source,
            })?;
        for unit in units {
            allocation.charge_tree(unit, limits)?;
            let mut unit = unit.clone();
            let compiler_shipped = is_std(&unit, no_build_std)?;
            allocation.member_nodes("canonical Cargo normalized-unit members", 1, limits)?;
            allocation.retained_string(
                "canonical Cargo normalized-unit field bytes",
                "is_std".len(),
                limits,
            )?;
            unit.as_object_mut()
                .ok_or_else(|| schema("Cargo unit graph", "unit is not an object"))?
                .insert("is_std".to_owned(), Value::Bool(compiler_shipped));
            let dependencies = unit
                .get_mut("dependencies")
                .and_then(Value::as_array_mut)
                .ok_or_else(|| schema("Cargo unit graph", "unit dependencies are absent"))?;
            for dependency in dependencies {
                let dependency = dependency
                    .as_object_mut()
                    .ok_or_else(|| schema("Cargo unit graph", "dependency is not an object"))?;
                let index = dependency
                    .remove("index")
                    .and_then(|index| index.as_u64())
                    .and_then(|index| usize::try_from(index).ok())
                    .filter(|index| *index < keys.len())
                    .ok_or_else(|| schema("Cargo unit graph", "dependency index is invalid"))?;
                allocation.charge_tree(&keys[index], limits)?;
                allocation.member_nodes("canonical Cargo dependency members", 1, limits)?;
                allocation.retained_string(
                    "canonical Cargo dependency field bytes",
                    "unit".len(),
                    limits,
                )?;
                dependency.insert("unit".to_owned(), keys[index].clone());
            }
            normalized_units.push(unit);
        }
        let root_indices = array(object, "roots", "Cargo unit graph")?;
        let mut roots = Vec::new();
        allocation.nodes("canonical Cargo root nodes", root_indices.len(), limits)?;
        roots
            .try_reserve_exact(root_indices.len())
            .map_err(|source| Error::Allocation {
                operation: "canonical Cargo unit graph",
                source,
            })?;
        for root in root_indices {
            let index = root
                .as_u64()
                .and_then(|index| usize::try_from(index).ok())
                .filter(|index| *index < keys.len())
                .ok_or_else(|| schema("Cargo unit graph", "root index is invalid"))?;
            allocation.charge_tree(&keys[index], limits)?;
            roots.push(keys[index].clone());
        }
        allocation.member_nodes("canonical Cargo graph object nodes", 3, limits)?;
        allocation.retained_string(
            "canonical Cargo graph field bytes",
            "version".len() + "units".len() + "roots".len(),
            limits,
        )?;
        let mut normalized = serde_json::json!({
            "version": object.get("version").cloned(),
            "units": normalized_units,
            "roots": roots,
        });
        canonicalize(&mut normalized, allocation, limits)?;
        canonical_json_bytes_budgeted(
            "canonical Cargo unit graph",
            &normalized,
            allocation,
            limits,
        )
    })
}

#[cfg(test)]
fn canonical_metadata_bytes(value: Value) -> Result<Box<[u8]>> {
    let limits = Limits::standard();
    let mut allocation = JsonAllocation::empty();
    let ownership = JsonOwnership::tree(&value)?;
    allocation.acquire(ownership, &limits)?;
    let mut value = Budgeted::from_retained(value, ownership);
    let bytes = canonical_metadata_bytes_budgeted(&mut value, &mut allocation, &limits)?;
    value.drop_from(&mut allocation)?;
    Ok(bytes.into_parts().0)
}

#[cfg(test)]
fn canonical_unit_bytes(value: Value, no_build_std: config::NoBuildStd) -> Result<Box<[u8]>> {
    let limits = Limits::standard();
    let mut allocation = JsonAllocation::empty();
    let ownership = JsonOwnership::tree(&value)?;
    allocation.acquire(ownership, &limits)?;
    let mut bytes = Vec::new();
    canonical_unit_write(&mut bytes, &value, no_build_std, &mut allocation, &limits)?;
    allocation.release(ownership)?;
    Ok(bytes.into_boxed_slice())
}

#[cfg(test)]
fn unit_key_budgeted(
    unit: &Value,
    no_build_std: config::NoBuildStd,
    allocation: &mut JsonAllocation,
    limits: &Limits,
) -> Result<Value> {
    let object = unit
        .as_object()
        .ok_or_else(|| schema("Cargo unit graph", "unit is not an object"))?;
    let mut key = Map::new();
    let fields = [
        "pkg_id", "target", "profile", "platform", "mode", "features",
    ];
    allocation.member_nodes("canonical Cargo unit-key members", fields.len() + 1, limits)?;
    allocation.retained_string(
        "canonical Cargo unit-key field bytes",
        fields
            .iter()
            .map(|field| field.len())
            .sum::<usize>()
            .saturating_add("is_std".len()),
        limits,
    )?;
    for field in fields {
        let value = object
            .get(field)
            .ok_or_else(|| schema("Cargo unit graph", &format!("unit lacks `{field}`")))?;
        allocation.charge_tree(value, limits)?;
        key.insert(field.to_owned(), value.clone());
    }
    key.insert(
        "is_std".to_owned(),
        Value::Bool(is_std(unit, no_build_std)?),
    );
    let mut key = Value::Object(key);
    canonicalize(&mut key, allocation, limits)?;
    Ok(key)
}

#[cfg(test)]
fn unit_key(unit: &Value, no_build_std: config::NoBuildStd) -> Result<Value> {
    unit_key_budgeted(
        unit,
        no_build_std,
        &mut JsonAllocation::empty(),
        &Limits::standard(),
    )
}

#[cfg(test)]
fn validate_command_directories(object: &Map<String, Value>, command_target: &Path) -> Result<()> {
    validate_command_directory_pair(object, command_target, command_target)
}

fn validate_command_directory_pair(
    object: &Map<String, Value>,
    command_target: &Path,
    command_build: &Path,
) -> Result<()> {
    let shared = command_target.as_os_str() == command_build.as_os_str();
    let command_target =
        super::normalized_absolute("Cargo command target directory", command_target)?;
    let command_build = if shared {
        None
    } else {
        Some(super::normalized_absolute(
            "Cargo command build directory",
            command_build,
        )?)
    };
    for (field, error_field, expected) in [
        (
            "target_directory",
            "Cargo target directory",
            &command_target,
        ),
        (
            "build_directory",
            "Cargo build directory",
            command_build.as_ref().unwrap_or(&command_target),
        ),
    ] {
        let path = object
            .get(field)
            .and_then(Value::as_str)
            .ok_or_else(|| schema("Cargo metadata", &format!("`{field}` is not a string")))?;
        let path = super::normalized_absolute(error_field, Path::new(path))?;
        if &path != expected {
            return Err(Error::ResolutionDrift {
                command: "metadata",
                field: error_field,
            });
        }
    }
    Ok(())
}

#[cfg(test)]
mod schema_tests {
    use std::fs;

    use serde_json::{json, Value};

    use crate::cargo::config::{Environment, Provenance};
    use crate::cargo::Limits;

    fn no_build_std() -> crate::cargo::config::NoBuildStd {
        let path = std::env::temp_dir().join(format!(
            "rawr-no-build-std-proof-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        fs::create_dir(&path).expect("test config root is created");
        let path = fs::canonicalize(&path).expect("test config root is canonical");
        let parallelism =
            crate::process::parallelism::Proof::discover().expect("test parallelism discovers");
        let provenance = Provenance::discover(
            &path,
            None,
            Environment::empty(),
            "aarch64-apple-darwin",
            "dev",
            parallelism,
            &Limits::standard(),
        )
        .expect("ordinary config provenance proves no build-std");
        fs::remove_dir(path).expect("test config root is removed");
        provenance.no_build_std()
    }

    #[test]
    fn pinned_is_std_schema_requires_proof_and_normalizes_absence() {
        let proof = no_build_std();
        let absent = json!({
            "pkg_id": "path+file:///fixture#fixture@0.1.0",
            "target": {},
            "profile": {},
            "platform": "aarch64-apple-darwin",
            "mode": "build",
            "features": [],
            "dependencies": []
        });
        let mut explicit_false = absent.clone();
        explicit_false["is_std"] = Value::Bool(false);
        assert_eq!(
            super::unit_key(&absent, proof).expect("proved absent is_std is non-std"),
            super::unit_key(&explicit_false, proof).expect("explicit false is non-std")
        );
        let absent_graph = json!({"version": 1, "units": [absent.clone()], "roots": [0]});
        let false_graph = json!({"version": 1, "units": [explicit_false], "roots": [0]});
        assert_eq!(
            super::canonical_unit_bytes(absent_graph, proof)
                .expect("proved absent is_std canonicalizes"),
            super::canonical_unit_bytes(false_graph, proof)
                .expect("explicit false is_std canonicalizes")
        );

        let mut explicit_true = absent.clone();
        explicit_true["is_std"] = Value::Bool(true);
        let error = super::unit_key(&explicit_true, proof)
            .expect_err("build-std/sysroot Cargo unit must reject");
        assert!(format!("{error}").contains("build-std/sysroot"));

        let mut invalid = absent;
        invalid["is_std"] = Value::String("false".to_owned());
        let error = super::unit_key(&invalid, proof).expect_err("non-boolean is_std must reject");
        assert!(format!("{error}").contains("not a boolean"));
    }
}

#[cfg(test)]
mod allocation_tests {
    #[cfg(feature = "rustc-driver")]
    use std::collections::BTreeSet;

    use serde_json::json;

    use super::{
        canonical_metadata_bytes_budgeted, canonicalize, parse_owned_json, preflight,
        preflight_from, sort_values, Allocation, JsonAllocation, AGGREGATE_BYTES, AGGREGATE_ITEMS,
        OWNED_JSON_VALUES, RETAINED_BUFFER, RETAINED_CONTAINERS, RETAINED_ELEMENTS,
        RETAINED_MEMBERS, RETAINED_STRINGS,
    };
    #[cfg(feature = "rustc-driver")]
    use super::{BuildGraph, Output, OWNED_REACHABILITY_NODES};
    use crate::cargo::Limits;

    /// The named accounting axis of this epoch recorded its target and
    /// a truthful high water; the charge itself was admitted.
    fn exceeded(allocation: &JsonAllocation, axis: &str, target: usize, high_water: usize) {
        let record = allocation.exceedances();
        let found = record
            .axis(axis)
            .unwrap_or_else(|| panic!("`{axis}` records an exceedance: {record:?}"));
        assert_eq!(
            (found.target, found.high_water),
            (target as u64, high_water as u64),
            "`{axis}` target and high water",
        );
    }

    /// Every recorded axis whose target is `target` reached exactly
    /// `high_water`; at least one such axis exists. Used where the exceeding
    /// charge site names its own resource.
    fn exceeded_at_target(allocation: &JsonAllocation, target: usize, high_water: usize) {
        let record = allocation.exceedances();
        let mut matched = 0_usize;
        for axis in record.axes().filter(|axis| axis.target == target as u64) {
            assert_eq!(
                axis.high_water, high_water as u64,
                "`{}` high water",
                axis.name
            );
            matched += 1;
        }
        assert!(
            matched > 0,
            "an axis with target {target} records an exceedance: {record:?}"
        );
    }

    /// The canonical JSON and unit byte sizes are accounting targets
    /// recorded on the epoch; exact admits silently, one over admits and
    /// records the truthful size, nothing refuses.
    #[test]
    fn canonical_json_bytes_record_the_json_target_without_refusal() {
        let value = json!({"packages": [], "version": 1});
        let length = serde_json::to_vec(&value).unwrap().len();
        let exercise = |json_bytes: usize| {
            let mut limits = Limits::standard();
            limits.json_bytes = json_bytes;
            let mut allocation = JsonAllocation::empty();
            let bytes = super::canonical_json_bytes_budgeted(
                "canonical fixture",
                &value,
                &mut allocation,
                &limits,
            )
            .expect("the canonical document is produced")
            .into_parts()
            .0;
            assert_eq!(bytes.len(), length);
            allocation
        };
        assert!(exercise(length)
            .exceedances()
            .axis("canonical Cargo JSON bytes")
            .is_none());
        exceeded(
            &exercise(length - 1),
            "canonical Cargo JSON bytes",
            length - 1,
            length,
        );
    }

    /// The artifact profile string sizes are accounting targets measured
    /// on the account that owns the message. Exact admits silently; under the
    /// target the same profile is produced and each crossing axis records its
    /// truthful size; a detached record folds into the epoch unchanged.
    #[test]
    fn artifact_profile_string_targets_record_without_refusal() {
        let value = json!({
            "opt_level": "3",
            "debuginfo": "line-tables-only",
            "debug_assertions": true,
            "overflow_checks": true,
            "test": false
        });
        let encoded = super::ArtifactProfile::from_build(&value, &Limits::standard())
            .expect("the fixture profile parses")
            .as_bytes()
            .len();
        let exercise = |string_bytes: usize| {
            let mut limits = Limits::standard();
            limits.string_bytes = string_bytes;
            let mut record = crate::exceedance::Record::new();
            let profile = super::ArtifactProfile::from_build_measured(&value, &limits, &mut record)
                .expect("the profile parses regardless of its string targets");
            assert_eq!(profile.as_bytes().len(), encoded);
            record
        };
        assert!(!exercise(encoded).is_exceeded());
        let one_under = exercise(encoded - 1);
        let axis = one_under
            .axis("artifact profile bytes")
            .expect("the encoded profile size records its axis");
        assert_eq!(
            (axis.target, axis.first_requested, axis.high_water),
            ((encoded - 1) as u64, encoded as u64, encoded as u64)
        );
        assert_eq!(one_under.axes().count(), 1);
        let zero = exercise(0);
        for (name, actual) in [
            ("artifact opt_level", 1),
            ("artifact debuginfo", "line-tables-only".len()),
            ("artifact profile bytes", encoded),
        ] {
            let axis = zero
                .axis(name)
                .unwrap_or_else(|| panic!("`{name}` records"));
            assert_eq!((axis.target, axis.high_water), (0, actual as u64));
        }
        let mut allocation = JsonAllocation::empty();
        allocation.record(&zero);
        exceeded(&allocation, "artifact profile bytes", 0, encoded);
        exceeded(&allocation, "artifact opt_level", 0, 1);
        assert_eq!(allocation.exceedances().axes().count(), 3);
    }

    /// The script and compiler-artifact unit counts of a prepared unit
    /// graph are accounting targets on the JSON epoch. The exact count admits
    /// silently; under it the graph is still prepared and each crossing axis
    /// records its truthful count.
    #[test]
    fn prepared_graph_unit_targets_record_without_refusal() {
        let source = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/cargo/unit.rs");
        let source = source.to_str().expect("the fixture source path is UTF-8");
        let profile = json!({
            "name": "dev",
            "opt_level": "0",
            "debuginfo": 2,
            "debug_assertions": true,
            "overflow_checks": true
        });
        let target = |kind: &str, crate_type: &str, name: &str| {
            json!({
                "kind": [kind],
                "crate_types": [crate_type],
                "name": name,
                "src_path": source,
                "edition": "2021"
            })
        };
        let document = json!({
            "version": 1,
            "roots": [1, 2],
            "units": [
                {
                    "pkg_id": "path+file:///fixture#fixture@0.1.0",
                    "target": target("custom-build", "bin", "build-script-build"),
                    "profile": profile,
                    "platform": null,
                    "mode": "build",
                    "features": [],
                    "dependencies": [],
                    "is_std": false
                },
                {
                    "pkg_id": "path+file:///fixture#fixture@0.1.0",
                    "target": target("custom-build", "bin", "build-script-build"),
                    "profile": profile,
                    "platform": "aarch64-apple-darwin",
                    "mode": "run-custom-build",
                    "features": [],
                    "dependencies": [{"index": 0, "extern_crate_name": null}],
                    "is_std": false
                },
                {
                    "pkg_id": "path+file:///fixture#fixture@0.1.0",
                    "target": target("lib", "lib", "fixture"),
                    "profile": profile,
                    "platform": "aarch64-apple-darwin",
                    "mode": "build",
                    "features": [],
                    "dependencies": [{"index": 1, "extern_crate_name": null}],
                    "is_std": false
                }
            ]
        });
        let exercise = |units: usize| {
            let mut limits = Limits::standard();
            limits.units = units;
            let owned = serde_json::from_value::<super::OwnedUnitDocument>(document.clone())
                .expect("the fixture unit graph deserializes");
            let masks = super::UnitMasks {
                flags: Box::from([super::RETAINED_UNIT; 3]),
            };
            let mut allocation = JsonAllocation::empty();
            let prepared = super::prepare_owned_graph(
                owned,
                &masks,
                "aarch64-apple-darwin",
                crate::cargo::config::NoBuildStd::fixture(),
                &limits,
                &mut allocation,
            )
            .expect("the unit graph is prepared regardless of the unit target");
            assert_eq!((prepared.outputs.len(), prepared.scripts.len()), (2, 1));
            allocation
        };
        assert!(!exercise(2).exceedances().is_exceeded());
        let one_under = exercise(1);
        exceeded(&one_under, "Build compiler artifact units", 1, 2);
        assert_eq!(one_under.exceedances().axes().count(), 1);
        let zero = exercise(0);
        exceeded(&zero, "normal Build script units", 0, 1);
        exceeded(&zero, "Build compiler artifact units", 0, 2);
    }

    #[test]
    fn retained_transport_moves_buffer_and_keeps_prior_exact_floor_account() {
        for floor in [0, 1] {
            let mut limits = Limits::standard();
            limits.json_bytes = 17 - floor;
            let mut epoch = Allocation::begin(&limits);
            let prior = epoch
                .retain_transport(vec![b'p'; 5].into_boxed_slice())
                .expect("prior ownership fits the original epoch");
            let bytes = vec![b'x'; 12].into_boxed_slice();
            let pointer = bytes.as_ptr();
            let result = epoch.retain_transport(bytes);
            let retained = result.expect("the transport buffer moves under either floor");
            assert_eq!(retained.bytes().as_ptr(), pointer);
            assert_eq!(epoch.retained.bytes, 17);
            assert_eq!(epoch.retained.total_bytes, 17);
            assert_eq!(epoch.retained.items, 2);
            if floor == 0 {
                assert!(!epoch.retained.exceedances().is_exceeded());
            } else {
                // The one-byte floor admits the move and records the
                // prior floor account plus this buffer as the truthful high water.
                exceeded(&epoch.retained, RETAINED_BUFFER, 16, 17);
                exceeded(&epoch.retained, AGGREGATE_BYTES, 16, 17);
                assert_eq!(epoch.retained.exceedances().axes().count(), 2);
            }
            assert_eq!(prior.bytes(), b"ppppp");
        }
    }

    #[test]
    fn retained_transport_and_parsed_tree_share_one_exact_epoch() {
        let mut limits = Limits::standard();
        limits.json_bytes = 15;
        let mut epoch = Allocation::begin(&limits);
        let prior = epoch
            .retain_transport(vec![b'p'; 7].into_boxed_slice())
            .expect("prior work fits");
        let transport = epoch
            .retain_transport(b"\"abc\"".to_vec().into_boxed_slice())
            .expect("five transport bytes fit");
        epoch.retained = preflight_from(
            "retained transport fixture",
            transport.bytes(),
            &limits,
            epoch.retained,
        )
        .expect("three decoded string bytes reach the exact total");
        assert_eq!(epoch.retained.bytes, 12);
        assert_eq!(epoch.retained.strings, 3);
        assert_eq!(epoch.retained.total_bytes, 15);
        assert!(!epoch.retained.exceedances().is_exceeded());
        // One more transport byte is admitted; the shared aggregate axis
        // records the exact epoch total as its high water while the buffer
        // axis (13 of 15) stays within target.
        let _over = epoch
            .retain_transport(vec![0].into_boxed_slice())
            .expect("the shared epoch admits one byte past its target");
        assert_eq!(epoch.retained.total_bytes, 16);
        assert_eq!(epoch.retained.bytes, 13);
        exceeded(&epoch.retained, AGGREGATE_BYTES, 15, 16);
        assert!(epoch.retained.exceedances().axis(RETAINED_BUFFER).is_none());
        assert_eq!(prior.bytes().len(), 7);
    }

    #[test]
    fn json_value_member_and_element_limits_accept_exact_and_reject_plus_one() {
        let mut limits = Limits::standard();
        limits.items = 6;
        let exact = preflight("fixture", br#"[1,2]"#, &limits).expect("exact scalar array seals");
        assert!(!exact.exceedances().is_exceeded());
        // One more element is admitted; only the shared aggregate item
        // axis crosses (values 4, elements 3 stay within the target of 6).
        let over = preflight("fixture", br#"[1,2,3]"#, &limits)
            .expect("one extra element is admitted and recorded");
        assert_eq!((over.values, over.elements, over.items), (4, 3, 8));
        exceeded(&over, AGGREGATE_ITEMS, 6, 8);
        assert_eq!(over.exceedances().axes().count(), 1);
        let exact = preflight("fixture", br#"{"a":1,"b":2}"#, &limits)
            .expect("exact object member count seals");
        assert!(!exact.exceedances().is_exceeded());
        let over = preflight("fixture", br#"{"a":1,"b":2,"c":3}"#, &limits)
            .expect("one extra member is admitted and recorded");
        assert_eq!((over.values, over.members, over.items), (4, 3, 8));
        exceeded(&over, AGGREGATE_ITEMS, 6, 8);
        assert_eq!(over.exceedances().axes().count(), 1);
        // The per-category axes cross when they are the binding target.
        limits.items = 2;
        let over = preflight("fixture", br#"[1,2,3]"#, &limits)
            .expect("element and value targets admit and record");
        exceeded(&over, RETAINED_ELEMENTS, 2, 3);
        exceeded(&over, super::RETAINED_VALUES, 2, 4);
        exceeded(&over, AGGREGATE_ITEMS, 2, 8);
        let over = preflight("fixture", br#"{"a":1,"b":2,"c":3}"#, &limits)
            .expect("member and value targets admit and record");
        exceeded(&over, RETAINED_MEMBERS, 2, 3);
        exceeded(&over, super::RETAINED_VALUES, 2, 4);
        exceeded(&over, AGGREGATE_ITEMS, 2, 8);
    }

    #[test]
    fn json_aggregate_string_and_depth_limits_accept_exact_and_reject_plus_one() {
        let mut limits = Limits::standard();
        limits.string_bytes = 4;
        let exact =
            preflight("fixture", br#"["aa","bb"]"#, &limits).expect("exact aggregate strings seal");
        assert!(!exact.exceedances().is_exceeded());
        // One more string byte is admitted; both the raw preflight count
        // and the decoded retained string axis record the exact total of 5.
        let over = preflight("fixture", br#"["aa","bbb"]"#, &limits)
            .expect("one extra string byte is admitted and recorded");
        assert_eq!(over.strings, 5);
        exceeded(&over, RETAINED_STRINGS, 4, 5);
        exceeded(&over, "Cargo JSON preflight string bytes", 4, 5);
        limits.string_bytes = 6;
        let exact = preflight("fixture", br#""\u0061""#, &limits)
            .expect("an escaped string fits its exact zero-copy raw boundary");
        assert!(!exact.exceedances().is_exceeded());
        limits.string_bytes = 5;
        // The six raw escape bytes cross the target; the one decoded byte does not.
        let over = preflight("fixture", br#""\u0061""#, &limits)
            .expect("the raw escape boundary is admitted and recorded");
        assert_eq!(over.strings, 1);
        exceeded(&over, "Cargo JSON preflight string bytes", 5, 6);
        assert!(over.exceedances().axis(RETAINED_STRINGS).is_none());
        // `depth` bounds serde recursion and stays a physical refusal.
        limits.string_bytes = Limits::standard().string_bytes;
        limits.depth = 2;
        preflight("fixture", br#"[[0]]"#, &limits).expect("exact JSON depth seals");
        assert!(matches!(
            preflight("fixture", br#"[[[0]]]"#, &limits),
            Err(crate::Error::Schema {
                document: "fixture",
                ..
            })
        ));
    }

    #[test]
    fn retained_buffer_local_and_cumulative_caps_are_distinct_and_atomic() {
        let limits = Limits::standard();
        let mut local = JsonAllocation::empty();
        local
            .retained_bytes("local retained buffer", 4, 4, &limits)
            .expect("the exact local buffer cap seals");
        assert!(!local.exceedances().is_exceeded());
        let retained = local;
        // The over-cap local buffer is admitted; every counter advances
        // together by exactly the charge, and only the local axis records.
        local
            .retained_bytes("local retained buffer", 5, 4, &limits)
            .expect("the local buffer target admits and records");
        assert_eq!(
            (local.items, local.bytes, local.total_bytes),
            (
                retained.items + 1,
                retained.bytes + 5,
                retained.total_bytes + 5
            ),
        );
        exceeded(&local, "local retained buffer", 4, 5);
        assert_eq!(local.exceedances().axes().count(), 1);

        let mut aggregate_limits = limits;
        aggregate_limits.json_bytes = 8;
        let mut cumulative = JsonAllocation::empty();
        cumulative
            .retained_bytes("first retained buffer", 4, 8, &aggregate_limits)
            .expect("the first cumulative buffer seals");
        cumulative
            .retained_bytes("second retained buffer", 4, 8, &aggregate_limits)
            .expect("the exact cumulative buffer cap seals");
        assert!(!cumulative.exceedances().is_exceeded());
        let retained = cumulative;
        // The one-byte third buffer is within its local cap; only the cumulative
        // buffer and aggregate byte axes record the exact epoch total of 9.
        cumulative
            .retained_bytes("third retained buffer", 1, 8, &aggregate_limits)
            .expect("the cumulative buffer target admits and records");
        assert_eq!(
            (cumulative.items, cumulative.bytes, cumulative.total_bytes),
            (
                retained.items + 1,
                retained.bytes + 1,
                retained.total_bytes + 1
            ),
        );
        exceeded(&cumulative, RETAINED_BUFFER, 8, 9);
        exceeded(&cumulative, AGGREGATE_BYTES, 8, 9);
        assert!(cumulative
            .exceedances()
            .axis("third retained buffer")
            .is_none());
        assert_eq!(cumulative.exceedances().axes().count(), 2);
    }

    #[test]
    fn canonicalization_rejects_when_the_cumulative_allocation_is_exhausted() {
        let mut limits = Limits::standard();
        limits.items = 4;
        let bytes = br#"{"a":1}"#;
        let mut allocation =
            preflight("fixture", bytes, &limits).expect("the retained parsed value exactly fits");
        let value = json!({"a": 1});
        let ownership = super::JsonOwnership::tree(&value).expect("fixture ownership measures");
        let mut value = super::Budgeted::from_retained(value, ownership);
        assert!(canonical_metadata_bytes_budgeted(&mut value, &mut allocation, &limits).is_err());
    }

    #[test]
    fn canonical_unit_stream_retains_source_and_releases_all_temporaries() {
        let limits = Limits::standard();
        let value = json!({
            "version": 1,
            "units": [{
                "pkg_id": "path+file:///fixture#fixture@0.1.0",
                "target": {},
                "profile": {},
                "platform": "aarch64-apple-darwin",
                "mode": "build",
                "features": [],
                "dependencies": [{
                    "index": 0,
                    "extern_crate_name": "fixture",
                    "public": false
                }],
                "is_std": false
            }],
            "roots": [0]
        });
        let ownership = super::JsonOwnership::tree(&value).expect("source ownership measures");
        let mut allocation = JsonAllocation::empty();
        allocation
            .acquire(ownership, &limits)
            .expect("source ownership fits");
        let expected = super::canonical_unit_bytes_clone_reference(
            super::Budgeted::from_retained(value.clone(), ownership),
            crate::cargo::config::NoBuildStd::fixture(),
            &mut {
                let mut reference = JsonAllocation::empty();
                reference
                    .acquire(ownership, &limits)
                    .expect("reference source ownership fits");
                reference
            },
            &limits,
        )
        .expect("clone-shaped reference canonicalization seals");
        let actual = super::canonical_unit_digest_budgeted(
            &value,
            crate::cargo::config::NoBuildStd::fixture(),
            &mut allocation,
            &limits,
        )
        .expect("borrowed canonical unit stream seals");
        assert_eq!(actual, super::digest(expected.value()));
        assert_eq!(allocation.retained_ownership(), ownership);
        allocation
            .release(ownership)
            .expect("source ownership releases");
        assert_eq!(
            allocation.full_usage(),
            JsonAllocation::empty().full_usage()
        );
    }

    #[test]
    fn unit_masks_precharge_one_bounded_buffer_and_release_exactly() {
        let value = json!({
            "units": [
                {
                    "mode": "build",
                    "dependencies": [{"index": 1}],
                    "is_std": false
                },
                {
                    "mode": "build",
                    "dependencies": [],
                    "is_std": false
                },
                {
                    "mode": "run-custom-build",
                    "dependencies": [],
                    "is_std": false
                }
            ]
        });
        let extraction = std::collections::BTreeMap::from([("rawr_iced", 0)]);
        let exercise = |limits: &Limits| {
            let mut allocation = JsonAllocation::empty();
            let masks = super::unit_masks(
                &value,
                &extraction,
                crate::cargo::config::NoBuildStd::fixture(),
                &mut allocation,
                limits,
            )?;
            assert!(masks.value().retained(0));
            assert!(masks.value().retained(1));
            assert!(!masks.value().retained(2));
            assert!(masks.value().extraction(0));
            assert!(!masks.value().extraction(1));
            let owned = masks.ownership();
            masks.drop_from(&mut allocation)?;
            assert_eq!(
                allocation.full_usage(),
                JsonAllocation::empty().full_usage()
            );
            Ok::<_, crate::Error>((owned, allocation))
        };

        let standard = Limits::standard();
        let (owned, allocation) = exercise(&standard).expect("one exact unit mask seals");
        assert!(!allocation.exceedances().is_exceeded());
        let mut exact = standard;
        exact.items = owned.items;
        exact.json_bytes = owned.total_bytes;
        exact.containers = owned.containers;
        let (_, allocation) = exercise(&exact).expect("the exact unit-mask caps seal");
        assert!(!allocation.exceedances().is_exceeded());

        // One below each exact cap still builds and releases the mask;
        // the crossed axis records the exact precharge as its high water, and
        // the record outlives the release.
        let mut item_floor = exact;
        item_floor.items = item_floor.items.saturating_sub(1);
        let (_, allocation) = exercise(&item_floor).expect("the item floor admits");
        exceeded(&allocation, AGGREGATE_ITEMS, item_floor.items, owned.items);
        let mut byte_floor = exact;
        byte_floor.json_bytes = byte_floor.json_bytes.saturating_sub(1);
        let (_, allocation) = exercise(&byte_floor).expect("the byte floor admits");
        exceeded(
            &allocation,
            AGGREGATE_BYTES,
            byte_floor.json_bytes,
            owned.total_bytes,
        );
        let mut container_floor = exact;
        container_floor.containers = container_floor.containers.saturating_sub(1);
        let (_, allocation) = exercise(&container_floor).expect("the container floor admits");
        exceeded(
            &allocation,
            RETAINED_CONTAINERS,
            container_floor.containers,
            owned.containers,
        );
    }

    #[test]
    fn failed_canonical_unit_transform_releases_only_its_owned_scope() {
        let limits = Limits::standard();
        let mut allocation = JsonAllocation::empty();
        let prior = allocation
            .retain_with(super::JsonOwnership::string(5), &limits, || {
                Ok::<_, crate::Error>(Box::<str>::from("prior"))
            })
            .expect("prior ownership fits");
        let before = allocation;
        let value = json!({
            "version": 1,
            "units": [{
                "pkg_id": "path+file:///fixture#fixture@0.1.0",
                "target": {},
                "profile": {},
                "platform": "aarch64-apple-darwin",
                "mode": "build",
                "features": [],
                "dependencies": [{"index": 1}],
                "is_std": false
            }],
            "roots": [0]
        });
        let ownership = super::JsonOwnership::tree(&value).expect("source ownership measures");
        allocation
            .acquire(ownership, &limits)
            .expect("source ownership fits");
        assert!(super::canonical_unit_digest_budgeted(
            &value,
            crate::cargo::config::NoBuildStd::fixture(),
            &mut allocation,
            &limits,
        )
        .is_err());
        allocation
            .release(ownership)
            .expect("failed source ownership releases");
        assert_eq!(allocation.full_usage(), before.full_usage());
        assert_eq!(allocation.strings, prior.value().len());
    }

    #[test]
    fn metadata_and_build_graph_fanout_share_exact_owned_counters() {
        let exercise = |limits: &Limits| {
            let mut allocation = JsonAllocation::empty();
            allocation.member_nodes("metadata package-map nodes", 2, limits)?;
            allocation.nodes("metadata feature nodes", 3, limits)?;
            allocation.retained_string("metadata retained strings", 5, limits)?;
            allocation.member_nodes("Build artifact-map nodes", 2, limits)?;
            allocation.nodes("Build output/cfg nodes", 3, limits)?;
            allocation.retained_string("Build retained strings", 7, limits)?;
            Ok::<_, crate::Error>(allocation)
        };
        let mut exact = Limits::standard();
        exact.items = 12;
        exact.string_bytes = 12;
        let allocation = exercise(&exact).expect("exact Metadata/BuildGraph fanout seals");
        assert_eq!(allocation.members + allocation.elements, 10);
        assert_eq!(allocation.items, 12);
        assert_eq!(allocation.strings, 12);
        assert!(!allocation.exceedances().is_exceeded());

        // One below each exact counter admits the whole fanout and records
        // exactly one axis whose high water is the exact shared total.
        let mut item_floor = exact;
        item_floor.items = 11;
        let allocation = exercise(&item_floor).expect("the item floor admits");
        exceeded(&allocation, AGGREGATE_ITEMS, 11, 12);
        assert_eq!(allocation.exceedances().axes().count(), 1);
        let mut string_floor = exact;
        string_floor.string_bytes = 11;
        let allocation = exercise(&string_floor).expect("the string floor admits");
        exceeded(&allocation, "Build retained strings", 11, 12);
        assert_eq!(allocation.exceedances().axes().count(), 1);
    }

    #[test]
    fn metadata_and_unit_raw_and_domain_ownership_share_one_aggregate_cap() {
        const METADATA: &[u8] = br#"{"metadata":"mmmmmmmmmmmmmmmmmmmmmmmmmmmmmmmm"}"#;
        const UNIT: &[u8] = br#"[{"unit":"uuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuu"}]"#;
        let exercise = |limits: &Limits| {
            OWNED_JSON_VALUES.with(|owned| owned.set(0));
            let mut allocation = preflight("Cargo metadata", METADATA, limits)?;
            let _metadata = parse_owned_json("Cargo metadata", METADATA)?;
            allocation.member_nodes("Metadata domain map", 1, limits)?;
            allocation.retained_string("Metadata domain identity", 8, limits)?;
            allocation = preflight_from("Cargo unit graph", UNIT, limits, allocation)?;
            let _unit = parse_owned_json("Cargo unit graph", UNIT)?;
            Ok::<_, crate::Error>((allocation, OWNED_JSON_VALUES.with(std::cell::Cell::get)))
        };

        let standard = Limits::standard();
        let (usage, owned) = exercise(&standard).expect("standard combined ownership measures");
        assert_eq!(owned, 2);
        let mut exact = standard;
        exact.items = usage.items;
        exact.json_bytes = usage.total_bytes;
        let (allocation, owned) = exercise(&exact).expect("exact combined aggregate caps seal");
        assert_eq!(owned, 2);
        assert!(!allocation.exceedances().is_exceeded());

        // One below each shared cap still parses both DOMs; the shared
        // aggregate axis records the exact combined total as its high water.
        let mut item_floor = exact;
        item_floor.items = item_floor.items.saturating_sub(1);
        let (allocation, owned) = exercise(&item_floor).expect("the item floor admits");
        assert_eq!(owned, 2);
        exceeded(&allocation, AGGREGATE_ITEMS, item_floor.items, usage.items);
        let mut byte_floor = exact;
        byte_floor.json_bytes = byte_floor.json_bytes.saturating_sub(1);
        let (allocation, owned) = exercise(&byte_floor).expect("the byte floor admits");
        assert_eq!(owned, 2);
        exceeded(
            &allocation,
            AGGREGATE_BYTES,
            byte_floor.json_bytes,
            usage.total_bytes,
        );
    }

    #[test]
    fn provisional_scope_releases_exact_delta_and_retains_prior_ownership_once() {
        let limits = Limits::standard();
        let mut epoch = Allocation::begin(&limits);
        let prior = epoch
            .retained
            .retain_with(super::JsonOwnership::string(5), &limits, || {
                Ok::<_, crate::Error>(Box::<str>::from("prior"))
            })
            .expect("prior ownership fits");
        let starting = epoch.usage();
        let mut checkpoint = epoch
            .begin_provisional()
            .expect("one provisional scope opens");
        let temporary = epoch
            .retained
            .retain_with(super::JsonOwnership::string(9), &limits, || {
                Ok::<_, crate::Error>(Box::<str>::from("temporary"))
            })
            .expect("temporary ownership fits");
        assert_eq!(epoch.usage().strings, 14);
        drop(temporary);
        epoch
            .release_provisional(&mut checkpoint)
            .expect("the exact temporary delta releases");
        assert_eq!(epoch.usage().full_usage(), starting.full_usage());
        assert_eq!(epoch.usage().strings, prior.value().len());
        assert_eq!(epoch.usage().peak_usage().2, 14);
        assert!(epoch.release_provisional(&mut checkpoint).is_err());
    }

    #[test]
    fn provisional_scope_rejects_nested_wrong_epoch_and_consumed_tokens() {
        let limits = Limits::standard();
        let mut first = Allocation::begin(&limits);
        let mut checkpoint = first.begin_provisional().expect("the first scope opens");
        assert!(first.begin_provisional().is_err());

        let mut second = Allocation::begin(&limits);
        assert!(second.release_provisional(&mut checkpoint).is_err());
        assert!(first.release_provisional(&mut checkpoint).is_err());
        assert!(second.begin_provisional().is_ok());
    }

    #[test]
    fn provisional_scope_releases_partial_error_ownership_after_values_drop() {
        let limits = Limits::standard();
        let mut epoch = Allocation::begin(&limits);
        let starting = epoch.usage();
        let mut checkpoint = epoch
            .begin_provisional()
            .expect("the error-path scope opens");
        let partial = epoch
            .retained
            .retain_with(super::JsonOwnership::string(7), &limits, || {
                Ok::<_, crate::Error>(Box::<str>::from("partial"))
            })
            .expect("partial provisional value fits");
        let failed = Err::<(), _>(crate::Error::Request {
            field: "provisional fixture",
            reason: "fails after allocation".into(),
        });
        drop(partial);
        epoch
            .release_provisional(&mut checkpoint)
            .expect("error cleanup releases the dropped partial value");
        assert!(failed.is_err());
        assert_eq!(epoch.usage().full_usage(), starting.full_usage());
        assert!(epoch.release_provisional(&mut checkpoint).is_err());
    }

    #[test]
    fn provisional_release_and_versioned_metadata_unit_share_one_epoch() {
        const SEED: &[u8] =
            b"version = 4\n\n[[package]]\nname = \"rawr-iced\"\nversion = \"0.1.0\"\n";
        const METADATA_TEMPLATE: &str = r#"{
            "packages": [
                {
                    "id": "path+file:///work/rawr#rawr-resolution-harness@0.0.0",
                    "name": "rawr-resolution-harness",
                    "version": "0.0.0",
                    "source": null,
                    "checksum": null,
                    "manifest_path": "/work/rawr/Cargo.toml",
                    "dependencies": [],
                    "targets": [{
                        "kind": ["lib"],
                        "crate_types": ["lib"],
                        "name": "rawr_resolution_harness",
                        "src_path": "/work/rawr/src/lib.rs",
                        "edition": "2024"
                    }]
                },
                {
                    "id": "path+file:///work/rawr/iced#rawr-iced@0.1.0",
                    "name": "rawr-iced",
                    "version": "0.1.0",
                    "source": null,
                    "checksum": null,
                    "manifest_path": "/work/rawr/iced/Cargo.toml",
                    "dependencies": [],
                    "targets": [{
                        "kind": ["lib"],
                        "crate_types": ["lib"],
                        "name": "rawr_iced",
                        "src_path": "/work/rawr/iced/src/lib.rs",
                        "edition": "2024"
                    }]
                }
            ],
            "workspace_members": ["path+file:///work/rawr#rawr-resolution-harness@0.0.0"],
            "workspace_default_members": ["path+file:///work/rawr#rawr-resolution-harness@0.0.0"],
            "resolve": {
                "root": "path+file:///work/rawr#rawr-resolution-harness@0.0.0",
                "nodes": [
                    {
                        "id": "path+file:///work/rawr#rawr-resolution-harness@0.0.0",
                        "dependencies": [],
                        "deps": [],
                        "features": []
                    },
                    {
                        "id": "path+file:///work/rawr/iced#rawr-iced@0.1.0",
                        "dependencies": [],
                        "deps": [],
                        "features": ["fixture"]
                    }
                ]
            },
            "workspace_root": "/work/rawr",
            "target_directory": "/work/rawr/target",
            "build_directory": "/work/rawr/target",
            "version": 1,
            "metadata": null
        }"#;
        const UNIT_TEMPLATE: &str = r#"{
            "version": 1,
            "units": [
                {
                    "pkg_id": "path+file:///work/rawr#rawr-resolution-harness@0.0.0",
                    "target": {
                        "kind": ["lib"],
                        "crate_types": ["lib"],
                        "name": "rawr_resolution_harness",
                        "src_path": "/work/rawr/src/lib.rs",
                        "edition": "2024"
                    },
                    "profile": {
                        "name": "dev",
                        "opt_level": "0",
                        "debuginfo": 2,
                        "debug_assertions": true,
                        "overflow_checks": true
                    },
                    "platform": "aarch64-apple-darwin",
                    "mode": "build",
                    "features": [],
                    "dependencies": [],
                    "is_std": false
                },
                {
                    "pkg_id": "path+file:///work/rawr/iced#rawr-iced@0.1.0",
                    "target": {
                        "kind": ["lib"],
                        "crate_types": ["lib"],
                        "name": "rawr_iced",
                        "src_path": "/work/rawr/iced/src/lib.rs",
                        "edition": "2024"
                    },
                    "profile": {
                        "name": "dev",
                        "opt_level": "0",
                        "debuginfo": 2,
                        "debug_assertions": true,
                        "overflow_checks": true
                    },
                    "platform": "aarch64-apple-darwin",
                    "mode": "build",
                    "features": ["fixture"],
                    "dependencies": [],
                    "is_std": false
                }
            ],
            "roots": [0]
        }"#;

        // The fixture names this workspace: its manifests are read from disk.
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap();
        let root_text = root.to_str().unwrap();
        let metadata_bytes = METADATA_TEMPLATE
            .replace("/work/rawr", root_text)
            .into_bytes();
        let unit_bytes = UNIT_TEMPLATE.replace("/work/rawr", root_text).into_bytes();
        let target = root.join("target");
        let standard = Limits::standard();
        let definition = crate::cargo::universe::Definition::fixture_authenticated(SEED, &standard);
        // Two large unknown fields fit the original DOM, but leave less space
        // than the first typed package needs. Preserve them in both canonical
        // output and the optional observation without a second whole DOM.
        let mut padded: serde_json::Value = serde_json::from_slice(&metadata_bytes).unwrap();
        for package in padded["packages"].as_array_mut().unwrap() {
            package["unknown_retained"] = serde_json::Value::String(String::new());
        }
        const PRIOR: usize = 256;
        let baseline = super::JsonOwnership::tree(&padded).unwrap().strings;
        let padding = standard.string_bytes - PRIOR - 64 - baseline;
        padded["packages"][0]["unknown_retained"] =
            serde_json::Value::String("x".repeat(padding / 2));
        padded["packages"][1]["unknown_retained"] =
            serde_json::Value::String("y".repeat(padding - padding / 2));
        let bytes = serde_json::to_vec(&padded).unwrap();
        let expected = super::canonical_metadata_bytes(padded.clone()).unwrap();
        for observed in [false, true] {
            let mut epoch = Allocation::begin(&standard);
            let prior = epoch
                .retained
                .retain_with(super::JsonOwnership::string(PRIOR), &standard, || {
                    Ok("p".repeat(PRIOR))
                })
                .unwrap();
            let before = epoch.retained.retained_ownership();
            let mut legacy =
                preflight_from("Cargo metadata", &bytes, &standard, epoch.retained).unwrap();
            assert!(!legacy.exceedances().is_exceeded());
            // The legacy whole-DOM path admits the first typed package and
            // records the string axis it crosses; the compacting path below
            // stays within target without a second whole DOM.
            super::parse_package_budgeted(&padded["packages"][0], &mut legacy, &standard)
                .expect("the legacy whole-DOM path admits the typed package and records");
            assert!(legacy.strings > standard.string_bytes);
            let axis = legacy
                .exceedances()
                .axis("Cargo metadata retained string bytes")
                .copied()
                .expect("the legacy retained string axis records");
            assert_eq!(axis.target, standard.string_bytes as u64);
            assert_eq!(axis.high_water, legacy.strings as u64);
            let mut scope = epoch.begin_provisional().unwrap();
            let (metadata, observation) = super::Metadata::parse_observed(
                &bytes,
                &[],
                &standard,
                1,
                &target,
                &target,
                &mut epoch,
                observed.then_some(&definition),
            )
            .expect("near-cap complete metadata and typed owner coexist after compaction");
            assert_eq!(metadata.canonical.as_ref(), expected.as_ref());
            assert_eq!(observation.is_some(), observed);
            assert!(epoch.retained.strings < standard.string_bytes / 2);
            assert!(!epoch.retained.exceedances().is_exceeded());
            drop(metadata);
            drop(observation);
            epoch.release_provisional(&mut scope).unwrap();
            assert_eq!(epoch.retained.retained_ownership(), before);
            prior.drop_from(&mut epoch.retained).unwrap();
            assert_eq!(epoch.retained.full_usage(), (0, 0, 0, 0));
        }
        let exercise = |limits: &Limits, prior_bytes: Option<usize>| {
            OWNED_JSON_VALUES.with(|owned| owned.set(0));
            let mut epoch = Allocation::begin(limits);
            let _prior = prior_bytes
                .map(|bytes| {
                    epoch
                        .retained
                        .retain_with(super::JsonOwnership::string(bytes), limits, || {
                            String::from_utf8(vec![b'p'; bytes])
                                .map(String::into_boxed_str)
                                .map_err(|error| crate::Error::Schema {
                                    document: "prior allocation fixture",
                                    reason: error.to_string().into_boxed_str(),
                                })
                        })
                })
                .transpose()?;
            let mut provisional = epoch.begin_provisional()?;
            let provisional_metadata =
                super::Metadata::parse(&metadata_bytes, &[], limits, 1, &target, &mut epoch)?;
            let provisional_selected = provisional_metadata.selections(&[], &mut epoch)?;
            drop(provisional_selected);
            drop(provisional_metadata);
            epoch.release_provisional(&mut provisional)?;
            let (metadata, observation) = super::Metadata::parse_versioned(
                &metadata_bytes,
                &[],
                limits,
                1,
                &target,
                &mut epoch,
                &definition,
            )?;
            let (_graph, _metadata_proof, _unit_proof) = super::BuildGraph::parse_versioned(
                &unit_bytes,
                &metadata,
                limits,
                1,
                crate::cargo::config::NoBuildStd::fixture(),
                "dev",
                "aarch64-apple-darwin",
                &mut epoch,
                &definition,
                observation,
            )?;
            Ok::<_, crate::Error>((epoch.usage(), OWNED_JSON_VALUES.with(std::cell::Cell::get)))
        };

        let (usage, owned) =
            exercise(&standard, None).expect("the versioned typed path authenticates");
        assert_eq!(
            owned, 7,
            "three source documents, two saved packages, and one streamed package/resolve row pair parse"
        );
        let peak = usage.peak_usage();
        let mut exact = standard;
        exact.items = peak.0;
        exact.json_bytes = peak.1;
        exact.string_bytes = peak.2;
        exact.containers = peak.3;
        let (exact_usage, owned) =
            exercise(&exact, None).expect("the shared aggregate caps seal exactly");
        assert_eq!(owned, 7);
        assert!(!exact_usage.exceedances().is_exceeded());

        // One below each exact shared cap still completes the whole epoch
        // (every DOM parses); the crossed axis records the exact shared peak as
        // its truthful high water.
        let mut item_floor = exact;
        item_floor.items = item_floor.items.saturating_sub(1);
        let (floor_usage, owned) =
            exercise(&item_floor, None).expect("the item floor admits the epoch");
        assert_eq!(owned, 7);
        exceeded(&floor_usage, AGGREGATE_ITEMS, item_floor.items, peak.0);
        let mut byte_floor = exact;
        byte_floor.json_bytes = byte_floor.json_bytes.saturating_sub(1);
        let (floor_usage, owned) =
            exercise(&byte_floor, None).expect("the byte floor admits the epoch");
        assert_eq!(owned, 7);
        exceeded(&floor_usage, AGGREGATE_BYTES, byte_floor.json_bytes, peak.1);
        let mut string_floor = exact;
        string_floor.string_bytes = string_floor.string_bytes.saturating_sub(1);
        let (floor_usage, owned) =
            exercise(&string_floor, None).expect("the string floor admits the epoch");
        assert_eq!(owned, 7);
        exceeded_at_target(&floor_usage, string_floor.string_bytes, peak.2);
        let mut container_floor = exact;
        container_floor.containers = container_floor.containers.saturating_sub(1);
        let (floor_usage, owned) =
            exercise(&container_floor, None).expect("the container floor admits the epoch");
        assert_eq!(owned, 7);
        exceeded(
            &floor_usage,
            RETAINED_CONTAINERS,
            container_floor.containers,
            peak.3,
        );

        let prior_bytes = standard
            .string_bytes
            .saturating_sub(peak.2)
            .min(standard.json_bytes.saturating_sub(peak.1));
        let (near_cap, owned) = exercise(&standard, Some(prior_bytes))
            .expect("prior live ownership plus released provisional and final proof fit");
        assert_eq!(owned, 7);
        assert_eq!(near_cap.peak_usage().2, standard.string_bytes);
        assert!(!near_cap.exceedances().is_exceeded());
        let mut one_byte_floor = standard;
        one_byte_floor.string_bytes = one_byte_floor.string_bytes.saturating_sub(1);
        let (near_cap, owned) = exercise(&one_byte_floor, Some(prior_bytes))
            .expect("one string byte below the near-cap epoch admits");
        assert_eq!(owned, 7);
        exceeded_at_target(
            &near_cap,
            one_byte_floor.string_bytes,
            standard.string_bytes,
        );
    }

    #[test]
    fn initial_post_and_publication_json_advance_one_exact_epoch() {
        const INITIAL: &[u8] = br#"{"initial":"aaaaaaaa"}"#;
        const POST: &[u8] = br#"{"post":"bbbbbbbb"}"#;
        const PUBLICATION: &[u8] = br#"{"publication":"cccccccc"}"#;
        let exercise = |limits: &Limits| {
            OWNED_JSON_VALUES.with(|owned| owned.set(0));
            let mut epoch = Allocation::begin(limits);
            for (document, bytes) in [
                ("initial metadata", INITIAL),
                ("post metadata", POST),
                ("publication metadata", PUBLICATION),
            ] {
                let next = preflight_from(document, bytes, limits, epoch.retained)?;
                let _owned = parse_owned_json(document, bytes)?;
                epoch.retained = next;
            }
            epoch
                .retained
                .member_nodes("root selection map nodes", 2, limits)?;
            epoch
                .retained
                .nodes("alias group vector nodes", 3, limits)?;
            Ok::<_, crate::Error>((epoch.usage(), OWNED_JSON_VALUES.with(std::cell::Cell::get)))
        };

        let standard = Limits::standard();
        let (usage, owned) = exercise(&standard).expect("the complete epoch measures");
        assert_eq!(owned, 3);
        let mut exact = standard;
        exact.items = usage.items;
        exact.json_bytes = usage.total_bytes;
        exact.string_bytes = usage.strings;
        let (allocation, owned) = exercise(&exact).expect("the exact epoch caps seal");
        assert_eq!(owned, 3);
        assert!(!allocation.exceedances().is_exceeded());

        // One below each exact cap still advances the whole epoch (all
        // three DOMs and the helper nodes); the crossed shared axis records the
        // exact epoch total as its high water.
        let mut item_floor = exact;
        item_floor.items = item_floor.items.saturating_sub(1);
        let (allocation, owned) = exercise(&item_floor).expect("the item floor admits");
        assert_eq!(owned, 3);
        exceeded(&allocation, AGGREGATE_ITEMS, item_floor.items, usage.items);
        let mut byte_floor = exact;
        byte_floor.json_bytes = byte_floor.json_bytes.saturating_sub(1);
        let (allocation, owned) = exercise(&byte_floor).expect("the byte floor admits");
        assert_eq!(owned, 3);
        exceeded(
            &allocation,
            AGGREGATE_BYTES,
            byte_floor.json_bytes,
            usage.total_bytes,
        );
    }

    #[test]
    fn two_live_documents_keep_every_owned_category_cumulative() {
        const FIRST: &[u8] = br#"{"a":["aaaa"]}"#;
        const SECOND: &[u8] = br#"{"b":["bbbb"]}"#;
        let exercise = |limits: &Limits| {
            OWNED_JSON_VALUES.with(|owned| owned.set(0));
            let first = preflight("first document", FIRST, limits)?;
            let _first = parse_owned_json("first document", FIRST)?;
            let mut allocation = preflight_from("second document", SECOND, limits, first)?;
            let _second = parse_owned_json("second document", SECOND)?;
            allocation.retained_bytes(
                "two-document canonical buffer",
                6,
                limits.json_bytes,
                limits,
            )?;
            Ok::<_, crate::Error>((allocation, OWNED_JSON_VALUES.with(std::cell::Cell::get)))
        };

        let standard = Limits::standard();
        let (usage, owned) = exercise(&standard).expect("two live documents measure");
        assert_eq!(owned, 2);
        assert_eq!(usage.values, 6);
        assert_eq!(usage.members, 2);
        assert_eq!(usage.elements, 2);
        assert_eq!(usage.strings, 10);
        assert_eq!(usage.containers, 4);
        assert_eq!(usage.bytes, 6);

        let mut exact = standard;
        exact.items = usage.items;
        exact.json_bytes = usage.total_bytes;
        exact.string_bytes = usage.strings;
        exact.containers = usage.containers;
        let (allocation, owned) = exercise(&exact).expect("two-document exact live caps seal");
        assert_eq!(owned, 2);
        assert!(!allocation.exceedances().is_exceeded());

        // One below each cumulative cap still owns both documents; the
        // crossed category axis records the exact cumulative total.
        let mut item_floor = exact;
        item_floor.items = item_floor.items.saturating_sub(1);
        let (allocation, owned) = exercise(&item_floor).expect("the item floor admits");
        assert_eq!(owned, 2);
        exceeded(&allocation, AGGREGATE_ITEMS, item_floor.items, usage.items);
        let mut string_floor = exact;
        string_floor.string_bytes = string_floor.string_bytes.saturating_sub(1);
        let (allocation, _) = exercise(&string_floor).expect("the string floor admits");
        exceeded(
            &allocation,
            RETAINED_STRINGS,
            string_floor.string_bytes,
            usage.strings,
        );
        let mut container_floor = exact;
        container_floor.containers = container_floor.containers.saturating_sub(1);
        let (allocation, _) = exercise(&container_floor).expect("the container floor admits");
        exceeded(
            &allocation,
            RETAINED_CONTAINERS,
            container_floor.containers,
            usage.containers,
        );
        let mut byte_floor = exact;
        byte_floor.json_bytes = byte_floor.json_bytes.saturating_sub(1);
        let (allocation, _) = exercise(&byte_floor).expect("the byte floor admits");
        exceeded(
            &allocation,
            AGGREGATE_BYTES,
            byte_floor.json_bytes,
            usage.total_bytes,
        );
        let mut member_floor = exact;
        member_floor.items = usage.members.saturating_sub(1);
        let (allocation, _) = exercise(&member_floor).expect("the member floor admits");
        exceeded(
            &allocation,
            RETAINED_MEMBERS,
            member_floor.items,
            usage.members,
        );
        let mut element_floor = exact;
        element_floor.items = usage.elements.saturating_sub(1);
        let (allocation, _) = exercise(&element_floor).expect("the element floor admits");
        exceeded(
            &allocation,
            RETAINED_ELEMENTS,
            element_floor.items,
            usage.elements,
        );
    }

    #[test]
    fn three_live_documents_keep_every_owned_category_cumulative() {
        const DOCUMENTS: [(&str, &[u8]); 3] = [
            ("first document", br#"{"a":["aaaa"]}"#),
            ("second document", br#"{"b":["bbbb"]}"#),
            ("third document", br#"{"c":["cccc"]}"#),
        ];
        let exercise = |limits: &Limits| {
            OWNED_JSON_VALUES.with(|owned| owned.set(0));
            let mut allocation = JsonAllocation::empty();
            let mut owned_documents = Vec::with_capacity(DOCUMENTS.len());
            for (document, bytes) in DOCUMENTS {
                allocation = preflight_from(document, bytes, limits, allocation)?;
                owned_documents.push(parse_owned_json(document, bytes)?);
            }
            allocation.retained_bytes(
                "three-document canonical buffer",
                9,
                limits.json_bytes,
                limits,
            )?;
            assert_eq!(owned_documents.len(), DOCUMENTS.len());
            Ok::<_, crate::Error>((allocation, OWNED_JSON_VALUES.with(std::cell::Cell::get)))
        };

        let standard = Limits::standard();
        let (usage, owned) = exercise(&standard).expect("the many-document epoch measures");
        assert_eq!(owned, 3);
        assert_eq!(usage.values, 9);
        assert_eq!(usage.members, 3);
        assert_eq!(usage.elements, 3);
        assert_eq!(usage.strings, 15);
        assert_eq!(usage.containers, 6);
        assert_eq!(usage.bytes, 9);
        let mut exact = standard;
        exact.items = usage.items;
        exact.json_bytes = usage.total_bytes;
        exact.string_bytes = usage.strings;
        exact.containers = usage.containers;
        let (allocation, owned) = exercise(&exact).expect("exact cumulative caps seal");
        assert_eq!(owned, 3);
        assert!(!allocation.exceedances().is_exceeded());

        // One below each cumulative cap still owns all three documents;
        // the crossed category axis records the exact cumulative total.
        let mut item_floor = exact;
        item_floor.items = item_floor.items.saturating_sub(1);
        let (allocation, owned) = exercise(&item_floor).expect("the item floor admits");
        assert_eq!(owned, 3);
        exceeded(&allocation, AGGREGATE_ITEMS, item_floor.items, usage.items);
        let mut byte_floor = exact;
        byte_floor.json_bytes = byte_floor.json_bytes.saturating_sub(1);
        let (allocation, _) = exercise(&byte_floor).expect("the byte floor admits");
        exceeded(
            &allocation,
            AGGREGATE_BYTES,
            byte_floor.json_bytes,
            usage.total_bytes,
        );
        let mut string_floor = exact;
        string_floor.string_bytes = string_floor.string_bytes.saturating_sub(1);
        let (allocation, _) = exercise(&string_floor).expect("the string floor admits");
        exceeded(
            &allocation,
            RETAINED_STRINGS,
            string_floor.string_bytes,
            usage.strings,
        );
        let mut container_floor = exact;
        container_floor.containers = container_floor.containers.saturating_sub(1);
        let (allocation, _) = exercise(&container_floor).expect("the container floor admits");
        exceeded(
            &allocation,
            RETAINED_CONTAINERS,
            container_floor.containers,
            usage.containers,
        );
        let mut member_floor = exact;
        member_floor.items = usage.members.saturating_sub(1);
        let (allocation, _) = exercise(&member_floor).expect("the member floor admits");
        exceeded(
            &allocation,
            RETAINED_MEMBERS,
            member_floor.items,
            usage.members,
        );
        let mut element_floor = exact;
        element_floor.items = usage.elements.saturating_sub(1);
        let (allocation, _) = exercise(&element_floor).expect("the element floor admits");
        exceeded(
            &allocation,
            RETAINED_ELEMENTS,
            element_floor.items,
            usage.elements,
        );
    }

    #[cfg(feature = "rustc-driver")]
    #[test]
    fn high_fanout_reachability_charges_every_transient_node_before_growth() {
        let mut dependencies = Vec::new();
        dependencies
            .try_reserve_exact(128)
            .expect("fixture dependency storage reserves");
        for index in 0..128 {
            dependencies.push(format!("leaf-{index}").into_boxed_str());
        }
        let root = Output::fixture(
            "path+file:///fixture#fixture@0.1.0",
            "/fixture/src/lib.rs",
            "fixture",
            "lib",
            "rlib",
        )
        .with_fixture_semantic_key("root")
        .with_fixture_dependencies(dependencies.into_boxed_slice());
        let graph = BuildGraph::fixture_artifacts(Box::from([root]));
        let roots = BTreeSet::from(["root"]);
        let usage = graph
            .artifact_reachable_allocation(&roots, "absent")
            .expect("standard high-fanout traversal measures");

        assert!(!usage.exceedances().is_exceeded());
        let mut exact_graph = graph.clone();
        exact_graph.limits.items = usage.items;
        exact_graph
            .artifact_reachable(&roots, "absent")
            .expect("exact transient aggregate cap seals");
        assert!(!exact_graph
            .artifact_reachable_allocation(&roots, "absent")
            .expect("the exact traversal measures")
            .exceedances()
            .is_exceeded());
        // One below the exact transient cap still traverses every node;
        // the aggregate item axis records the exact transient total.
        let mut floor_graph = graph;
        floor_graph.limits.items = usage.items.saturating_sub(1);
        assert!(!floor_graph
            .artifact_reachable(&roots, "absent")
            .expect("the item floor admits the traversal"));
        let floor_usage = floor_graph
            .artifact_reachable_allocation(&roots, "absent")
            .expect("the item-floor traversal measures");
        assert_eq!(floor_usage.items, usage.items);
        exceeded(
            &floor_usage,
            AGGREGATE_ITEMS,
            floor_graph.limits.items,
            usage.items,
        );
    }

    #[cfg(feature = "rustc-driver")]
    #[test]
    fn post_metadata_checkpoint_precedes_high_fanout_reachability_ownership() {
        const DOCUMENTS: [(&str, &[u8]); 3] = [
            ("Cargo metadata", br#"{"metadata":"aaaaaaaa"}"#),
            ("Cargo unit graph", br#"{"unit":"bbbbbbbb"}"#),
            ("post-Build Cargo metadata", br#"{"post":"cccccccc"}"#),
        ];
        let exercise = |limits: &Limits| {
            OWNED_JSON_VALUES.with(|owned| owned.set(0));
            OWNED_REACHABILITY_NODES.with(|owned| owned.set(0));
            let mut epoch = Allocation::begin(limits);
            for (document, bytes) in DOCUMENTS {
                let next = preflight_from(document, bytes, limits, epoch.retained)?;
                let _owned = parse_owned_json(document, bytes)?;
                epoch.retained = next;
            }

            let mut dependencies = Vec::new();
            dependencies
                .try_reserve_exact(128)
                .expect("fixture dependency storage reserves");
            for index in 0..128 {
                dependencies.push(format!("leaf-{index}").into_boxed_str());
            }
            let root = Output::fixture(
                "path+file:///fixture#fixture@0.1.0",
                "/fixture/src/lib.rs",
                "fixture",
                "lib",
                "rlib",
            )
            .with_fixture_semantic_key("root")
            .with_fixture_dependencies(dependencies.into_boxed_slice());
            let mut graph = BuildGraph::fixture_artifacts(Box::from([root]));
            graph.limits = *limits;
            graph.bind_reachability(&epoch, limits)?;
            let roots = BTreeSet::from(["root"]);
            let usage = graph.artifact_reachable_allocation(&roots, "absent")?;
            Ok::<_, crate::Error>((
                usage,
                OWNED_JSON_VALUES.with(std::cell::Cell::get),
                OWNED_REACHABILITY_NODES.with(std::cell::Cell::get),
            ))
        };

        let standard = Limits::standard();
        let (usage, json_owned, reachability_owned) =
            exercise(&standard).expect("the full live sequence measures");
        assert_eq!(json_owned, 3);
        assert!(reachability_owned > 128);
        let mut exact = standard;
        exact.items = usage.items;
        exact.json_bytes = usage.total_bytes;
        let (exact_usage, exact_json_owned, exact_reachability_owned) =
            exercise(&exact).expect("the exact live sequence seals");
        assert_eq!(exact_json_owned, 3);
        assert_eq!(exact_reachability_owned, reachability_owned);
        assert!(!exact_usage.exceedances().is_exceeded());

        // One below the exact shared cap still owns every DOM and every
        // transient reachability node; the shared axis bound to the epoch
        // records the exact live total as its high water.
        let mut item_floor = exact;
        item_floor.items = item_floor.items.saturating_sub(1);
        let (floor_usage, json_owned, floor_reachability_owned) =
            exercise(&item_floor).expect("the item floor admits the live sequence");
        assert_eq!(json_owned, 3);
        assert_eq!(floor_reachability_owned, reachability_owned);
        exceeded(&floor_usage, AGGREGATE_ITEMS, item_floor.items, usage.items);

        let mut byte_floor = exact;
        byte_floor.json_bytes = byte_floor.json_bytes.saturating_sub(1);
        let (floor_usage, json_owned, floor_reachability_owned) =
            exercise(&byte_floor).expect("the byte floor admits the live sequence");
        assert_eq!(json_owned, 3);
        assert_eq!(floor_reachability_owned, reachability_owned);
        exceeded(
            &floor_usage,
            AGGREGATE_BYTES,
            byte_floor.json_bytes,
            usage.total_bytes,
        );
    }

    #[test]
    fn sortable_array_destination_accepts_exact_and_rejects_plus_one() {
        let mut exact = Limits::standard();
        exact.items = 3;
        let mut value = json!([1]);
        let mut allocation = JsonAllocation::empty();
        sort_values(&mut value, &mut allocation, &exact)
            .expect("temporary and destination array nodes fit exactly");
        assert!(!allocation.exceedances().is_exceeded());

        // One below the exact cap still sorts; the aggregate item axis
        // records the exact temporary-plus-destination total of 3.
        let mut over = Limits::standard();
        over.items = 2;
        let mut value = json!([1]);
        let mut allocation = JsonAllocation::empty();
        sort_values(&mut value, &mut allocation, &over)
            .expect("the item floor admits the sort and records");
        assert_eq!(value, json!([1]));
        exceeded(&allocation, AGGREGATE_ITEMS, 2, 3);
    }

    #[test]
    fn recursive_array_destination_accepts_exact_and_rejects_plus_one() {
        let mut exact = Limits::standard();
        exact.items = 3;
        let mut value = json!([1]);
        let mut allocation = JsonAllocation::empty();
        canonicalize(&mut value, &mut allocation, &exact)
            .expect("recursive temporary and destination nodes fit exactly");
        assert!(!allocation.exceedances().is_exceeded());

        // One below the exact cap still canonicalizes; the aggregate item
        // axis records the exact temporary-plus-destination total of 3.
        let mut over = Limits::standard();
        over.items = 2;
        let mut value = json!([1]);
        let mut allocation = JsonAllocation::empty();
        canonicalize(&mut value, &mut allocation, &over)
            .expect("the item floor admits the canonicalization and records");
        assert_eq!(value, json!([1]));
        exceeded(&allocation, AGGREGATE_ITEMS, 2, 3);
    }
}

fn normalize_metadata(
    value: &mut Value,
    ownership: &mut JsonOwnership,
    allocation: &mut JsonAllocation,
    limits: &Limits,
) -> Result<()> {
    let object = value
        .as_object_mut()
        .ok_or_else(|| schema("Cargo metadata", "root is not an object"))?;
    for field in ["target_directory", "build_directory"] {
        let slot = object
            .get_mut(field)
            .ok_or_else(|| schema("Cargo metadata", &format!("missing `{field}`")))?;
        let previous = slot
            .as_str()
            .ok_or_else(|| schema("Cargo metadata", &format!("`{field}` is not a string")))?;
        let released = JsonOwnership::string_bytes(previous.len());
        let acquired = JsonOwnership::string_bytes("$CARGO_COMMAND_TARGET".len());
        allocation.acquire(acquired, limits)?;
        let previous = std::mem::replace(slot, Value::String("$CARGO_COMMAND_TARGET".to_owned()));
        drop(previous);
        allocation.release(released)?;
        *ownership = ownership.checked_sub(released)?.checked_add(acquired)?;
    }
    sort_metadata_field(object, "workspace_members", allocation, limits)?;
    sort_metadata_field(object, "workspace_default_members", allocation, limits)?;
    if let Some(packages) = object.get_mut("packages").and_then(Value::as_array_mut) {
        for package in packages {
            let package = package
                .as_object_mut()
                .ok_or_else(|| schema("Cargo metadata", "package is not an object"))?;
            sort_metadata_field(package, "dependencies", allocation, limits)?;
            sort_metadata_field(package, "targets", allocation, limits)?;
            if let Some(dependencies) = package
                .get_mut("dependencies")
                .and_then(Value::as_array_mut)
            {
                for dependency in dependencies {
                    if let Some(dependency) = dependency.as_object_mut() {
                        sort_metadata_field(dependency, "features", allocation, limits)?;
                    }
                }
            }
            if let Some(targets) = package.get_mut("targets").and_then(Value::as_array_mut) {
                for target in targets {
                    if let Some(target) = target.as_object_mut() {
                        for field in ["kind", "crate_types", "required_features"] {
                            sort_metadata_field(target, field, allocation, limits)?;
                        }
                    }
                }
            }
            if let Some(features) = package.get_mut("features").and_then(Value::as_object_mut) {
                for members in features.values_mut() {
                    sort_metadata_values(members, allocation, limits)?;
                }
            }
        }
        sort_metadata_field(object, "packages", allocation, limits)?;
    }
    if let Some(resolve) = object.get_mut("resolve").and_then(Value::as_object_mut) {
        sort_metadata_field(resolve, "nodes", allocation, limits)?;
        if let Some(nodes) = resolve.get_mut("nodes").and_then(Value::as_array_mut) {
            for node in nodes {
                let node = node
                    .as_object_mut()
                    .ok_or_else(|| schema("Cargo metadata", "resolve node is not an object"))?;
                for field in ["deps", "dependencies", "features"] {
                    sort_metadata_field(node, field, allocation, limits)?;
                }
                if let Some(dependencies) = node.get_mut("deps").and_then(Value::as_array_mut) {
                    for dependency in dependencies {
                        if let Some(dependency) = dependency.as_object_mut() {
                            sort_metadata_field(dependency, "dep_kinds", allocation, limits)?;
                        }
                    }
                }
            }
        }
        sort_metadata_field(resolve, "nodes", allocation, limits)?;
    }
    Ok(())
}

fn sort_metadata_field(
    object: &mut Map<String, Value>,
    field: &str,
    allocation: &mut JsonAllocation,
    limits: &Limits,
) -> Result<()> {
    if let Some(value) = object.get_mut(field) {
        sort_metadata_values(value, allocation, limits)?;
    }
    Ok(())
}

fn sort_metadata_values(
    value: &mut Value,
    allocation: &mut JsonAllocation,
    limits: &Limits,
) -> Result<()> {
    let Some(values) = value.as_array_mut() else {
        return Ok(());
    };
    for value in values.iter_mut() {
        canonicalize_metadata_objects(value, allocation, limits)?;
    }
    let scratch = values.iter().try_fold(
        JsonOwnership::collection()
            .checked_add(JsonOwnership::nodes(values.len()))?
            .checked_add(JsonOwnership::collection())?
            .checked_add(JsonOwnership::nodes(values.len()))?,
        |ownership, value| {
            ownership.checked_add(JsonOwnership::buffer(canonical_json_size(
                "canonical Cargo JSON",
                value,
            )?))
        },
    )?;
    let sorted = allocation.with_scratch(scratch, limits, || {
        let mut keyed = Vec::new();
        keyed
            .try_reserve_exact(values.len())
            .map_err(|source| Error::Allocation {
                operation: "canonical Cargo JSON",
                source,
            })?;
        for value in std::mem::take(values) {
            keyed.push((canonical_json_key("canonical Cargo JSON", &value)?, value));
        }
        keyed.sort_unstable_by(|left, right| left.0.cmp(&right.0));
        let mut sorted = Vec::new();
        sorted
            .try_reserve_exact(keyed.len())
            .map_err(|source| Error::Allocation {
                operation: "canonical Cargo JSON",
                source,
            })?;
        sorted.extend(keyed.into_iter().map(|(_, value)| value));
        Ok(sorted)
    })?;
    *values = sorted;
    Ok(())
}

fn canonicalize_metadata_objects(
    value: &mut Value,
    allocation: &mut JsonAllocation,
    limits: &Limits,
) -> Result<()> {
    match value {
        Value::Array(values) => {
            for value in values {
                canonicalize_metadata_objects(value, allocation, limits)?;
            }
        }
        Value::Object(object) => {
            for value in object.values_mut() {
                canonicalize_metadata_objects(value, allocation, limits)?;
            }
            let scratch = JsonOwnership::collection()
                .checked_add(JsonOwnership::nodes(object.len()))?
                .checked_add(JsonOwnership::collection())?
                .checked_add(JsonOwnership::members(object.len()))?;
            let sorted = allocation.with_scratch(scratch, limits, || {
                let retained = std::mem::take(object);
                let mut keyed = Vec::new();
                keyed
                    .try_reserve_exact(retained.len())
                    .map_err(|source| Error::Allocation {
                        operation: "canonical Cargo JSON",
                        source,
                    })?;
                keyed.extend(retained);
                keyed.sort_unstable_by(|left, right| left.0.cmp(&right.0));
                let mut sorted = Map::new();
                sorted.extend(keyed);
                Ok(sorted)
            })?;
            *object = sorted;
        }
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => {}
    }
    Ok(())
}

fn canonical_json_size(document: &'static str, value: &Value) -> Result<usize> {
    struct Counter(usize);
    impl std::io::Write for Counter {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.0 = self
                .0
                .checked_add(bytes.len())
                .ok_or_else(|| std::io::Error::other("canonical JSON byte count overflow"))?;
            Ok(bytes.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    let mut counter = Counter(0);
    serde_json::to_writer(&mut counter, value).map_err(|error| Error::Schema {
        document,
        reason: error.to_string().into_boxed_str(),
    })?;
    Ok(counter.0)
}

fn canonical_json_key(document: &'static str, value: &Value) -> Result<Vec<u8>> {
    let length = canonical_json_size(document, value)?;
    let mut bytes = Vec::new();
    bytes
        .try_reserve_exact(length)
        .map_err(|source| Error::Allocation {
            operation: document,
            source,
        })?;
    serde_json::to_writer(&mut bytes, value).map_err(|error| Error::Schema {
        document,
        reason: error.to_string().into_boxed_str(),
    })?;
    Ok(bytes)
}

#[cfg(test)]
fn sort_values(value: &mut Value, allocation: &mut JsonAllocation, limits: &Limits) -> Result<()> {
    let Some(values) = value.as_array_mut() else {
        return Ok(());
    };
    let mut keyed = Vec::new();
    allocation.nodes("canonical Cargo sort nodes", values.len(), limits)?;
    keyed
        .try_reserve_exact(values.len())
        .map_err(|source| Error::Allocation {
            operation: "canonical Cargo JSON",
            source,
        })?;
    for mut value in std::mem::take(values) {
        canonicalize_objects(&mut value, allocation, limits)?;
        let key = json_sort_key(&value, allocation, limits)?;
        keyed.push((key, value));
    }
    keyed.sort_unstable_by(|left, right| left.0.cmp(&right.0));
    allocation.nodes(
        "canonical Cargo sorted array destination nodes",
        keyed.len(),
        limits,
    )?;
    values
        .try_reserve_exact(keyed.len())
        .map_err(|source| Error::Allocation {
            operation: "canonical Cargo JSON",
            source,
        })?;
    values.extend(keyed.into_iter().map(|(_, value)| value));
    Ok(())
}

#[cfg(test)]
fn canonicalize_objects(
    value: &mut Value,
    allocation: &mut JsonAllocation,
    limits: &Limits,
) -> Result<()> {
    match value {
        Value::Array(values) => {
            for value in values {
                canonicalize_objects(value, allocation, limits)?;
            }
        }
        Value::Object(object) => {
            let retained = std::mem::take(object);
            let mut keyed = Vec::new();
            allocation.nodes("canonical Cargo object sort nodes", retained.len(), limits)?;
            keyed
                .try_reserve_exact(retained.len())
                .map_err(|source| Error::Allocation {
                    operation: "canonical Cargo JSON",
                    source,
                })?;
            keyed.extend(retained);
            for (_, value) in &mut keyed {
                canonicalize_objects(value, allocation, limits)?;
            }
            keyed.sort_unstable_by(|left, right| left.0.cmp(&right.0));
            let mut sorted = Map::new();
            allocation.member_nodes(
                "canonical Cargo reordered object nodes",
                keyed.len(),
                limits,
            )?;
            for (key, value) in keyed {
                sorted.insert(key, value);
            }
            *object = sorted;
        }
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => {}
    }
    Ok(())
}

#[cfg(test)]
fn canonicalize(value: &mut Value, allocation: &mut JsonAllocation, limits: &Limits) -> Result<()> {
    match value {
        Value::Array(values) => {
            let mut keyed = Vec::new();
            allocation.nodes("canonical Cargo recursive sort nodes", values.len(), limits)?;
            keyed
                .try_reserve_exact(values.len())
                .map_err(|source| Error::Allocation {
                    operation: "canonical Cargo JSON",
                    source,
                })?;
            for mut value in std::mem::take(values) {
                canonicalize(&mut value, allocation, limits)?;
                let key = json_sort_key(&value, allocation, limits)?;
                keyed.push((key, value));
            }
            keyed.sort_unstable_by(|left, right| left.0.cmp(&right.0));
            allocation.nodes(
                "canonical Cargo recursive array destination nodes",
                keyed.len(),
                limits,
            )?;
            values
                .try_reserve_exact(keyed.len())
                .map_err(|source| Error::Allocation {
                    operation: "canonical Cargo JSON",
                    source,
                })?;
            values.extend(keyed.into_iter().map(|(_, value)| value));
        }
        Value::Object(object) => {
            let retained = std::mem::take(object);
            let mut keyed = Vec::new();
            allocation.nodes(
                "canonical Cargo recursive object nodes",
                retained.len(),
                limits,
            )?;
            keyed
                .try_reserve_exact(retained.len())
                .map_err(|source| Error::Allocation {
                    operation: "canonical Cargo JSON",
                    source,
                })?;
            keyed.extend(retained);
            for (_, value) in &mut keyed {
                canonicalize(value, allocation, limits)?;
            }
            keyed.sort_unstable_by(|left, right| left.0.cmp(&right.0));
            let mut sorted = Map::new();
            allocation.member_nodes(
                "canonical Cargo recursive reordered nodes",
                keyed.len(),
                limits,
            )?;
            for (key, value) in keyed {
                sorted.insert(key, value);
            }
            *object = sorted;
        }
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => {}
    }
    Ok(())
}

#[cfg(test)]
fn json_sort_key(
    value: &Value,
    allocation: &mut JsonAllocation,
    limits: &Limits,
) -> Result<Vec<u8>> {
    struct Counter(usize);
    impl std::io::Write for Counter {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.0 = self
                .0
                .checked_add(bytes.len())
                .ok_or_else(|| std::io::Error::other("JSON sort-key size overflow"))?;
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let mut counter = Counter(0);
    serde_json::to_writer(&mut counter, value).map_err(|error| Error::Schema {
        document: "canonical Cargo JSON",
        reason: error.to_string().into_boxed_str(),
    })?;
    allocation.retained_bytes(
        "canonical Cargo sort-key bytes",
        counter.0,
        limits.json_bytes,
        limits,
    )?;
    let mut bytes = Vec::new();
    bytes
        .try_reserve_exact(counter.0)
        .map_err(|source| Error::Allocation {
            operation: "canonical Cargo JSON",
            source,
        })?;
    serde_json::to_writer(&mut bytes, value).map_err(|error| Error::Schema {
        document: "canonical Cargo JSON",
        reason: error.to_string().into_boxed_str(),
    })?;
    Ok(bytes)
}

fn exact_schema(object: &Map<String, Value>, document: &'static str, expected: u32) -> Result<()> {
    let actual = object
        .get("version")
        .and_then(Value::as_u64)
        .and_then(|version| u32::try_from(version).ok())
        .ok_or_else(|| schema(document, "version is absent or invalid"))?;
    if actual != expected {
        return Err(Error::Toolchain {
            field: if document == "Cargo metadata" {
                "graph schema"
            } else {
                "unit schema"
            },
            expected: expected.to_string().into_boxed_str(),
            actual: actual.to_string().into_boxed_str(),
        });
    }
    Ok(())
}

fn array<'a>(
    object: &'a Map<String, Value>,
    key: &str,
    document: &'static str,
) -> Result<&'a [Value]> {
    object
        .get(key)
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .ok_or_else(|| schema(document, &format!("`{key}` is absent or not an array")))
}

fn string<'a>(
    object: &'a Map<String, Value>,
    key: &str,
    document: &'static str,
) -> Result<&'a str> {
    object
        .get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| schema(document, &format!("`{key}` is absent or not a string")))
}

fn value_string<'a>(value: &'a Value, key: &str) -> Option<&'a str> {
    value.get(key).and_then(Value::as_str)
}

fn schema(document: &'static str, reason: &str) -> Error {
    Error::Schema {
        document,
        reason: reason.into(),
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::Arc;

    use serde_json::{json, Value};

    use super::{
        canonical_metadata_bytes, decode_checksum, git_source_matches, parse_package,
        validate_command_directories, BuildGraph, Metadata, Package, PackageSource,
        OWNED_LOCK_DOCUMENTS,
    };

    fn script_unit(mode: &str, dependencies: &[usize]) -> super::OwnedUnit {
        serde_json::from_value(json!({
            "pkg_id": "registry+https://example.test#index@1.0.0",
            "target": {"kind": ["custom-build"], "crate_types": ["bin"],
                "name": "build-script-build", "src_path": "/fixture/build.rs", "edition": "2021"},
            "profile": {"name": "dev", "opt_level": "0", "debuginfo": 0,
                "debug_assertions": true, "overflow_checks": true},
            "platform": null, "mode": mode, "features": [],
            "dependencies": dependencies.iter().map(|index| json!({"index": index})).collect::<Vec<_>>(),
            "is_std": false
        })).expect("script unit fixture decodes")
    }

    #[test]
    fn script_compiler_follows_edges_even_when_features_are_identical() {
        let mut units = vec![
            script_unit("build", &[]),
            script_unit("build", &[]),
            script_unit("run-custom-build", &[0]),
            script_unit("run-custom-build", &[1]),
        ];
        units[3].platform = Some("aarch64-apple-darwin".into());
        assert_eq!(
            super::script_compiler(&units, &units[2]).expect("host edge"),
            0
        );
        assert_eq!(
            super::script_compiler(&units, &units[3]).expect("target edge"),
            1
        );
        units[2].dependencies[0].index = 1;
        units[3].dependencies[0].index = 0;
        assert_eq!(
            super::script_compiler(&units, &units[2]).expect("swapped host edge"),
            1
        );
        assert_eq!(
            super::script_compiler(&units, &units[3]).expect("swapped target edge"),
            0
        );
    }

    #[test]
    fn script_compiler_rejects_missing_ambiguous_and_non_host_edges() {
        let mut units = vec![script_unit("build", &[]), script_unit("build", &[])];
        for dependencies in [&[][..], &[0, 1][..], &[2][..]] {
            assert!(
                super::script_compiler(&units, &script_unit("run-custom-build", dependencies))
                    .is_err()
            );
        }
        units[0].platform = Some("aarch64-apple-darwin".into());
        assert!(super::script_compiler(&units, &script_unit("run-custom-build", &[0])).is_err());
        units[0].platform = None;
        units[0].pkg_id = "different-package".into();
        assert!(super::script_compiler(&units, &script_unit("run-custom-build", &[0])).is_err());
    }

    static FIXTURE_ID: AtomicU64 = AtomicU64::new(1);

    struct Fixture(PathBuf);

    impl Fixture {
        fn new(name: &str) -> Self {
            let root = std::env::temp_dir().join(format!(
                "rawr-unit-{name}-{}-{}",
                std::process::id(),
                FIXTURE_ID.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir_all(&root).expect("fixture root can be created");
            fs::write(
                root.join("Cargo.toml"),
                "[package]\nname = \"same\"\nversion = \"1.0.0\"\n",
            )
            .expect("fixture manifest can be written");
            Self(fs::canonicalize(root).expect("fixture root canonicalizes"))
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn metadata(packages: impl IntoIterator<Item = Package>) -> Metadata {
        Metadata::fixture_packages(packages)
    }

    #[test]
    fn metadata_identity_and_features_borrow_the_existing_package_map_entries() {
        let package = |id: &str| Package {
            id: id.into(),
            name: id.into(),
            version: "0.1.0".into(),
            manifest: PathBuf::from(format!("/{id}/Cargo.toml")),
            root: PathBuf::from(format!("/{id}")).into(),
            source: PackageSource::Path {
                cargo_id: format!("path+file:///{id}").into(),
            },
            features: Some(Box::default()),
        };
        let mut metadata = metadata([package("z-root"), package("a-root")]);

        assert_eq!(metadata.root_id().expect("first root exists"), "a-root");
        let (package_id, package) = metadata
            .packages
            .get_key_value("z-root")
            .expect("shared package identity exists");
        assert!(
            Arc::ptr_eq(package_id, &package.id),
            "the package table key and retained package share one exact PackageId allocation",
        );
        metadata
            .packages
            .get_mut("z-root")
            .expect("feature package exists")
            .features = Some(vec![Box::<str>::from("shared")].into_boxed_slice());
        assert_eq!(
            metadata
                .packages
                .get("z-root")
                .and_then(|package| package.features.as_deref()),
            Some([Box::<str>::from("shared")].as_slice()),
            "feature authority lives on the package row whose map key already owns its ID",
        );
        metadata.selected.insert("selected".into(), 1);
        metadata.declared.insert("declared".into(), 0);
        assert_eq!(
            metadata
                .package_id(*metadata.selected.get("selected").expect("selected ordinal"))
                .expect("selected package exists"),
            "z-root"
        );
        assert_eq!(
            metadata
                .package_id(*metadata.declared.get("declared").expect("declared ordinal"))
                .expect("declared package exists"),
            "a-root"
        );
        metadata.root = 1;
        assert_eq!(metadata.root_id().expect("second root exists"), "z-root");
        metadata.root = 2;
        assert!(metadata.root_id().is_err());
    }

    #[test]
    fn registry_shorthand_and_cargo_canonical_requirement_are_semantic_equals() {
        let shorthand = semver::VersionReq::parse("1").expect("test requirement parses");
        let canonical = semver::VersionReq::parse("^1").expect("Cargo form parses");
        assert_eq!(shorthand, canonical);
    }

    #[test]
    fn git_source_id_requires_prefix_url_and_exact_requested_revision() {
        let revision = "23f021145eff7dfd420e8f3739b742e902568678";
        let source = format!("git+https://example.invalid/repo?rev={revision}#{revision}");
        assert!(git_source_matches(
            &source,
            "https://example.invalid/repo",
            revision
        ));
        assert!(!git_source_matches(
            source.strip_prefix("git+").expect("prefix exists"),
            "https://example.invalid/repo",
            revision
        ));
    }

    #[test]
    fn registry_lock_join_uses_full_name_version_source_identity() {
        let first_root = Fixture::new("registry-one");
        let second_root = Fixture::new("registry-two");
        let first_source = "registry+https://one.invalid/index";
        let second_source = "registry+https://two.invalid/index";
        let first_id = format!("{first_source}#same@1.0.0");
        let second_id = format!("{second_source}#same@1.0.0");
        let metadata = metadata([
            Package::fixture(
                &first_id,
                "same",
                "1.0.0",
                first_root.0.clone(),
                PackageSource::Registry {
                    cargo_id: first_source.into(),
                    checksum: None,
                },
            ),
            Package::fixture(
                &second_id,
                "same",
                "1.0.0",
                second_root.0.clone(),
                PackageSource::Registry {
                    cargo_id: second_source.into(),
                    checksum: None,
                },
            ),
        ]);
        let lock = format!(
            "version = 4\n\n[[package]]\nname = \"same\"\nversion = \"1.0.0\"\nsource = \"{first_source}\"\nchecksum = \"{}\"\n\n[[package]]\nname = \"same\"\nversion = \"1.0.0\"\nsource = \"{second_source}\"\nchecksum = \"{}\"\n",
            "11".repeat(32),
            "22".repeat(32),
        );
        let limits = super::Limits::standard();
        let mut allocation = super::Allocation::begin(&limits);
        let bound = metadata
            .bind_lock(lock.as_bytes(), &limits, &mut allocation)
            .expect("full source identity disambiguates equal name/version packages");
        assert!(matches!(
            bound.package(&first_id).map(Package::source),
            Some(PackageSource::Registry { checksum: Some(value), .. }) if *value == [0x11; 32]
        ));
        assert!(matches!(
            bound.package(&second_id).map(Package::source),
            Some(PackageSource::Registry { checksum: Some(value), .. }) if *value == [0x22; 32]
        ));
    }

    #[test]
    fn lock_dom_is_charged_before_ownership_at_exact_epoch_caps() {
        let fixture = Fixture::new("registry-lock-budget");
        let source = "registry+https://one.invalid/index";
        let id = format!("{source}#same@1.0.0");
        let lock = format!(
            "version = 4\n\n[[package]]\nname = \"same\"\nversion = \"1.0.0\"\nsource = \"{source}\"\nchecksum = \"{}\"\n",
            "11".repeat(32),
        );
        let exercise = |limits: &super::Limits| {
            OWNED_LOCK_DOCUMENTS.with(|owned| owned.set(0));
            let package = Package::fixture(
                &id,
                "same",
                "1.0.0",
                fixture.0.clone(),
                PackageSource::Registry {
                    cargo_id: source.into(),
                    checksum: None,
                },
            );
            let mut allocation = super::Allocation::begin(limits);
            let bound = metadata([package]).bind_lock(lock.as_bytes(), limits, &mut allocation);
            (bound, allocation)
        };

        let mut exact = super::Limits::standard();
        exact.items = lock.len().saturating_add(1);
        exact.json_bytes = lock.len();
        exact.manifest_bytes = lock.len();
        let (bound, allocation) = exercise(&exact);
        bound.expect("the exact lock DOM allocation seals");
        assert_eq!(OWNED_LOCK_DOCUMENTS.with(std::cell::Cell::get), 1);
        assert!(!allocation.usage().exceedances().is_exceeded());

        // One item below the exact DOM precharge still binds the lock and
        // owns the DOM; the aggregate item axis records the exact precharge
        // (every TOML byte as a node plus the retained buffer) as high water.
        let mut item_floor = exact;
        item_floor.items = item_floor.items.saturating_sub(1);
        let (bound, allocation) = exercise(&item_floor);
        bound.expect("the item floor admits the lock DOM");
        assert_eq!(OWNED_LOCK_DOCUMENTS.with(std::cell::Cell::get), 1);
        let axis = allocation
            .usage()
            .exceedances()
            .axis(super::AGGREGATE_ITEMS)
            .copied()
            .expect("the aggregate item axis records the lock DOM precharge");
        assert_eq!(
            (axis.target, axis.first_requested, axis.high_water),
            (lock.len() as u64, exact.items as u64, exact.items as u64)
        );
        // The whole-document byte target is recorded on the epoch before
        // any DOM ownership and never refuses; the lock still binds and the
        // DOM is owned exactly once. The physical bound is the capture that
        // read the lock, not this target.
        let mut byte_floor = exact;
        byte_floor.json_bytes = byte_floor.json_bytes.saturating_sub(1);
        let (bound, allocation) = exercise(&byte_floor);
        bound.expect("one byte under the lock size still binds the lock");
        assert_eq!(OWNED_LOCK_DOCUMENTS.with(std::cell::Cell::get), 1);
        let axis = allocation
            .usage()
            .exceedances()
            .axis("Cargo lock bytes")
            .copied()
            .expect("the lock document size records its axis");
        assert_eq!(
            (axis.target, axis.first_requested, axis.high_water),
            (
                (lock.len() - 1) as u64,
                lock.len() as u64,
                lock.len() as u64
            )
        );
    }

    #[test]
    fn registry_lock_join_rejects_missing_malformed_mismatched_or_ambiguous_checksum() {
        let fixture = Fixture::new("registry-negative");
        let source = "registry+https://one.invalid/index";
        let id = format!("{source}#same@1.0.0");
        let package = || {
            Package::fixture(
                &id,
                "same",
                "1.0.0",
                fixture.0.clone(),
                PackageSource::Registry {
                    cargo_id: source.into(),
                    checksum: Some([0x11; 32]),
                },
            )
        };
        for lock in [
            format!(
                "version = 4\n\n[[package]]\nname = \"same\"\nversion = \"1.0.0\"\nsource = \"{source}\"\n"
            ),
            format!(
                "version = 4\n\n[[package]]\nname = \"same\"\nversion = \"1.0.0\"\nsource = \"{source}\"\nchecksum = \"not-a-checksum\"\n"
            ),
            format!(
                "version = 4\n\n[[package]]\nname = \"same\"\nversion = \"1.0.0\"\nsource = \"{source}\"\nchecksum = \"{}\"\n",
                "22".repeat(32)
            ),
            format!(
                "version = 4\n\n[[package]]\nname = \"same\"\nversion = \"1.0.0\"\nsource = \"{source}\"\nchecksum = \"{}\"\n\n[[package]]\nname = \"same\"\nversion = \"1.0.0\"\nsource = \"{source}\"\nchecksum = \"{}\"\n",
                "11".repeat(32),
                "11".repeat(32),
            ),
        ] {
            let limits = super::Limits::standard();
            let mut allocation = super::Allocation::begin(&limits);
            assert!(metadata([package()])
                .bind_lock(lock.as_bytes(), &limits, &mut allocation)
                .is_err());
        }
        assert!(decode_checksum("not-a-checksum").is_err());
    }

    #[test]
    fn metadata_package_source_shape_rejects_source_mismatch_and_inexact_git_oid() {
        let fixture = Fixture::new("source-shape");
        let manifest = fixture.0.join("Cargo.toml");
        let mismatched = json!({
            "id": "registry+https://two.invalid/index#same@1.0.0",
            "name": "same",
            "version": "1.0.0",
            "manifest_path": manifest,
            "source": "registry+https://one.invalid/index",
            "checksum": null,
        });
        assert!(parse_package(&mismatched).is_err());

        let inexact = json!({
            "id": "git+https://example.invalid/repo#same@1.0.0",
            "name": "same",
            "version": "1.0.0",
            "manifest_path": fixture.0.join("Cargo.toml"),
            "source": "git+https://example.invalid/repo#abc",
            "checksum": null,
        });
        assert!(parse_package(&inexact).is_err());
    }

    #[test]
    fn path_metadata_reconstructs_the_real_cargo_package_id_shape_exactly() {
        let fixture = Fixture::new("path-source-shape");
        let source_base = format!("path+file://{}", fixture.0.display());
        let package = |id: String, name: &str, version: &str, source: Value, checksum: Value| {
            json!({
                "id": id,
                "name": name,
                "version": version,
                "manifest_path": fixture.0.join("Cargo.toml"),
                "source": source,
                "checksum": checksum,
            })
        };
        let valid = package(
            format!("{source_base}#same@1.0.0"),
            "same",
            "1.0.0",
            Value::Null,
            Value::Null,
        );
        parse_package(&valid).expect("Cargo's real path metadata shape seals");

        let compact_root = fixture.0.join("same");
        fs::create_dir(&compact_root).expect("compact path package root is created");
        fs::write(
            compact_root.join("Cargo.toml"),
            "[package]\nname='same'\nversion='1.0.0'\n",
        )
        .expect("compact path manifest is written");
        let compact_source = format!("path+file://{}", compact_root.display());
        let compact = json!({
            "id": format!("{compact_source}#1.0.0"),
            "name": "same",
            "version": "1.0.0",
            "manifest_path": compact_root.join("Cargo.toml"),
            "source": Value::Null,
            "checksum": Value::Null,
        });
        parse_package(&compact).expect("Cargo's compact basename-matched path shape seals");

        for invalid in [
            package(
                "path+file:///not-the-package#same@1.0.0".into(),
                "same",
                "1.0.0",
                Value::Null,
                Value::Null,
            ),
            package(
                format!("{source_base}#other@1.0.0"),
                "same",
                "1.0.0",
                Value::Null,
                Value::Null,
            ),
            package(
                format!("{source_base}#same@2.0.0"),
                "same",
                "1.0.0",
                Value::Null,
                Value::Null,
            ),
            package(
                format!("{source_base}#same"),
                "same",
                "1.0.0",
                Value::Null,
                Value::Null,
            ),
            package(
                format!("{source_base}#1.0.0"),
                "same",
                "1.0.0",
                Value::Null,
                Value::Null,
            ),
            package(
                format!("{source_base}#same@1.0.0"),
                "same",
                "1.0.0",
                Value::String("registry+https://example.invalid/index".into()),
                Value::Null,
            ),
            package(
                format!("{source_base}#same@1.0.0"),
                "same",
                "1.0.0",
                Value::Null,
                Value::String("11".repeat(32)),
            ),
        ] {
            assert!(parse_package(&invalid).is_err());
        }
    }

    #[test]
    fn registry_metadata_reconstructs_the_real_cargo_package_id_shape_exactly() {
        let fixture = Fixture::new("registry-source-shape");
        let source = "registry+https://github.com/rust-lang/crates.io-index";
        let package = |id: String, name: &str, version: &str, source: &str, checksum: Value| {
            json!({
                "id": id,
                "name": name,
                "version": version,
                "manifest_path": fixture.0.join("Cargo.toml"),
                "source": source,
                "checksum": checksum,
            })
        };
        let valid = package(
            format!("{source}#same@1.0.0"),
            "same",
            "1.0.0",
            source,
            Value::String("11".repeat(32)),
        );
        parse_package(&valid).expect("Cargo's real registry metadata shape seals");

        for invalid in [
            package(
                "registry+https://other.invalid/index#same@1.0.0".into(),
                "same",
                "1.0.0",
                source,
                Value::Null,
            ),
            package(
                format!("{source}#other@1.0.0"),
                "same",
                "1.0.0",
                source,
                Value::Null,
            ),
            package(
                format!("{source}#same@2.0.0"),
                "same",
                "1.0.0",
                source,
                Value::Null,
            ),
            package(
                format!("{source}#same@1.0.0"),
                "same",
                "1.0.0",
                source,
                Value::String("not-a-checksum".into()),
            ),
        ] {
            assert!(parse_package(&invalid).is_err());
        }
    }

    #[test]
    fn publication_replay_uses_the_same_exact_package_identity_parser() {
        let fixture = Fixture::new("publication-package-id");
        let source = "registry+https://github.com/rust-lang/crates.io-index";
        let id = format!("{source}#same@1.0.0");
        let value = json!({
            "id": id,
            "name": "same",
            "version": "1.0.0",
            "manifest_path": fixture.0.join("Cargo.toml"),
            "source": source,
            "checksum": "11".repeat(32),
        });
        let initial = parse_package(&value).expect("initial metadata package identity seals");
        let publication = parse_package(&value).expect("publication replay package identity seals");
        assert_eq!(initial, publication);

        let mut drifted = value;
        drifted["id"] = Value::String(format!("{source}#same@1.0.1"));
        assert!(parse_package(&drifted).is_err());
    }

    #[test]
    fn git_metadata_reconstructs_the_real_cargo_package_id_shape_exactly() {
        let fixture = Fixture::new("git-source-shape");
        let oid = "23f021145eff7dfd420e8f3739b742e902568678";
        let source_base =
            "git+https://example.invalid/repo?rev=23f021145eff7dfd420e8f3739b742e902568678";
        let source = format!("{source_base}#{oid}");
        let package = |id: String, name: &str, version: &str, source: String, checksum: Value| {
            json!({
                "id": id,
                "name": name,
                "version": version,
                "manifest_path": fixture.0.join("Cargo.toml"),
                "source": source,
                "checksum": checksum,
            })
        };
        let valid = package(
            format!("{source_base}#same@1.0.0"),
            "same",
            "1.0.0",
            source.clone(),
            Value::Null,
        );
        parse_package(&valid).expect("Cargo's real Git metadata shape seals");
        let compact_source_base =
            "git+https://example.invalid/same?rev=23f021145eff7dfd420e8f3739b742e902568678";
        let compact = package(
            format!("{compact_source_base}#1.0.0"),
            "same",
            "1.0.0",
            format!("{compact_source_base}#{oid}"),
            Value::Null,
        );
        parse_package(&compact).expect("Cargo's repository-named compact Git PackageId seals");

        let negatives = [
            package(
                "git+https://other.invalid/repo#same@1.0.0".into(),
                "same",
                "1.0.0",
                source.clone(),
                Value::Null,
            ),
            package(
                format!("{source_base}#other@1.0.0"),
                "same",
                "1.0.0",
                source.clone(),
                Value::Null,
            ),
            package(
                format!("{source_base}#same@2.0.0"),
                "same",
                "1.0.0",
                source.clone(),
                Value::Null,
            ),
            package(
                format!("{source}#same@1.0.0"),
                "same",
                "1.0.0",
                source.clone(),
                Value::Null,
            ),
            package(
                format!("{source_base}#same@1.0.0"),
                "same",
                "1.0.0",
                source.clone(),
                Value::String("11".repeat(32)),
            ),
            package(
                format!("{source_base}#same@1.0.0"),
                "same",
                "1.0.0",
                format!("{source_base}#abc"),
                Value::Null,
            ),
            package(
                format!("{source_base}#same@1.0.0"),
                "same",
                "1.0.0",
                format!("{source_base}#{}", "g".repeat(40)),
                Value::Null,
            ),
        ];
        for invalid in negatives {
            assert!(parse_package(&invalid).is_err());
        }
    }

    #[test]
    fn metadata_directories_bind_distinct_layout_outputs_without_weakening_legacy() {
        let target = Path::new("/fixture/target/final");
        let build = Path::new("/fixture/build/final");
        let value = json!({
            "target_directory": target,
            "build_directory": build,
        });
        let object = value.as_object().expect("directory fixture is an object");
        super::validate_command_directory_pair(object, target, build)
            .expect("distinct assigned directories match");
        assert!(validate_command_directories(object, target).is_err());
        for (actual_target, actual_build, field) in [
            (build, build, "Cargo target directory"),
            (target, target, "Cargo build directory"),
        ] {
            let value = json!({
                "target_directory": actual_target,
                "build_directory": actual_build,
            });
            let error = super::validate_command_directory_pair(
                value.as_object().expect("drift fixture is an object"),
                target,
                build,
            )
            .expect_err("either directory changing rejects");
            assert!(matches!(error, crate::Error::ResolutionDrift {
                command: "metadata", field: actual,
            } if actual == field));
        }
    }

    #[test]
    fn canonical_unit_and_validated_metadata_bytes_are_equality_authority() {
        let initial = BuildGraph {
            raw: super::RawDocument::from_bytes(b"a"),
            _allocation: super::JsonAllocation::empty(),
            limits: super::Limits::standard(),
            canonical: super::digest(b"a"),
            artifacts: Box::default(),
            scripts: Box::default(),
            #[cfg(feature = "rustc-driver")]
            extraction: Box::default(),
        };
        let drifted = BuildGraph {
            raw: super::RawDocument::from_bytes(b"b"),
            _allocation: super::JsonAllocation::empty(),
            limits: super::Limits::standard(),
            canonical: super::digest(b"b"),
            artifacts: Box::default(),
            scripts: Box::default(),
            #[cfg(feature = "rustc-driver")]
            extraction: Box::default(),
        };
        assert!(!initial.equals(&drifted));
        initial
            .require_exact_raw(b"a", "post-build fixture")
            .expect("the exact raw unit replay seals");
        assert!(initial
            .require_exact_raw(b"b", "post-build fixture")
            .is_err());

        let metadata = metadata([]);
        metadata
            .require_exact_raw(&[], "post-build fixture")
            .expect("the exact raw metadata replay seals");
        assert!(metadata
            .require_exact_raw(b"drift", "post-build fixture")
            .is_err());

        let resolution = json!({
            "target_directory": "/fixture/resolution-target",
            "build_directory": "/fixture/resolution-target",
        });
        let authoritative = json!({
            "target_directory": "/fixture/authoritative-target",
            "build_directory": "/fixture/authoritative-target",
        });
        validate_command_directories(
            resolution
                .as_object()
                .expect("the metadata fixture is an object"),
            Path::new("/fixture/resolution-target"),
        )
        .expect("both resolution command directories match exactly");
        validate_command_directories(
            authoritative
                .as_object()
                .expect("the metadata fixture is an object"),
            Path::new("/fixture/authoritative-target"),
        )
        .expect("both authoritative command directories match exactly");
        assert_eq!(
            canonical_metadata_bytes(resolution).expect("the resolution metadata canonicalizes"),
            canonical_metadata_bytes(authoritative)
                .expect("the authoritative metadata canonicalizes")
        );

        let mismatch = json!({
            "target_directory": "/fixture/authoritative-target",
            "build_directory": "/fixture/other-build-target",
        });
        let error = validate_command_directories(
            mismatch
                .as_object()
                .expect("the mismatched metadata fixture is an object"),
            Path::new("/fixture/authoritative-target"),
        )
        .expect_err("an independently redirected build directory rejects");
        assert!(format!("{error}").contains("Cargo build directory"));
    }
}
