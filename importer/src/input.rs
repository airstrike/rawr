//! Content-addressed normal-build input and build-script evidence.

use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsString;
use std::fs::{self, File};
use std::io::Read;
use std::path::{Component, Path, PathBuf};

use serde_json::Value;
use sha2::{Digest as _, Sha256};

use crate::cargo::{self, toolchain};
use crate::{Error, Result};

use self::build::{Artifact, Build, Evidence, Key, Package, Script};
use self::complete::Complete;
use self::config::Config;
use self::proof::{Recipe, Role};

mod artifact;
pub(crate) mod build;
pub(crate) mod canonical;
pub(crate) mod complete;
pub(crate) mod config;
pub mod declaration;
mod dependency;
#[cfg(feature = "rustc-driver")]
pub(crate) use dependency::scan::{escape_environment, escape_path};
pub mod fingerprint;
#[cfg(test)]
mod membership;
pub(crate) mod native;
pub(crate) mod package;
pub mod proof;
mod replay;
pub(crate) mod scalar;
mod script;
pub(crate) mod stored;
pub(crate) mod workspace;

const LINE_BYTES: usize = 1024 * 1024;

/// Fixed chunk through which a freshness digest streams its file.
const DIGEST_CHUNK_BYTES: usize = 32 * 1024;

#[cfg(test)]
std::thread_local! {
    static OWNED_ACCOUNTED_READS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// A complete revalidatable baseline born only from a successful Cargo build.
pub(crate) struct Baseline {
    proof: proof::Proof,
    recipes: Box<[proof::Recipe]>,
    recipe_allocation: Usage,
    allocation: Allocation,
    universe_build: Option<cargo::universe::BuildProjection>,
    package_replay: Option<cargo::execution::Scope>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct Usage {
    pub(super) items: usize,
    pub(super) bytes: usize,
}

#[cfg(feature = "import-baseline")]
impl Usage {
    pub(crate) const fn observation(self) -> (usize, usize) {
        (self.items, self.bytes)
    }
}

struct HexDigest<'digest>(&'digest [u8; 32]);

impl std::fmt::Display for HexDigest<'_> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        for byte in self.0 {
            write!(formatter, "{byte:02x}")?;
        }
        Ok(())
    }
}

pub(super) struct Allocation {
    pub(super) usage: Usage,
    item_limit: usize,
    byte_limit: usize,
    string_limit: usize,
    /// Set only by genuine allocation failure, checked-arithmetic overflow,
    /// observation allocation failure or a physical `Error::Limit`.
    refused: bool,
    /// Process-unique account identity for exceedance reporting.
    id: u64,
    exceeded: crate::exceedance::Record,
}

pub(crate) const INPUT_ITEMS_RESOURCE: &str = "input aggregate owned items";
pub(crate) const INPUT_BYTES_RESOURCE: &str = "input aggregate owned bytes";
pub(crate) const INPUT_STRING_RESOURCE: &str = "input owned string bytes";

impl Allocation {
    #[cfg(feature = "import-baseline")]
    fn observe_capture<T>(&self, result: &Result<T>) {
        use crate::metrics::{self, accounting::OwnerStage, accounting::Scalar};

        metrics::record_scalar(Scalar::OwnerInputItems, self.usage.items as u64);
        metrics::record_scalar(Scalar::OwnerInputBytes, self.usage.bytes as u64);
        let Err(mut error) = result.as_ref() else {
            return;
        };
        let stage = OwnerStage::InputCapture;
        metrics::record_scalar(Scalar::OwnerRefusals(stage), 1);
        let (actual, limit) = loop {
            match error {
                Error::Limit { actual, limit, .. } => break (*actual, *limit),
                Error::Node { source, .. } => match source.as_ref() {
                    crate::node::Error::Limit { actual, limit, .. } => break (*actual, *limit),
                    crate::node::Error::Importer(inner) => error = inner,
                    _ => return,
                },
                _ => return,
            }
        };
        metrics::record_scalar(Scalar::OwnerRefusedRequired(stage), actual as u64);
        metrics::record_scalar(Scalar::OwnerRefusedLimit(stage), limit as u64);
    }

    /// Start one current stored-input validation account before any bytes decode.
    /// Callers retain it across all work in the supplied graph validation epoch.
    pub(crate) fn for_replay(limits: &cargo::Limits) -> Self {
        Self::for_proof(Usage { items: 0, bytes: 0 }, limits)
    }

    /// Join two actual spent accounts without refunding prior lookup or cold work.
    pub(crate) fn absorb(&mut self, other: Self) -> Result<()> {
        if (self.item_limit, self.byte_limit, self.string_limit)
            != (other.item_limit, other.byte_limit, other.string_limit)
        {
            self.refused = true;
            return Err(input(
                "input-account-limits",
                "cannot join different account ceilings",
            ));
        }
        if self.refused || other.refused {
            self.refused = true;
            return Err(Error::InputAccountRefused);
        }
        let Some(items) = self.usage.items.checked_add(other.usage.items) else {
            self.refused = true;
            return Err(Error::InputAccountRefused);
        };
        let Some(bytes) = self.usage.bytes.checked_add(other.usage.bytes) else {
            self.refused = true;
            return Err(Error::InputAccountRefused);
        };
        self.usage = Usage { items, bytes };
        if other.exceeded.is_exceeded() {
            self.exceeded.absorb(&other.exceeded);
            self.publish();
        }
        self.exceeded(INPUT_ITEMS_RESOURCE, self.item_limit, items);
        self.exceeded(INPUT_BYTES_RESOURCE, self.byte_limit, bytes);
        Ok(())
    }

    fn new(limits: &cargo::Limits) -> Self {
        Self::with_limits(limits.items, limits.json_bytes, limits.string_bytes)
    }

    fn from_usage(usage: Usage, limits: &cargo::Limits) -> Self {
        Self {
            usage,
            ..Self::with_limits(limits.items, limits.json_bytes, limits.string_bytes)
        }
    }

    fn for_proof(usage: Usage, limits: &cargo::Limits) -> Self {
        Self {
            usage,
            ..Self::with_limits(limits.items, limits.input_proof_bytes, limits.string_bytes)
        }
    }

    fn with_limits(item_limit: usize, byte_limit: usize, string_limit: usize) -> Self {
        Self {
            usage: Usage { items: 0, bytes: 0 },
            item_limit,
            byte_limit,
            string_limit,
            refused: false,
            id: crate::exceedance::next_id(),
            exceeded: crate::exceedance::Record::new(),
        }
    }

    /// The axes of this account that crossed their targets so far.
    #[cfg(test)]
    pub(crate) const fn exceedances(&self) -> crate::exceedance::Record {
        self.exceeded
    }

    fn publish(&self) {
        crate::exceedance::publish(crate::exceedance::Account::Input, self.id, &self.exceeded);
    }

    /// Crossing an accounting target is recorded and published, never
    /// refused; the charge has already been added to the original account.
    pub(super) fn exceeded(&mut self, resource: &'static str, target: usize, actual: usize) {
        if actual > target
            && self
                .exceeded
                .exceeded(resource, target as u64, actual as u64, None)
        {
            self.publish();
        }
    }

    /// Compare one string against the string target, recording an exceedance.
    pub(super) fn string_exceeded(&mut self, resource: &'static str, bytes: usize) {
        self.exceeded(resource, self.string_limit, bytes);
    }

    /// Compare one count against the item target, recording an exceedance.
    pub(super) fn items_exceeded(&mut self, resource: &'static str, count: usize) {
        self.exceeded(resource, self.item_limit, count);
    }

    /// The latched error for a checked-arithmetic overflow in this account.
    fn overflow(&mut self, operation: &'static str) -> Error {
        self.refused = true;
        Error::Node {
            operation,
            source: Box::new(crate::node::Error::Allocation),
        }
    }

    /// Charge owned items. The sum is checked: overflow is a genuine failure
    /// that latches the account and stays terminal; saturating
    /// arithmetic is not a substitute. Crossing the item target only records.
    pub(super) fn items(&mut self, amount: usize) -> Result<()> {
        self.check()?;
        let actual = self
            .usage
            .items
            .checked_add(amount)
            .ok_or_else(|| self.overflow(INPUT_ITEMS_RESOURCE))?;
        self.usage.items = actual;
        self.exceeded(INPUT_ITEMS_RESOURCE, self.item_limit, actual);
        Ok(())
    }

    /// Charge owned bytes with the same checked sum and target recording.
    pub(super) fn bytes(&mut self, amount: usize) -> Result<()> {
        self.check()?;
        let actual = self
            .usage
            .bytes
            .checked_add(amount)
            .ok_or_else(|| self.overflow(INPUT_BYTES_RESOURCE))?;
        self.usage.bytes = actual;
        self.exceeded(INPUT_BYTES_RESOURCE, self.byte_limit, actual);
        Ok(())
    }

    pub(super) fn string(&mut self, value: &str) -> Result<()> {
        self.check()?;
        self.exceeded(INPUT_STRING_RESOURCE, self.string_limit, value.len());
        self.items(1)?;
        self.bytes(value.len())
    }

    pub(super) fn path(&mut self, value: &Path) -> Result<()> {
        self.items(1)?;
        self.bytes(value.as_os_str().len())
    }

    fn formatted(&mut self, arguments: std::fmt::Arguments<'_>) -> Result<String> {
        self.check()?;
        struct Counter(usize);
        impl std::fmt::Write for Counter {
            fn write_str(&mut self, value: &str) -> std::fmt::Result {
                self.0 = self.0.checked_add(value.len()).ok_or(std::fmt::Error)?;
                Ok(())
            }
        }
        let retained = arguments;
        let mut counter = Counter(0);
        std::fmt::write(&mut counter, arguments).map_err(|_| {
            self.refused = true;
            Error::Node {
                operation: "input formatted length",
                source: Box::new(crate::node::Error::Allocation),
            }
        })?;
        self.items(1)?;
        self.bytes(counter.0)?;
        let mut value = String::new();
        value.try_reserve_exact(counter.0).map_err(|source| {
            self.refused = true;
            Error::Allocation {
                operation: "input formatted string",
                source,
            }
        })?;
        std::fmt::write(&mut value, retained).map_err(|_| Error::Input {
            code: "input-format-render",
            reason: "formatted freshness string rendering failed".into(),
        })?;
        Ok(value)
    }

    const fn usage(&self) -> Usage {
        self.usage
    }

    fn check(&self) -> Result<()> {
        if self.refused {
            Err(Error::InputAccountRefused)
        } else {
            Ok(())
        }
    }

    /// Latch this account on a genuine failure (test-only cause injection).
    #[cfg(test)]
    pub(crate) fn refuse(&mut self) {
        self.refused = true;
    }
}

/// Cargo document parsers measure their string targets against the input
/// account that owns the parsed message.
impl cargo::unit::Measure for Allocation {
    fn measured(&mut self, resource: &'static str, target: usize, actual: usize) {
        self.exceeded(resource, target, actual);
    }
}

#[cfg(feature = "rustc-driver")]
#[derive(Clone, Eq, PartialEq)]
pub(crate) struct ArtifactAuthority {
    pub(crate) semantic_key: Box<str>,
    pub(crate) package_id: Box<str>,
    pub(crate) target: PathBuf,
    pub(crate) target_kinds: Box<[Box<str>]>,
    pub(crate) crate_types: Box<[Box<str>]>,
    pub(crate) crate_name: Box<str>,
    pub(crate) features: Box<[Box<str>]>,
}

#[cfg(feature = "rustc-driver")]
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) struct SelectedArtifact {
    pub(crate) semantic_key: Box<str>,
    pub(crate) package_id: Box<str>,
    pub(crate) target: PathBuf,
    pub(crate) target_kinds: Box<[Box<str>]>,
    pub(crate) crate_types: Box<[Box<str>]>,
    pub(crate) crate_name: Box<str>,
    pub(crate) features: Box<[Box<str>]>,
    pub(crate) profile: cargo::unit::ArtifactProfile,
    pub(crate) fresh: bool,
    pub(crate) rmeta: PathBuf,
    pub(crate) length: u64,
    pub(crate) digest: [u8; 32],
}

#[cfg(feature = "rustc-driver")]
pub(crate) enum ArtifactJoin {
    Missing,
    Ambiguous,
    Available(ArtifactAuthority),
}

#[derive(Clone, Copy)]
enum JsonContainer {
    Array { expects_value: bool },
    Object { expects_key: bool },
}

impl Evidence {
    #[cfg(feature = "rustc-driver")]
    pub(crate) fn probe_directories(
        &self,
        graph: &cargo::unit::BuildGraph,
        limits: &cargo::Limits,
        allocation: &mut crate::extract::Allocation,
    ) -> Result<Box<[PathBuf]>> {
        let mut directories = Vec::new();
        for artifact in &self.artifacts {
            for output in &artifact.outputs {
                if !graph
                    .artifacts()
                    .iter()
                    .any(|unit| compare_artifact(unit, artifact) == Ordering::Equal)
                {
                    return Err(input("missing-probe-output-join", output.display()));
                }
                let parent = output
                    .parent()
                    .ok_or_else(|| input("probe-output-parent", output.display()))?;
                allocation.exceeded(
                    "metadata probe dependency directories",
                    limits.units,
                    directories.len().saturating_add(1),
                );
                allocation.reserve(&mut directories, 1)?;
                directories.push(allocation.clone_path(parent)?);
            }
        }
        directories.sort_unstable();
        directories.dedup();
        Ok(directories.into_boxed_slice())
    }

    #[cfg(feature = "rustc-driver")]
    pub(crate) fn selected_artifacts(
        &self,
        graph: &cargo::unit::BuildGraph,
        limits: &cargo::Limits,
        allocation: &mut crate::extract::Allocation,
    ) -> Result<Box<[SelectedArtifact]>> {
        let mut selected = Vec::new();
        for unit in graph.extraction() {
            allocation.exceeded(
                "selected rmeta artifacts",
                limits.packages,
                selected.len().saturating_add(1),
            );
            let mut unit_matches = graph
                .artifacts()
                .iter()
                .filter(|output| output.semantic_key() == unit.semantic_key());
            let output = unit_matches
                .next()
                .ok_or_else(|| input("missing-selected-unit-output", unit.semantic_key()))?;
            if unit_matches.next().is_some() {
                return Err(input("ambiguous-selected-unit-output", unit.semantic_key()));
            }
            let mut matches = self
                .artifacts
                .iter()
                .filter(|artifact| compare_artifact(output, artifact) == Ordering::Equal);
            let artifact = matches
                .next()
                .ok_or_else(|| input("missing-selected-artifact", unit.semantic_key()))?;
            if matches.next().is_some() {
                return Err(input("ambiguous-selected-artifact", unit.semantic_key()));
            }
            let mut metadata = artifact
                .outputs
                .iter()
                .filter(|output| output.extension().is_some_and(|value| value == "rmeta"));
            let rmeta = metadata
                .next()
                .ok_or_else(|| input("missing-selected-rmeta", unit.semantic_key()))?;
            if metadata.next().is_some() {
                return Err(input("ambiguous-selected-rmeta", unit.semantic_key()));
            }
            let authority = artifact_authority(output, artifact, allocation)?;
            if authority.semantic_key.as_ref() != unit.semantic_key() {
                return Err(input("selected-semantic-key-mismatch", unit.semantic_key()));
            }
            // A compiler artifact read binds to the physical capture bound,
            // never to the configurable JSON target.
            let bytes = read_file_accounted(
                "selected rmeta artifact",
                rmeta,
                crate::process::CAPTURE_BYTES,
                allocation,
            )?;
            let length = u64::try_from(bytes.len())
                .map_err(|_| input("selected-rmeta-length", rmeta.display()))?;
            allocation.reserve(&mut selected, 1)?;
            allocation.items(1)?;
            allocation.bytes(artifact.profile.as_bytes().len())?;
            selected.push(SelectedArtifact {
                semantic_key: authority.semantic_key,
                package_id: authority.package_id,
                target: authority.target,
                target_kinds: authority.target_kinds,
                crate_types: authority.crate_types,
                crate_name: authority.crate_name,
                features: authority.features,
                profile: artifact.profile.clone(),
                fresh: artifact.fresh,
                rmeta: allocation.clone_path(rmeta)?,
                length,
                digest: Sha256::digest(bytes).into(),
            });
        }
        selected.sort_unstable();
        if selected
            .windows(2)
            .any(|pair| pair[0].semantic_key == pair[1].semantic_key)
        {
            return Err(input(
                "duplicate-selected-semantic-unit",
                "selected normal-library unit",
            ));
        }
        Ok(selected.into_boxed_slice())
    }

    #[cfg(feature = "rustc-driver")]
    pub(crate) fn authority_for_artifact(
        &self,
        graph: &cargo::unit::BuildGraph,
        artifact: &Path,
        allocation: &mut crate::extract::Allocation,
    ) -> Result<ArtifactJoin> {
        let matches = self
            .artifacts
            .iter()
            .filter(|candidate| candidate.outputs.iter().any(|output| output == artifact));
        let mut authorities = Vec::new();
        for package in matches {
            for unit in graph.artifacts() {
                if compare_artifact(unit, package) != Ordering::Equal
                    || !graph.artifact_selected_reachable(unit.semantic_key())?
                {
                    continue;
                }
                authorities.push((
                    unit.platform(),
                    artifact_authority(unit, package, allocation)?,
                ));
            }
        }
        authorities.sort_unstable_by(|left, right| left.1.semantic_key.cmp(&right.1.semantic_key));
        authorities.dedup_by(|left, right| left == right);
        if authorities.is_empty() {
            return Ok(ArtifactJoin::Missing);
        }
        if authorities.len() > 1 {
            // Cargo's public artifact projection omits the platform, so a host
            // unit and a target unit of one package can share it. Cargo places
            // target-platform output under its triple directory; keep the units
            // whose platform agrees with the artifact's location.
            let under = |triple: &str| {
                artifact
                    .components()
                    .any(|component| component.as_os_str() == triple)
            };
            let triples = authorities
                .iter()
                .filter_map(|(platform, _)| *platform)
                .collect::<Vec<_>>();
            authorities.retain(|(platform, _)| match platform {
                Some(triple) => under(triple),
                None => !triples.iter().any(|triple| under(triple)),
            });
        }
        let Some((_, authority)) = authorities.pop() else {
            return Ok(ArtifactJoin::Ambiguous);
        };
        if !authorities.is_empty() {
            return Ok(ArtifactJoin::Ambiguous);
        }
        Ok(ArtifactJoin::Available(authority))
    }
    #[cfg(all(feature = "rustc-driver", test))]
    pub(crate) fn validate_extern_artifact(
        &self,
        graph: &cargo::unit::BuildGraph,
        semantic_key: &str,
        package_id: &str,
        target: &Path,
        target_kinds: &[Box<str>],
        crate_types: &[Box<str>],
        crate_name: &str,
        features: &[Box<str>],
        artifact: &Path,
    ) -> Result<()> {
        let mut allocation = crate::extract::Allocation::with_limits(usize::MAX, usize::MAX);
        match self.authority_for_artifact(graph, artifact, &mut allocation)? {
            ArtifactJoin::Available(authority)
                if authority.semantic_key.as_ref() == semantic_key
                    && authority.package_id.as_ref() == package_id
                    && authority.target == target
                    && authority.target_kinds.as_ref() == target_kinds
                    && authority.crate_types.as_ref() == crate_types
                    && authority.crate_name.as_ref() == crate_name
                    && authority.features.as_ref() == features =>
            {
                Ok(())
            }
            ArtifactJoin::Ambiguous => Err(input("ambiguous-extern-artifact-join", package_id)),
            ArtifactJoin::Missing | ArtifactJoin::Available(_) => {
                Err(input("missing-extern-artifact-join", package_id))
            }
        }
    }

    pub(crate) fn parse(
        bytes: &[u8],
        environment: Box<[(OsString, OsString)]>,
        cargo_commit: &str,
        limits: &cargo::Limits,
    ) -> Result<Self> {
        Self::parse_accounted(
            bytes,
            environment,
            cargo_commit,
            limits,
            Allocation::new(limits),
        )
        .map(|(evidence, _)| evidence)
    }

    /// Parse under the supplied fresh input account and hand it back with the
    /// evidence, so a caller can inspect the account's own exceedance record
    /// instead of the process registry.
    fn parse_accounted(
        bytes: &[u8],
        environment: Box<[(OsString, OsString)]>,
        cargo_commit: &str,
        limits: &cargo::Limits,
        allocation: Allocation,
    ) -> Result<(Self, Allocation)> {
        if cargo_commit != toolchain::supported_cargo_commit() {
            return Err(input("cargo-commit-skew", cargo_commit));
        }
        if !bytes.is_empty() && !bytes.ends_with(b"\n") {
            return Err(input(
                "truncated-build-messages",
                "Cargo build JSON does not end at a line boundary",
            ));
        }
        let mut allocation = allocation;
        // The document size is an accounting target recorded on the
        // input account.
        allocation.exceeded("Cargo build JSON bytes", limits.json_bytes, bytes.len());
        allocation.bytes(bytes.len())?;
        allocation.items(environment.len())?;
        for (name, value) in &environment {
            allocation.items(2)?;
            allocation.bytes(name.as_os_str().len())?;
            allocation.bytes(value.as_os_str().len())?;
        }
        let mut packages = BTreeMap::<Box<str>, Package>::new();
        let mut scripts = Vec::<Script>::new();
        let mut artifacts = Vec::new();
        let mut finished = None;
        let mut aggregate_items = 0_usize;
        for (index, line) in bytes
            .split(|byte| *byte == b'\n')
            .filter(|line| !line.is_empty())
            .enumerate()
        {
            allocation.exceeded(
                "Cargo build JSON lines",
                limits.items,
                index.saturating_add(1),
            );
            allocation.exceeded(
                "Cargo build JSON line bytes",
                limits.string_bytes,
                line.len(),
            );
            let previous_items = aggregate_items;
            aggregate_items = preflight_json(line, aggregate_items, limits, &mut allocation)?;
            allocation.items(aggregate_items.saturating_sub(previous_items))?;
            let value: Value = serde_json::from_slice(line).map_err(|error| Error::Schema {
                document: "Cargo build JSON",
                reason: error.to_string().into_boxed_str(),
            })?;
            let object = value
                .as_object()
                .ok_or_else(|| schema("message is not an object"))?;
            let reason = object
                .get("reason")
                .and_then(Value::as_str)
                .ok_or_else(|| schema("message reason is absent"))?;
            match reason {
                "compiler-artifact" => {
                    let package_id = value_string(object, "package_id")?;
                    let manifest_value = value_string(object, "manifest_path")?;
                    allocation.items(1)?;
                    allocation.string(package_id)?;
                    allocation.path(Path::new(manifest_value))?;
                    let manifest =
                        canonical_file("Cargo build manifest", Path::new(manifest_value))?;
                    allocation.path(&manifest)?;
                    let root_value = manifest
                        .parent()
                        .ok_or_else(|| schema("artifact manifest has no package root"))?;
                    allocation.path(root_value)?;
                    let root = root_value.to_path_buf();
                    let target = object
                        .get("target")
                        .and_then(Value::as_object)
                        .ok_or_else(|| schema("artifact target is absent"))?;
                    let kinds = canonical_strings(
                        target.get("kind"),
                        "artifact target kinds",
                        &mut allocation,
                    )?;
                    let crate_types = canonical_strings(
                        target.get("crate_types"),
                        "artifact crate types",
                        &mut allocation,
                    )?;
                    let source_value = value_string(target, "src_path")?;
                    allocation.path(Path::new(source_value))?;
                    let source = canonical_file("Cargo target source", Path::new(source_value))?;
                    allocation.path(&source)?;
                    let custom_build = kinds.iter().any(|kind| kind.as_ref() == "custom-build");
                    let build_script = if custom_build {
                        allocation.path(&source)?;
                        Some(source.clone())
                    } else {
                        None
                    };
                    let crate_name = value_string(target, "name")?;
                    allocation.string(crate_name)?;
                    let features = canonical_strings(
                        object.get("features"),
                        "artifact features",
                        &mut allocation,
                    )?;
                    let profile_value = object
                        .get("profile")
                        .ok_or_else(|| schema("artifact profile is absent"))?;
                    charge_serialized_value(profile_value, &mut allocation)?;
                    let profile = cargo::unit::ArtifactProfile::from_build_measured(
                        profile_value,
                        limits,
                        &mut allocation,
                    )?;
                    let fresh = object
                        .get("fresh")
                        .and_then(Value::as_bool)
                        .ok_or_else(|| schema("artifact fresh status is absent"))?;
                    let key = Key {
                        package_id: package_id.into(),
                        target: source,
                        target_kinds: kinds,
                        crate_types,
                        crate_name: crate_name.into(),
                        features,
                    };
                    match packages.get_mut(package_id) {
                        Some(package) if package.manifest == manifest => {
                            if let Some(build_script) = build_script {
                                match &package.build_script {
                                    Some(existing) if existing != &build_script => {
                                        return Err(input(
                                            "build-script-path-mismatch",
                                            package_id,
                                        ));
                                    }
                                    Some(_) => {}
                                    None => package.build_script = Some(build_script),
                                }
                            }
                        }
                        Some(_) => return Err(input("package-manifest-mismatch", package_id)),
                        None => {
                            allocation.exceeded(
                                "Cargo build packages",
                                limits.packages,
                                packages.len().saturating_add(1),
                            );
                            allocation.items(1)?;
                            allocation.string(package_id)?;
                            packages.insert(
                                package_id.into(),
                                Package {
                                    manifest,
                                    root,
                                    build_script,
                                },
                            );
                        }
                    }
                    let output_values = strings(
                        object.get("filenames"),
                        "artifact filenames",
                        &mut allocation,
                    )?;
                    allocation.items(output_values.len())?;
                    let mut outputs = Vec::new();
                    outputs
                        .try_reserve_exact(output_values.len().saturating_add(1))
                        .map_err(|source| Error::Allocation {
                            operation: "artifact outputs",
                            source,
                        })?;
                    for path in output_values {
                        let path = canonical_path("Cargo built output", Path::new(path.as_ref()))?;
                        allocation.path(&path)?;
                        outputs.push(path);
                    }
                    if let Some(executable) = object.get("executable").and_then(Value::as_str) {
                        let executable =
                            canonical_path("Cargo built executable", Path::new(executable))?;
                        if !outputs.contains(&executable) {
                            allocation.path(&executable)?;
                            outputs.push(executable);
                        }
                    }
                    outputs.sort_unstable();
                    outputs.dedup();
                    allocation.items(outputs.len())?;
                    let mut dep_info = Vec::new();
                    dep_info
                        .try_reserve_exact(outputs.len())
                        .map_err(|source| Error::Allocation {
                            operation: "dependency information",
                            source,
                        })?;
                    for output in &outputs {
                        let candidate = if custom_build {
                            dependency::build_script_path(cargo_commit, output)?
                        } else {
                            dependency::rustc_path(cargo_commit, output)?
                        };
                        if let Some(path) = candidate.filter(|path| path.is_file()) {
                            allocation.path(&path)?;
                            dep_info.push(path);
                        }
                    }
                    dep_info.sort_unstable();
                    dep_info.dedup();
                    if dep_info.len() > 1 {
                        return Err(input("ambiguous-dep-info", package_id));
                    }
                    let dep_info = dep_info
                        .pop()
                        .ok_or_else(|| input("missing-dep-info", package_id))?;
                    allocation.exceeded(
                        "Cargo build artifacts",
                        limits.units,
                        artifacts.len().saturating_add(1),
                    );
                    allocation.items(1)?;
                    allocation.string(package_id)?;
                    artifacts
                        .try_reserve(1)
                        .map_err(|source| Error::Allocation {
                            operation: "build artifacts",
                            source,
                        })?;
                    artifacts.push(Artifact {
                        package_id: package_id.into(),
                        outputs: outputs.into_boxed_slice(),
                        dep_info,
                        key,
                        profile,
                        fresh,
                    });
                }
                "build-script-executed" => {
                    let package_id_value = value_string(object, "package_id")?;
                    let out_dir_value = value_string(object, "out_dir")?;
                    allocation.items(1)?;
                    allocation.string(package_id_value)?;
                    allocation.path(Path::new(out_dir_value))?;
                    let package_id: Box<str> = package_id_value.into();
                    let out_dir = declaration::out_directory(Path::new(out_dir_value))?;
                    allocation.path(&out_dir)?;
                    let mut cfg = strings(object.get("cfgs"), "build-script cfg", &mut allocation)?
                        .into_vec();
                    cfg.sort_unstable();
                    if cfg.windows(2).any(|pair| pair[0] == pair[1]) {
                        return Err(input("duplicate-build-script-cfg", package_id));
                    }
                    let environment = pairs(object.get("env"), &mut allocation)?;
                    allocation.exceeded(
                        "Cargo build-script messages",
                        limits.units,
                        scripts.len().saturating_add(1),
                    );
                    scripts.try_reserve(1).map_err(|source| Error::Allocation {
                        operation: "build scripts",
                        source,
                    })?;
                    allocation.bytes(std::mem::size_of::<Option<(usize, bool)>>())?;
                    scripts.push(Script {
                        compiler: None,
                        package_id,
                        out_dir,
                        cfg: cfg.into_boxed_slice(),
                        environment,
                    });
                }
                "compiler-message" => {}
                "build-finished" => {
                    let success = object
                        .get("success")
                        .and_then(Value::as_bool)
                        .ok_or_else(|| schema("build-finished success is absent"))?;
                    if finished.replace(success).is_some() {
                        return Err(input("duplicate-build-finished", "Cargo build JSON"));
                    }
                }
                other => return Err(input("unknown-build-message", other)),
            }
        }
        if finished != Some(true) {
            return Err(input(
                "incomplete-build-messages",
                "successful build-finished is absent",
            ));
        }
        for script in &scripts {
            if !packages.contains_key(&script.package_id) {
                return Err(input(
                    "unresolved-build-script-package",
                    script.package_id.as_ref(),
                ));
            }
        }
        let mut out_directories = BTreeSet::new();
        for script in &scripts {
            if !out_directories.insert(&script.out_dir) {
                return Err(input(
                    "ambiguous-build-script-output",
                    script.out_dir.display(),
                ));
            }
        }
        scripts.sort_unstable_by(|left, right| {
            left.package_id
                .cmp(&right.package_id)
                .then_with(|| left.out_dir.cmp(&right.out_dir))
        });
        if scripts.windows(2).any(|pair| {
            pair[0].package_id == pair[1].package_id && pair[0].out_dir == pair[1].out_dir
        }) {
            return Err(input(
                "duplicate-build-script-execution",
                "PackageId and OUT_DIR must identify one execution",
            ));
        }
        artifacts.sort_unstable_by(|left, right| {
            left.key
                .cmp(&right.key)
                .then_with(|| left.profile.cmp(&right.profile))
                .then_with(|| left.outputs.cmp(&right.outputs))
                .then_with(|| left.dep_info.cmp(&right.dep_info))
                .then_with(|| left.fresh.cmp(&right.fresh))
        });
        artifacts.dedup_by(|left, right| left.same_public_message(right));
        let evidence = Self {
            usage: allocation.usage(),
            packages: build::packages::Table::from_cold(packages),
            scripts: scripts.into_boxed_slice(),
            artifacts: artifacts.into_boxed_slice(),
            environment,
        };
        Ok((evidence, allocation))
    }

    pub(crate) fn fallback_packages(
        &mut self,
        cargo_commit: &str,
        limits: &cargo::Limits,
    ) -> Result<Vec<(Box<str>, PathBuf)>> {
        let mut allocation = Allocation::from_usage(self.usage, limits);
        let mut packages = Vec::<(Box<str>, PathBuf)>::new();
        for script in &self.scripts {
            let declarations = declaration::Declarations::read_accounted(
                cargo_commit,
                &script.package_id,
                &script.out_dir,
                limits,
                &mut allocation,
            )?;
            if declarations.paths.is_empty() && declarations.environment.is_empty() {
                let package = self.packages.get(&script.package_id).ok_or_else(|| {
                    input(
                        "unresolved-build-script-package",
                        script.package_id.as_ref(),
                    )
                })?;
                if !packages
                    .iter()
                    .any(|(package_id, _)| package_id == &script.package_id)
                {
                    allocation.exceeded(
                        "default package file sets",
                        limits.packages,
                        packages.len().saturating_add(1),
                    );
                    allocation.items(1)?;
                    allocation.string(&script.package_id)?;
                    allocation.path(&package.manifest)?;
                    packages
                        .try_reserve(1)
                        .map_err(|source| Error::Allocation {
                            operation: "fallback packages",
                            source,
                        })?;
                    packages.push((script.package_id.clone(), package.manifest.clone()));
                }
            }
        }
        self.usage = allocation.usage();
        Ok(packages)
    }
}

impl Build {
    #[cfg(feature = "rustc-driver")]
    pub(crate) fn selected_artifacts(
        &self,
        graph: &cargo::unit::BuildGraph,
        limits: &cargo::Limits,
        allocation: &mut crate::extract::Allocation,
    ) -> Result<Box<[SelectedArtifact]>> {
        self.evidence.selected_artifacts(graph, limits, allocation)
    }

    #[cfg(feature = "rustc-driver")]
    pub(crate) fn probe_directories(
        &self,
        graph: &cargo::unit::BuildGraph,
        limits: &cargo::Limits,
        allocation: &mut crate::extract::Allocation,
    ) -> Result<Box<[PathBuf]>> {
        self.evidence.probe_directories(graph, limits, allocation)
    }

    #[cfg(feature = "rustc-driver")]
    pub(crate) fn authority_for_artifact(
        &self,
        graph: &cargo::unit::BuildGraph,
        artifact: &Path,
        allocation: &mut crate::extract::Allocation,
    ) -> Result<ArtifactJoin> {
        self.evidence
            .authority_for_artifact(graph, artifact, allocation)
    }

    #[cfg(all(feature = "rustc-driver", test))]
    pub(crate) fn validate_extern_artifact(
        &self,
        graph: &cargo::unit::BuildGraph,
        semantic_key: &str,
        package_id: &str,
        target: &Path,
        target_kinds: &[Box<str>],
        crate_types: &[Box<str>],
        crate_name: &str,
        features: &[Box<str>],
        artifact: &Path,
    ) -> Result<()> {
        self.evidence.validate_extern_artifact(
            graph,
            semantic_key,
            package_id,
            target,
            target_kinds,
            crate_types,
            crate_name,
            features,
            artifact,
        )
    }

    pub(crate) fn parse(
        bytes: &[u8],
        environment: Box<[(OsString, OsString)]>,
        cargo_commit: &str,
        limits: &cargo::Limits,
    ) -> Result<Self> {
        Ok(Self {
            evidence: Evidence::parse(bytes, environment, cargo_commit, limits)?,
            package_files: BTreeMap::new(),
        })
    }

    pub(crate) fn fallback_packages(
        &mut self,
        cargo_commit: &str,
        limits: &cargo::Limits,
    ) -> Result<Vec<(Box<str>, PathBuf)>> {
        self.evidence.fallback_packages(cargo_commit, limits)
    }

    pub(crate) const fn usage(&self) -> Usage {
        self.evidence.usage
    }

    pub(crate) fn bind_package_files(
        &mut self,
        package_id: &str,
        files: package::Set,
        limits: &cargo::Limits,
    ) -> Result<()> {
        if self.package_files.contains_key(package_id) {
            return Err(input("duplicate-package-file-set", package_id));
        }
        if files.base_usage() != self.evidence.usage {
            return Err(input("package-file-allocation-lineage", package_id));
        }
        let mut allocation = Allocation::from_usage(files.usage(), limits);
        allocation.items(1)?;
        allocation.string(package_id)?;
        self.package_files.insert(package_id.into(), files);
        self.evidence.usage = allocation.usage();
        Ok(())
    }

    pub(crate) fn correlate(
        mut self,
        graph: &cargo::unit::BuildGraph,
        limits: &cargo::Limits,
        work: &crate::node::GraphBudget,
        command_build: &Path,
        target: &str,
    ) -> Result<Complete> {
        let mut allocation = Allocation::from_usage(self.evidence.usage, limits);
        correlate_artifacts(
            graph.artifacts(),
            &self.evidence.artifacts,
            limits,
            &mut allocation,
        )?;
        correlate_scripts(
            graph.scripts(),
            &self.evidence.scripts,
            limits,
            &mut allocation,
        )?;
        build::scripts::bind(
            graph,
            &mut self.evidence.scripts,
            &self.evidence.artifacts,
            command_build,
            target,
            &mut stored::Budget::borrow(limits, &mut allocation, work),
        )
        .map_err(|error| input("build-script-compiler-association", error))?;
        self.evidence.usage = allocation.usage();
        Ok(Complete { build: self })
    }
}

#[cfg(feature = "rustc-driver")]
fn clone_strings(
    values: &[Box<str>],
    allocation: &mut crate::extract::Allocation,
) -> Result<Box<[Box<str>]>> {
    let mut cloned = Vec::new();
    allocation.reserve(&mut cloned, values.len())?;
    for value in values {
        cloned.push(allocation.owned(value)?);
    }
    Ok(cloned.into_boxed_slice())
}

#[cfg(feature = "rustc-driver")]
fn artifact_authority(
    unit: &cargo::unit::Output,
    package: &Artifact,
    allocation: &mut crate::extract::Allocation,
) -> Result<ArtifactAuthority> {
    allocation.items(1)?;
    Ok(ArtifactAuthority {
        semantic_key: allocation.owned(unit.semantic_key())?,
        package_id: allocation.owned(&package.key.package_id)?,
        target: allocation.clone_path(&package.key.target)?,
        target_kinds: clone_strings(&package.key.target_kinds, allocation)?,
        crate_types: clone_strings(&package.key.crate_types, allocation)?,
        crate_name: allocation.owned(&package.key.crate_name)?,
        features: clone_strings(&package.key.features, allocation)?,
    })
}

fn correlate_artifacts(
    expected: &[cargo::unit::Output],
    artifacts: &[Artifact],
    limits: &cargo::Limits,
    allocation: &mut Allocation,
) -> Result<()> {
    // The unit counts are accounting targets recorded on the input
    // account; only the bijection below refuses.
    allocation.exceeded(
        "Build compiler artifact units",
        limits.units,
        expected.len(),
    );
    allocation.exceeded("Cargo compiler artifacts", limits.units, artifacts.len());
    // Cargo's public message profile omits the full unit context. A package profile override can
    // therefore produce multiple distinct artifacts with the same public projection. Prove total
    // coverage here; selected artifact authority is joined later by its path to a full PackageId,
    // profile, and reachable semantic unit, where zero or residual ambiguity rejects.
    for output in expected {
        if !artifacts
            .iter()
            .any(|artifact| compare_artifact(output, artifact) == Ordering::Equal)
        {
            return Err(input("missing-build-artifact", output.package_id()));
        }
    }
    for artifact in artifacts {
        if !expected
            .iter()
            .any(|output| compare_artifact(output, artifact) == Ordering::Equal)
        {
            return Err(input(
                "unexpected-build-artifact",
                artifact.key.package_id.as_ref(),
            ));
        }
    }
    Ok(())
}

fn correlate_scripts(
    expected: &[cargo::unit::BuildScript],
    messages: &[Script],
    limits: &cargo::Limits,
    allocation: &mut Allocation,
) -> Result<()> {
    // The script counts are accounting targets recorded on the input
    // account that owns the messages. The stored replay route reaches
    // `correlate_scripts_with` directly; its graph and message counts were
    // recorded by the accounts that parsed them.
    allocation.exceeded("normal Build script units", limits.units, expected.len());
    allocation.exceeded(
        "Cargo build-script executions",
        limits.units,
        messages.len(),
    );
    correlate_scripts_with(
        expected,
        messages,
        limits,
        |code, reason| input(code, reason),
        |source| Error::Allocation {
            operation: "build script correlation",
            source,
        },
    )
}

/// Prove the script bijection. `_limits` is retained for the stored replay
/// caller; the counts it once gated are accounting targets measured by the
/// owning accounts, and the only refusals here are correctness and
/// genuine allocation failure.
fn correlate_scripts_with<E: From<Error>>(
    expected: &[cargo::unit::BuildScript],
    messages: &[Script],
    _limits: &cargo::Limits,
    error: impl Fn(&'static str, &str) -> E,
    allocation_error: impl Fn(std::collections::TryReserveError) -> E,
) -> std::result::Result<(), E> {
    let mut expected_ids = Vec::new();
    expected_ids
        .try_reserve_exact(expected.len())
        .map_err(&allocation_error)?;
    for script in expected {
        expected_ids.push(script.package_id());
    }
    let mut actual_ids = Vec::new();
    actual_ids
        .try_reserve_exact(messages.len())
        .map_err(&allocation_error)?;
    for script in messages {
        actual_ids.push(script.package_id.as_ref());
    }
    expected_ids.sort_unstable();
    actual_ids.sort_unstable();
    if expected_ids != actual_ids {
        return Err(error(
            "build-script-unit-cardinality",
            "unit-graph and build-script-executed PackageId multisets differ",
        ));
    }
    Ok(())
}

type PublicArtifactKey<'a> = (
    &'a str,
    &'a Path,
    &'a [Box<str>],
    &'a [Box<str>],
    &'a str,
    &'a [Box<str>],
    &'a cargo::unit::ArtifactProfile,
);

fn public_output_key(output: &cargo::unit::Output) -> PublicArtifactKey<'_> {
    (
        output.package_id(),
        output.target(),
        output.target_kinds(),
        output.crate_types(),
        output.crate_name(),
        output.features(),
        output.artifact_profile(),
    )
}

fn compare_artifact(output: &cargo::unit::Output, artifact: &Artifact) -> Ordering {
    let key = &artifact.key;
    public_output_key(output).cmp(&(
        key.package_id.as_ref(),
        key.target.as_path(),
        key.target_kinds.as_ref(),
        key.crate_types.as_ref(),
        key.crate_name.as_ref(),
        key.features.as_ref(),
        &artifact.profile,
    ))
}

impl Baseline {
    #[cfg(test)]
    pub(crate) fn fixture_universe(build: &[u8]) -> Self {
        Self {
            package_replay: None,
            proof: proof::Proof::fixture(),
            recipes: Box::default(),
            recipe_allocation: Usage { items: 0, bytes: 0 },
            allocation: Allocation::for_proof(
                Usage { items: 0, bytes: 0 },
                &cargo::Limits::standard(),
            ),
            universe_build: Some(cargo::universe::BuildProjection::from_canonical(
                build.into(),
            )),
        }
    }

    #[allow(
        clippy::too_many_arguments,
        reason = "all resolution authorities are exact inputs"
    )]
    pub(crate) fn capture_retaining(
        complete: Complete,
        authority: &[cargo::Input],
        workspace: workspace::Image,
        probe_artifacts: &[(PathBuf, [u8; 32])],
        config: Config,
        harness: [PathBuf; 3],
        proof: &cargo::toolchain::Proof,
        internal: &cargo::internal::Recipe,
        universe: Option<&cargo::universe::MetadataProof>,
        target: &str,
        profile: &str,
        context: &std::sync::Arc<crate::context::Context>,
        graph: &cargo::unit::BuildGraph,
        command_build: &Path,
        limits: &cargo::Limits,
    ) -> Result<(Self, build::Retained)> {
        let mut build = complete.build;
        if proof.cargo_commit() != toolchain::supported_cargo_commit() {
            return Err(input("cargo-commit-skew", proof.cargo_commit()));
        }
        let mut allocation = Allocation::for_proof(build.evidence.usage, limits);
        let result = canonical::pass(|| {
            let mut native = native::Projection::bind(
                &build.evidence,
                harness.each_ref().map(PathBuf::as_path),
                graph,
                command_build,
                target,
                &mut stored::Budget::borrow(limits, &mut allocation, context.graph_budget()),
            )
            .map_err(native::capture_error)?;
            let mut recipes = Vec::new();
            macro_rules! recipe {
                ($value:expr) => {
                    recipe!($value, allocation)
                };
                ($value:expr, $account:ident) => {{
                    $account.exceeded(
                        "input proof recipes",
                        limits.items,
                        recipes.len().saturating_add(1),
                    );
                    $account.items(1)?;
                    recipes.try_reserve(1).map_err(|source| Error::Allocation {
                        operation: "input recipes",
                        source,
                    })?;
                    recipes.push($value);
                }};
            }
            scalar::config(
                &config.projection,
                &config.provenance,
                limits,
                &mut allocation,
                |value, allocation| {
                    recipe!(value.retain(allocation)?, allocation);
                    Ok(())
                },
            )?;
            scalar::internal(internal, &mut allocation, |value, allocation| {
                recipe!(value.retain(allocation)?, allocation);
                Ok(())
            })?;
            for retained in authority {
                // Compiler artifact reads bind to the physical capture bound,
                // never to the configurable JSON target.
                let actual = optional_file_digest_accounted(
                    retained.path(),
                    crate::process::CAPTURE_BYTES,
                    &mut allocation,
                )?;
                if actual.as_ref() != retained.digest() {
                    return Err(Error::ResolutionDrift {
                        command: "input publication",
                        field: "Cargo authority input",
                    });
                }
                recipe!(if retained.digest().is_some() {
                    Recipe::file(Role::Authority, retained.path(), &mut allocation)?
                } else {
                    Recipe::path(Role::Authority, retained.path(), &mut allocation)?
                });
            }
            workspace.require(&mut workspace::Account::borrow(
                &mut allocation,
                context.graph_budget(),
                limits,
            ))?;
            let workspace_expected = workspace.entry;
            allocation.items(workspace.roots.len())?;
            allocation.bytes(
                workspace
                    .roots
                    .len()
                    .saturating_mul(std::mem::size_of::<PathBuf>()),
            )?;
            for path in &workspace.roots {
                allocation.path(path)?;
            }
            recipe!(Recipe::Workspace {
                roots: workspace.roots,
                graph: context.graph_budget().clone(),
            });
            for (path, expected) in probe_artifacts {
                let actual =
                    file_digest_accounted(path, crate::process::CAPTURE_BYTES, &mut allocation)?;
                if &actual != expected {
                    return Err(Error::ResolutionDrift {
                        command: "input publication",
                        field: "metadata probe artifact",
                    });
                }
                recipe!(Recipe::file(Role::BuiltOutput, path, &mut allocation)?);
            }
            for (role, path) in [Role::Harness, Role::Harness, Role::Harness]
                .into_iter()
                .zip(&harness)
            {
                recipe!(Recipe::file(role, path, &mut allocation)?);
            }
            for (path, expected) in config.candidates {
                // A Cargo config file is a manifest-class read bound to the
                // physical tail capture, never to the manifest target.
                let current = optional_file_digest_accounted(
                    &path,
                    crate::process::TAIL_BYTES,
                    &mut allocation,
                )?;
                if current != expected {
                    return Err(Error::ResolutionDrift {
                        command: "input publication",
                        field: "Cargo config provenance",
                    });
                }
                recipe!(Recipe::path(Role::Config, &path, &mut allocation)?);
            }
            scalar::toolchain(proof, &mut allocation, |value, allocation| {
                recipe!(value.retain(allocation)?, allocation);
                Ok(())
            })?;
            scalar::selection(target, profile, &mut allocation, |value, allocation| {
                recipe!(value.retain(allocation)?, allocation);
                Ok(())
            })?;
            for (package_id, package) in &build.evidence.packages {
                recipe!(Recipe::package_file(
                    Role::PackageManifest,
                    package_id,
                    &package.manifest,
                    &mut allocation,
                )?);
                if let Some(build_script) = &package.build_script {
                    recipe!(Recipe::package_file(
                        Role::BuildScript,
                        package_id,
                        build_script,
                        &mut allocation,
                    )?);
                }
                scalar::package(
                    package_id,
                    &package.root,
                    &mut allocation,
                    |value, allocation| {
                        recipe!(value.retain(allocation)?, allocation);
                        Ok(())
                    },
                )?;
            }
            for (index, artifact) in build.evidence.artifacts.iter().enumerate() {
                let package = build
                    .evidence
                    .packages
                    .get(&artifact.package_id)
                    .ok_or_else(|| {
                        input("unresolved-artifact-package", artifact.package_id.as_ref())
                    })?;
                let matching = build
                    .evidence
                    .scripts
                    .iter()
                    .filter(|script| script.package_id.as_ref() == artifact.package_id.as_ref())
                    .count();
                allocation.items(matching)?;
                let mut out_directories = Vec::new();
                out_directories
                    .try_reserve_exact(matching)
                    .map_err(|source| Error::Allocation {
                        operation: "input recipe output directories",
                        source,
                    })?;
                for script in build
                    .evidence
                    .scripts
                    .iter()
                    .filter(|script| script.package_id.as_ref() == artifact.package_id.as_ref())
                {
                    allocation.path(&script.out_dir)?;
                    out_directories.push(script.out_dir.clone());
                }
                recipe!(Recipe::dep_info(
                    &artifact.package_id,
                    &artifact.dep_info,
                    &package.root,
                    &out_directories,
                    &mut allocation,
                )?);
                let dependency = dependency::Dependency::read(
                    proof.cargo_commit(),
                    &artifact.dep_info,
                    &package.root,
                    limits,
                    &mut allocation,
                )?;
                native
                    .observe(
                        index,
                        &dependency,
                        &mut stored::Budget::borrow(
                            limits,
                            &mut allocation,
                            context.graph_budget(),
                        ),
                    )
                    .map_err(native::capture_error)?;
                artifact::derived(
                    &dependency,
                    &artifact.package_id,
                    &artifact.dep_info,
                    &mut allocation,
                    |value, allocation| {
                        recipe!(value.retain(allocation)?, allocation);
                        Ok(())
                    },
                )?;
            }
            let mut fallback_bound = BTreeSet::<&str>::new();
            for script in &build.evidence.scripts {
                let package_id = &script.package_id;
                let package = build
                    .evidence
                    .packages
                    .get(package_id)
                    .ok_or_else(|| input("unresolved-build-script-package", package_id))?;
                let declarations = declaration::Declarations::read_accounted(
                    proof.cargo_commit(),
                    package_id,
                    &script.out_dir,
                    limits,
                    &mut allocation,
                )?;
                let execution = script::declarations(
                    script,
                    package,
                    &declarations,
                    config.cargo_env_overrides,
                    &build.evidence.environment,
                    &mut allocation,
                    |value, allocation| {
                        recipe!(value.retain(allocation)?, allocation);
                        Ok(())
                    },
                )?;
                if script::fallback(&declarations) && !fallback_bound.contains(package_id.as_ref())
                {
                    allocation.exceeded(
                        "default package file sets",
                        limits.packages,
                        fallback_bound.len().saturating_add(1),
                    );
                    fallback_bound.insert(package_id);
                    let files = build
                        .package_files
                        .remove(package_id.as_ref())
                        .ok_or_else(|| input("missing-package-file-set", package_id))?;
                    recipe!(Recipe::Package(files));
                }
                script::outputs(script, execution, &mut allocation, |value, allocation| {
                    recipe!(value.retain(allocation)?, allocation);
                    Ok(())
                })?;
            }
            if !build.package_files.is_empty() {
                return Err(input(
                    "unused-package-file-set",
                    "fallback package proof was not transformed into a recipe",
                ));
            }
            allocation.exceeded("input proof recipes", limits.items, recipes.len());
            let recipe_allocation = allocation.usage();
            let mut proof = proof::evaluate(&recipes, false, limits, &mut allocation)?;
            if !proof.contains_workspace(&workspace_expected) {
                return Err(Error::ResolutionDrift {
                    command: "input publication",
                    field: "workspace input before-image",
                });
            }
            let native = native
                .finish(
                    proof.stored_parts().1,
                    &mut stored::Budget::borrow(limits, &mut allocation, context.graph_budget()),
                )
                .map_err(native::capture_error)?;
            proof.bind_native(native);
            let package_replay = recipes
                .iter()
                .any(|recipe| {
                    matches!(
                        recipe,
                        Recipe::Package(package::Set::Path(_) | package::Set::Git(_))
                    )
                })
                .then(|| {
                    cargo::execution::Scope::bind(
                        context,
                        cargo::execution::Operation::PackageReplay,
                        *proof.fingerprint().as_bytes(),
                    )
                });
            let universe_build = universe
                .map(|protected| {
                    proof::protected_digest_accounted(
                        &proof,
                        &build.evidence,
                        protected,
                        target,
                        limits,
                        &mut allocation,
                        context.graph_budget(),
                    )
                })
                .transpose()?;
            Ok((
                proof,
                recipes,
                recipe_allocation,
                universe_build,
                package_replay,
            ))
        });
        #[cfg(feature = "import-baseline")]
        allocation.observe_capture(&result);
        let (proof, recipes, recipe_allocation, universe_build, package_replay) = result?;
        Ok((
            Self {
                proof,
                recipes: recipes.into_boxed_slice(),
                recipe_allocation,
                allocation,
                universe_build,
                package_replay,
            },
            build::Retained { build },
        ))
    }

    pub(crate) const fn fingerprint(&self) -> fingerprint::Fingerprint {
        self.proof.fingerprint()
    }

    pub(crate) const fn universe_build(&self) -> Option<&cargo::universe::BuildProjection> {
        self.universe_build.as_ref()
    }

    pub(crate) fn prove(self, limits: &cargo::Limits) -> Result<proof::Proof> {
        let mut allocation = Allocation::for_proof(self.recipe_allocation, limits);
        let mut evaluate = || proof::evaluate(&self.recipes, true, limits, &mut allocation);
        let mut current = match self.package_replay {
            Some(scope) => scope.run(evaluate)?,
            None => evaluate()?,
        };
        if current.fingerprint() != self.proof.fingerprint() {
            return Err(Error::Input {
                code: "input-changed",
                reason: "content changed after authoritative Build capture".into(),
            });
        }
        // The same exact evidence was re-observed; preserve its already-derived
        // native association without retaining Build or rereading dependencies.
        current.bind_native(*self.proof.native()?);
        Ok(current)
    }
}

fn declared_path(root: &Path, value: &str) -> Result<PathBuf> {
    bounded_path_text(value)?;
    let path = Path::new(value);
    if path.is_absolute() {
        normalized_absolute(path)
    } else {
        normalized_absolute(&root.join(path))
    }
}

#[allow(
    clippy::type_complexity,
    reason = "Cargo build-script environment is a closed ordered key/value wire tuple"
)]
fn pairs(
    value: Option<&Value>,
    allocation: &mut Allocation,
) -> Result<Box<[(Box<str>, Box<str>)]>> {
    let values = value
        .and_then(Value::as_array)
        .ok_or_else(|| schema("build-script environment is absent"))?;
    // Entry and string sizes are accounting targets recorded on the
    // input account; the charges below are the checked admission.
    allocation.items_exceeded("build-script environment entries", values.len());
    allocation.items(values.len())?;
    let mut pairs = Vec::new();
    pairs
        .try_reserve_exact(values.len())
        .map_err(|source| Error::Allocation {
            operation: "build script environment",
            source,
        })?;
    for pair in values {
        let pair = pair
            .as_array()
            .filter(|pair| pair.len() == 2)
            .ok_or_else(|| schema("build-script environment entry is not a pair"))?;
        let key = pair[0]
            .as_str()
            .ok_or_else(|| schema("build-script environment key is not a string"))?;
        let value = pair[1]
            .as_str()
            .ok_or_else(|| schema("build-script environment value is not a string"))?;
        allocation.string_exceeded("build-script environment", key.len());
        allocation.string_exceeded("build-script environment", value.len());
        allocation.string(key)?;
        allocation.string(value)?;
        pairs.push((key.into(), value.into()));
    }
    pairs.sort_unstable();
    if pairs.windows(2).any(|pair| pair[0].0 == pair[1].0) {
        return Err(input(
            "duplicate-build-script-environment",
            "Cargo build JSON",
        ));
    }
    Ok(pairs.into_boxed_slice())
}

fn strings(
    value: Option<&Value>,
    document: &'static str,
    allocation: &mut Allocation,
) -> Result<Box<[Box<str>]>> {
    let values = value
        .and_then(Value::as_array)
        .ok_or_else(|| schema(&format!("{document} are absent")))?;
    // The entry count and each string size are accounting targets
    // recorded on the input account; the charges below are the admission.
    allocation.items_exceeded(document, values.len());
    allocation.items(values.len())?;
    let mut owned = Vec::new();
    owned
        .try_reserve_exact(values.len())
        .map_err(|source| Error::Allocation {
            operation: "build strings",
            source,
        })?;
    for value in values {
        let value = value
            .as_str()
            .ok_or_else(|| schema(&format!("{document} entry is not a string")))?;
        allocation.string_exceeded(document, value.len());
        allocation.string(value)?;
        owned.push(value.into());
    }
    Ok(owned.into_boxed_slice())
}

fn canonical_strings(
    value: Option<&Value>,
    document: &'static str,
    allocation: &mut Allocation,
) -> Result<Box<[Box<str>]>> {
    let mut values = strings(value, document, allocation)?.into_vec();
    values.sort_unstable();
    if values.windows(2).any(|pair| pair[0] == pair[1]) {
        return Err(input("duplicate-build-message-value", document));
    }
    Ok(values.into_boxed_slice())
}

fn charge_serialized_value(value: &Value, allocation: &mut Allocation) -> Result<()> {
    struct Counter(usize);
    impl std::io::Write for Counter {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.0 = self
                .0
                .checked_add(bytes.len())
                .ok_or_else(|| std::io::Error::other("Build JSON value size overflow"))?;
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let mut counter = Counter(0);
    serde_json::to_writer(&mut counter, value).map_err(|error| Error::Schema {
        document: "Cargo build JSON",
        reason: error.to_string().into_boxed_str(),
    })?;
    allocation.items(1)?;
    allocation.bytes(counter.0)
}

fn value_string<'a>(object: &'a serde_json::Map<String, Value>, key: &str) -> Result<&'a str> {
    object
        .get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| schema(&format!("{key} is absent or not a string")))
}

/// Scan one Cargo build JSON line before serde owns it. Containers, items
/// and string bytes are accounting targets recorded on `allocation`;
/// `depth` bounds the container stack and stays a physical refusal.
fn preflight_json(
    bytes: &[u8],
    mut items: usize,
    limits: &cargo::Limits,
    allocation: &mut Allocation,
) -> Result<usize> {
    let mut in_string = false;
    let mut escaped = false;
    let mut string_bytes = 0_usize;
    let mut containers = 0_usize;
    let mut stack = Vec::<JsonContainer>::new();
    for byte in bytes {
        if in_string {
            if escaped {
                escaped = false;
            } else if *byte == b'\\' {
                escaped = true;
            } else if *byte == b'"' {
                allocation.exceeded(
                    "Cargo build JSON string bytes",
                    limits.string_bytes,
                    string_bytes,
                );
                string_bytes = 0;
                in_string = false;
            } else {
                string_bytes = string_bytes.saturating_add(1);
            }
            continue;
        }
        if *byte == b'"' {
            begin_array_value(&mut stack, &mut items, limits, allocation)?;
            in_string = true;
        } else if matches!(*byte, b'[' | b'{') {
            begin_array_value(&mut stack, &mut items, limits, allocation)?;
            containers = containers.saturating_add(1);
            allocation.exceeded("Cargo build JSON containers", limits.containers, containers);
            limited(
                "Cargo build JSON depth",
                limits.depth,
                stack.len().saturating_add(1),
            )?;
            stack.push(if *byte == b'[' {
                JsonContainer::Array {
                    expects_value: true,
                }
            } else {
                JsonContainer::Object { expects_key: true }
            });
        } else if matches!(*byte, b']' | b'}') {
            stack
                .pop()
                .ok_or_else(|| schema("unbalanced JSON container"))?;
        } else if *byte == b',' {
            match stack.last_mut() {
                Some(JsonContainer::Array { expects_value }) => *expects_value = true,
                Some(JsonContainer::Object { expects_key }) => *expects_key = true,
                None => {}
            }
        } else if !byte.is_ascii_whitespace() && *byte != b':' {
            begin_array_value(&mut stack, &mut items, limits, allocation)?;
        }
    }
    if in_string || escaped || !stack.is_empty() {
        return Err(schema("truncated JSON line"));
    }
    Ok(items)
}

fn begin_array_value(
    stack: &mut [JsonContainer],
    items: &mut usize,
    limits: &cargo::Limits,
    allocation: &mut Allocation,
) -> Result<()> {
    let expected = match stack.last_mut() {
        Some(JsonContainer::Array { expects_value }) if *expects_value => {
            *expects_value = false;
            true
        }
        Some(JsonContainer::Object { expects_key }) if *expects_key => {
            *expects_key = false;
            true
        }
        Some(JsonContainer::Array { .. }) | Some(JsonContainer::Object { .. }) | None => false,
    };
    if expected {
        *items = items.saturating_add(1);
        allocation.exceeded("Cargo build JSON items", limits.items, *items);
    }
    Ok(())
}

fn environment_value<'a>(environment: &'a [(OsString, OsString)], key: &str) -> Option<&'a str> {
    environment
        .iter()
        .find_map(|(name, value)| (name == key).then(|| value.to_str()).flatten())
}

fn optional_file_digest_accounted(
    path: &Path,
    limit: usize,
    allocation: &mut Allocation,
) -> Result<Option<[u8; 32]>> {
    match fs::metadata(path) {
        Ok(metadata) if metadata.is_file() => {
            Ok(Some(file_digest_accounted(path, limit, allocation)?))
        }
        Ok(_) => Err(input("config-input-shape", path.display())),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(source) => Err(Error::Io {
            operation: "inspect optional input",
            path: path.to_path_buf(),
            source,
        }),
    }
}

fn file_digest_accounted(
    path: &Path,
    limit: usize,
    allocation: &mut Allocation,
) -> Result<[u8; 32]> {
    use crate::file_digest::{Domain, Identity};
    // An unchanged physical identity has the digest it had when hashed.
    let identity = fs::metadata(path).ok().as_ref().and_then(Identity::of);
    if let Some(digest) = crate::file_digest::get(identity, Domain::Sha256) {
        let length = usize::try_from(identity.map_or(0, Identity::length)).unwrap_or(usize::MAX);
        limited("content input", limit, length)?;
        allocation.items(1)?;
        allocation.bytes(length)?;
        return Ok(digest);
    }
    let digest = Sha256::digest(read_file_budgeted(
        "content input",
        path,
        limit,
        allocation,
    )?)
    .into();
    if fs::metadata(path).ok().as_ref().and_then(Identity::of) == identity {
        crate::file_digest::insert(identity, Domain::Sha256, digest);
    }
    Ok(digest)
}

fn read_file_budgeted(
    resource: &'static str,
    path: &Path,
    limit: usize,
    allocation: &mut Allocation,
) -> Result<Vec<u8>> {
    let metadata = fs::metadata(path).map_err(|source| Error::Io {
        operation: "inspect bounded input",
        path: path.to_path_buf(),
        source,
    })?;
    let length = usize::try_from(metadata.len()).unwrap_or(usize::MAX);
    limited(resource, limit, length)?;
    allocation.items(1)?;
    allocation.bytes(length)?;
    let bytes = read_file(resource, path, length)?;
    #[cfg(test)]
    OWNED_ACCOUNTED_READS.with(|owned| owned.set(owned.get().saturating_add(1)));
    Ok(bytes)
}

fn read_file(resource: &'static str, path: &Path, limit: usize) -> Result<Vec<u8>> {
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
    limited(resource, limit, length)?;
    let mut bytes = Vec::new();
    bytes
        .try_reserve_exact(length)
        .map_err(|source| Error::Allocation {
            operation: "bounded input read",
            source,
        })?;
    bytes.resize(length, 0);
    if let Err(source) = file.read_exact(&mut bytes) {
        if source.kind() == std::io::ErrorKind::UnexpectedEof {
            return Err(input("input-changed-during-read", path.display()));
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
        return Err(input("input-changed-during-read", path.display()));
    }
    Ok(bytes)
}

/// Stream one freshness file of `expected` metadata bytes into SHA-256 through
/// a fixed `DIGEST_CHUNK_BYTES` chunk; no buffer of the file is ever admitted.
/// An opened extent past `expected` is refused as a physical read bound, and a
/// shorter extent or growth during the read is input drift, exactly as the
/// whole-buffer `read_file`.
fn freshness_digest(resource: &'static str, path: &Path, expected: usize) -> Result<[u8; 32]> {
    use crate::file_digest::{self, Domain, Identity};

    // A file whose identity is unchanged since it was hashed is not reopened:
    // its path's metadata names the same identity an open would.
    if let Ok(metadata) = fs::metadata(path) {
        let length = usize::try_from(metadata.len()).unwrap_or(usize::MAX);
        if length == expected
            && let Some(digest) = file_digest::get(Identity::of(&metadata), Domain::Sha256)
        {
            return Ok(digest);
        }
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
    limited(resource, expected, length)?;
    // An unchanged physical identity has the digest it had when hashed.
    let identity = Identity::of(&metadata);
    if length == expected
        && let Some(digest) = file_digest::get(identity, Domain::Sha256)
    {
        return Ok(digest);
    }
    let mut hasher = Sha256::new();
    let mut chunk = [0_u8; DIGEST_CHUNK_BYTES];
    let mut remaining = expected;
    while remaining != 0 {
        let wanted = remaining.min(DIGEST_CHUNK_BYTES);
        match file.read(&mut chunk[..wanted]) {
            Ok(0) => return Err(input("input-changed-during-read", path.display())),
            Ok(count) => {
                hasher.update(&chunk[..count]);
                remaining -= count;
            }
            Err(source) if source.kind() == std::io::ErrorKind::Interrupted => {}
            Err(source) => {
                return Err(Error::Io {
                    operation: "read bounded input",
                    path: path.to_path_buf(),
                    source,
                });
            }
        }
    }
    let mut extra = [0_u8; 1];
    let extra = file.read(&mut extra).map_err(|source| Error::Io {
        operation: "probe bounded input growth",
        path: path.to_path_buf(),
        source,
    })?;
    if extra != 0 {
        return Err(input("input-changed-during-read", path.display()));
    }
    let digest = hasher.finalize().into();
    if length == expected && file.metadata().ok().as_ref().and_then(Identity::of) == identity {
        file_digest::insert(identity, Domain::Sha256, digest);
    }
    Ok(digest)
}

#[cfg(feature = "rustc-driver")]
fn read_file_accounted(
    resource: &'static str,
    path: &Path,
    limit: usize,
    allocation: &mut crate::extract::Allocation,
) -> Result<Vec<u8>> {
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
    limited(resource, limit, length)?;
    allocation.items(1)?;
    allocation.bytes(length)?;
    let mut bytes = Vec::new();
    bytes
        .try_reserve_exact(length)
        .map_err(|source| Error::Allocation {
            operation: "accounted input read",
            source,
        })?;
    bytes.resize(length, 0);
    if let Err(source) = file.read_exact(&mut bytes) {
        if source.kind() == std::io::ErrorKind::UnexpectedEof {
            return Err(input("input-changed-during-read", path.display()));
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
        return Err(input("input-changed-during-read", path.display()));
    }
    Ok(bytes)
}

fn canonical_path(field: &'static str, path: &Path) -> Result<PathBuf> {
    let metadata = fs::metadata(path).map_err(|source| Error::Io {
        operation: "inspect Cargo output",
        path: path.to_path_buf(),
        source,
    })?;
    if metadata.is_file() {
        canonical_file(field, path)
    } else if metadata.is_dir() {
        canonical_directory(field, path)
    } else {
        Err(input("unsupported-output-shape", path.display()))
    }
}

fn canonical_file(field: &'static str, path: &Path) -> Result<PathBuf> {
    let normalized = normalized_absolute(path)?;
    if canonical::regular_file(&normalized) {
        return Ok(normalized);
    }
    let canonical = fs::canonicalize(path).map_err(|source| Error::Io {
        operation: "canonicalize file",
        path: path.to_path_buf(),
        source,
    })?;
    if !canonical.is_file() || canonical != normalized {
        return Err(Error::Path {
            field,
            path: path.to_path_buf(),
            reason: "must be a canonical regular file without symlink traversal".into(),
        });
    }
    Ok(canonical)
}

fn canonical_directory(field: &'static str, path: &Path) -> Result<PathBuf> {
    let normalized = normalized_absolute(path)?;
    let canonical = fs::canonicalize(path).map_err(|source| Error::Io {
        operation: "canonicalize directory",
        path: path.to_path_buf(),
        source,
    })?;
    if !canonical.is_dir() || canonical != normalized {
        return Err(Error::Path {
            field,
            path: path.to_path_buf(),
            reason: "must be a canonical directory without symlink traversal".into(),
        });
    }
    Ok(canonical)
}

fn normalized_absolute(path: &Path) -> Result<PathBuf> {
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
            Component::RootDir | Component::Normal(_) => normalized.push(component.as_os_str()),
            Component::CurDir => {}
            Component::ParentDir => {
                if !normalized.pop() {
                    return Err(input("path-escape", path.display()));
                }
            }
        }
    }
    Ok(normalized)
}

fn reject_symlinks(path: &Path) -> Result<()> {
    let mut current = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Prefix(prefix) => current.push(prefix.as_os_str()),
            Component::RootDir | Component::Normal(_) => current.push(component.as_os_str()),
            Component::CurDir | Component::ParentDir => continue,
        }
        match fs::symlink_metadata(&current) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return Err(input("symlink-input", current.display()));
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => break,
            Err(source) => {
                return Err(Error::Io {
                    operation: "inspect input ancestor",
                    path: current,
                    source,
                });
            }
        }
    }
    Ok(())
}

fn path_text<'a>(field: &'static str, path: &'a Path) -> Result<&'a str> {
    let value = path.to_str().ok_or_else(|| Error::Path {
        field,
        path: path.to_path_buf(),
        reason: "is not valid Unicode".into(),
    })?;
    bounded_path_text(value)?;
    Ok(value)
}

fn bounded_path_text(value: &str) -> Result<()> {
    limited("input path bytes", LINE_BYTES, value.len())
}

/// Retained for `declaration.rs`, which compares a declared string against
/// the string target outside any input account.
/// A physical bound (path text, a file read that outgrew its admitted
/// buffer, JSON depth) refuses. Accounting targets are recorded on their
/// account instead.
fn limited(resource: &'static str, limit: usize, actual: usize) -> Result<()> {
    if actual > limit {
        Err(Error::Limit {
            resource,
            limit,
            actual,
        })
    } else {
        Ok(())
    }
}

fn schema(reason: &str) -> Error {
    Error::Schema {
        document: "Cargo build JSON",
        reason: reason.into(),
    }
}

fn input(reason: &'static str, detail: impl std::fmt::Display) -> Error {
    Error::Input {
        code: reason,
        reason: detail.to_string().into_boxed_str(),
    }
}

#[cfg(test)]
mod build_message_tests {
    #[cfg(feature = "rustc-driver")]
    use std::collections::BTreeMap;
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicU64, Ordering};

    use super::{
        charge_serialized_value, correlate_artifacts, correlate_scripts, dependency, pairs,
        preflight_json, proof, read_file, read_file_budgeted, strings, Allocation, Artifact, Key,
        Script, Usage, INPUT_BYTES_RESOURCE, INPUT_ITEMS_RESOURCE, OWNED_ACCOUNTED_READS,
    };
    #[cfg(feature = "rustc-driver")]
    use super::{Build, Evidence};
    use crate::cargo::{unit, Limits};
    use crate::input::proof::{Recipe, Role};

    static READ_ID: AtomicU64 = AtomicU64::new(1);

    /// The Cargo build JSON document size is an accounting target on the
    /// input account; exact records nothing, one under parses the same
    /// document and the account's own record carries the axis with the
    /// truthful size and no latch.
    #[test]
    fn build_json_bytes_record_the_json_target_without_refusal() {
        let bytes = b"{\"reason\":\"build-finished\",\"success\":true}\n";
        let parse = |json_bytes: usize| {
            let mut limits = Limits::standard();
            limits.json_bytes = json_bytes;
            super::Evidence::parse_accounted(
                bytes,
                Box::default(),
                crate::cargo::toolchain::supported_cargo_commit(),
                &limits,
                Allocation::new(&limits),
            )
            .map(|(_, allocation)| allocation)
        };
        let exact = parse(bytes.len()).expect("a document at the JSON target parses");
        assert!(!exact.exceedances().is_exceeded());
        let mut one_under =
            parse(bytes.len() - 1).expect("a document one byte over the JSON target parses");
        let axis = *one_under
            .exceedances()
            .axis("Cargo build JSON bytes")
            .expect("the input account records the document axis");
        assert_eq!(
            (axis.target, axis.first_requested, axis.high_water),
            (
                (bytes.len() - 1) as u64,
                bytes.len() as u64,
                bytes.len() as u64
            )
        );
        assert!(one_under.items(0).is_ok(), "an exceedance never latches");
    }

    /// Cause ordering: an exceedance never latches, a later genuine
    /// checked-arithmetic overflow does, and the latch then refuses every
    /// further charge while the recorded exceedance and spent usage stand.
    #[test]
    fn checked_overflow_latches_the_input_account_after_an_exceedance() {
        let mut allocation = Allocation::with_limits(usize::MAX, 4, usize::MAX);
        allocation
            .bytes(5)
            .expect("one byte over the target is charged");
        assert_eq!(
            allocation
                .exceedances()
                .axis(INPUT_BYTES_RESOURCE)
                .unwrap()
                .high_water,
            5
        );
        assert!(allocation.items(0).is_ok(), "an exceedance never latches");
        assert!(matches!(
            allocation.bytes(usize::MAX),
            Err(crate::Error::Node {
                operation: INPUT_BYTES_RESOURCE,
                ..
            })
        ));
        assert!(matches!(
            allocation.items(0),
            Err(crate::Error::InputAccountRefused)
        ));
        assert_eq!(allocation.usage(), Usage { items: 0, bytes: 5 });
        assert_eq!(allocation.exceedances().axes().count(), 1);
    }

    /// Crossing the proof byte target is charged, recorded and returns
    /// `Ok`, so the baseline observer sees the usage and no refusal; only a
    /// genuine physical cause (here a real file read against a zero bound)
    /// reports a refusal, with its own figures.
    #[cfg(feature = "import-baseline")]
    #[test]
    fn allocation_observation_keeps_capture_usage_and_reports_only_physical_refusal() {
        use crate::metrics::{accounting::OwnerStage, accounting::Scalar, recorder};

        let mut limits = Limits::standard();
        limits.input_proof_bytes = 3;
        let mut allocation = Allocation::for_proof(Usage { items: 1, bytes: 2 }, &limits);
        allocation.bytes(1).unwrap();
        let path = std::env::temp_dir().join(format!(
            "rawr-input-observe-{}-{}",
            std::process::id(),
            READ_ID.fetch_add(1, Ordering::Relaxed)
        ));
        fs::write(&path, b"x").expect("the one-byte observation fixture is written");
        let physical = read_file("observation fixture", &path, 0);
        fs::remove_file(&path).expect("the observation fixture is retired");
        assert!(matches!(
            physical,
            Err(crate::Error::Limit {
                resource: "observation fixture",
                limit: 0,
                actual: 1,
            })
        ));
        let recorder = recorder::Recorder::transform_at(10, 1).unwrap();
        {
            let _scope = recorder.scope().unwrap();
            allocation.observe_capture(&Ok::<_, crate::Error>(()));
            let result = allocation.bytes(1);
            allocation.observe_capture(&result);
            result.expect("one byte over the proof target is charged, not refused");
            allocation.observe_capture(&physical);
        }
        assert_eq!(allocation.usage(), Usage { items: 1, bytes: 4 });
        let axis = *allocation
            .exceedances()
            .axis(INPUT_BYTES_RESOURCE)
            .expect("the proof account records its byte axis");
        assert_eq!(
            (axis.target, axis.first_requested, axis.high_water),
            (3, 4, 4)
        );
        assert!(allocation.bytes(0).is_ok(), "an exceedance never latches");
        let evidence = recorder.snapshot().unwrap().accounting;
        let current = evidence.scalar(Scalar::OwnerInputBytes).unwrap();
        assert_eq!((current.count, current.min, current.max), (3, 3, 4));
        assert_eq!(evidence.scalar(Scalar::OwnerInputItems).unwrap().max, 1);
        assert_eq!(
            evidence
                .scalar(Scalar::OwnerRefusals(OwnerStage::InputCapture))
                .unwrap()
                .count,
            1
        );
        assert_eq!(
            evidence
                .scalar(Scalar::OwnerRefusedRequired(OwnerStage::InputCapture))
                .unwrap()
                .max,
            1
        );
        assert_eq!(
            evidence
                .scalar(Scalar::OwnerRefusedLimit(OwnerStage::InputCapture))
                .unwrap()
                .max,
            0
        );
    }

    #[test]
    fn input_proof_byte_authority_is_exact_and_isolated_from_shared_input() {
        let limits = Limits::standard();
        assert_eq!(limits.byte_limit(), 33_554_432);
        assert_eq!(limits.canonical_bytes, 1_207_959_552);
        assert_eq!(limits.input_proof_bytes, 536_870_912);

        // The byte target is an accounting target. The exact boundary
        // records nothing; one byte over is still charged, returns `Ok`,
        // records the axis with a truthful high water and latches nothing.
        let mut shared = Allocation::new(&limits);
        shared
            .bytes(33_554_432)
            .expect("the exact shared input boundary is accepted");
        assert!(!shared.exceedances().is_exceeded());
        shared
            .bytes(1)
            .expect("one byte over the shared target is charged, not refused");
        assert_eq!(
            shared.usage(),
            Usage {
                items: 0,
                bytes: 33_554_433
            }
        );
        let shared_axis = *shared
            .exceedances()
            .axis(INPUT_BYTES_RESOURCE)
            .expect("the shared account records its byte axis");
        assert_eq!(
            (
                shared_axis.target,
                shared_axis.first_requested,
                shared_axis.high_water
            ),
            (33_554_432, 33_554_433, 33_554_433)
        );
        assert!(shared.items(0).is_ok(), "an exceedance never latches");

        let mut proof = Allocation::for_proof(Usage { items: 0, bytes: 0 }, &limits);
        proof
            .bytes(536_870_912)
            .expect("the exact input-proof boundary is accepted");
        assert!(!proof.exceedances().is_exceeded());
        proof
            .bytes(1)
            .expect("one byte over the input-proof target is charged, not refused");
        assert_eq!(
            proof.usage(),
            Usage {
                items: 0,
                bytes: 536_870_913
            }
        );
        let proof_axis = *proof
            .exceedances()
            .axis(INPUT_BYTES_RESOURCE)
            .expect("the proof account records its byte axis");
        assert_eq!(
            (
                proof_axis.target,
                proof_axis.first_requested,
                proof_axis.high_water
            ),
            (536_870_912, 536_870_913, 536_870_913)
        );
        assert!(proof.items(0).is_ok(), "an exceedance never latches");
        // The shared account's exceedance is isolated from the proof account.
        assert_eq!(shared.exceedances().axes().count(), 1);
        assert_eq!(proof.exceedances().axes().count(), 1);
    }

    #[test]
    fn removing_an_ordinary_compiler_artifact_rejects_completeness() {
        let limits = Limits::standard();
        let mut allocation = Allocation::new(&limits);
        let output = output("dependency", "lib", "lib");
        let artifact = artifact(&output);
        correlate_artifacts(
            std::slice::from_ref(&output),
            std::slice::from_ref(&artifact),
            &limits,
            &mut allocation,
        )
        .expect("the complete ordinary artifact correlates");
        let error =
            correlate_artifacts(std::slice::from_ref(&output), &[], &limits, &mut allocation)
                .expect_err("removing the compiler-artifact line rejects");
        assert!(format!("{error}").contains("missing-build-artifact"));
    }

    #[test]
    fn harness_and_every_unexpected_artifact_class_are_bijective() {
        let limits = Limits::standard();
        let mut allocation = Allocation::new(&limits);
        let harness = output("rawr-resolution-harness", "lib", "lib");
        let harness_artifact = artifact(&harness);
        correlate_artifacts(
            std::slice::from_ref(&harness),
            std::slice::from_ref(&harness_artifact),
            &limits,
            &mut allocation,
        )
        .expect("the synthetic root harness is explicitly matched");

        for artifact in [
            artifact(&output("unexpected-library", "lib", "lib")),
            artifact(&output("unexpected-proc-macro", "proc-macro", "proc-macro")),
            artifact(&output("unexpected-custom-build", "custom-build", "bin")),
        ] {
            let error = correlate_artifacts(&[], &[artifact], &limits, &mut allocation)
                .expect_err("an unplanned compiler artifact rejects the bijection");
            assert!(format!("{error}").contains("unexpected-build-artifact"));
        }
    }

    #[test]
    fn same_package_build_script_executions_retain_distinct_out_directories() {
        let limits = Limits::standard();
        let mut allocation = Allocation::new(&limits);
        let package_id = "path+file:///fixture#0.1.0";
        let expected = [
            unit::BuildScript::fixture(package_id, "host"),
            unit::BuildScript::fixture(package_id, "target"),
        ];
        let actual = [script(package_id, "host"), script(package_id, "target")];
        assert_ne!(actual[0].out_dir, actual[1].out_dir);
        correlate_scripts(&expected, &actual, &limits, &mut allocation)
            .expect("two same-PackageId executions satisfy the exact package multiset");
        let error = correlate_scripts(&expected, &actual[..1], &limits, &mut allocation)
            .expect_err("omitting one same-PackageId execution rejects");
        assert!(format!("{error}").contains("build-script-unit-cardinality"));
    }

    /// The unit counts correlated against the Cargo graph are accounting
    /// targets on the input account. The exact count records nothing; one
    /// under still proves the bijection, records each crossing axis with the
    /// truthful count and latches nothing.
    #[test]
    fn correlated_unit_counts_record_the_unit_target_without_refusal() {
        let outputs = [
            output("dependency", "lib", "lib"),
            output("other", "lib", "lib"),
        ];
        let artifacts = [artifact(&outputs[0]), artifact(&outputs[1])];
        let correlate = |units: usize| {
            let mut limits = Limits::standard();
            limits.units = units;
            let mut allocation = Allocation::new(&limits);
            correlate_artifacts(&outputs, &artifacts, &limits, &mut allocation)
                .expect("the complete bijection correlates regardless of the unit target");
            allocation
        };
        assert!(!correlate(2).exceedances().is_exceeded());
        let mut one_under = correlate(1);
        for name in ["Build compiler artifact units", "Cargo compiler artifacts"] {
            let axis = *one_under
                .exceedances()
                .axis(name)
                .unwrap_or_else(|| panic!("`{name}` records its axis"));
            assert_eq!(
                (axis.target, axis.first_requested, axis.high_water),
                (1, 2, 2)
            );
        }
        assert!(one_under.items(0).is_ok(), "an exceedance never latches");

        let package_id = "path+file:///fixture#0.1.0";
        let expected = [
            unit::BuildScript::fixture(package_id, "host"),
            unit::BuildScript::fixture(package_id, "target"),
        ];
        let actual = [script(package_id, "host"), script(package_id, "target")];
        let mut limits = Limits::standard();
        limits.units = 1;
        let mut allocation = Allocation::new(&limits);
        correlate_scripts(&expected, &actual, &limits, &mut allocation)
            .expect("the exact script multiset correlates regardless of the unit target");
        for name in ["normal Build script units", "Cargo build-script executions"] {
            let axis = *allocation
                .exceedances()
                .axis(name)
                .unwrap_or_else(|| panic!("`{name}` records its axis"));
            assert_eq!(
                (axis.target, axis.first_requested, axis.high_water),
                (1, 2, 2)
            );
        }
        assert!(allocation.items(0).is_ok(), "an exceedance never latches");
    }

    /// The preflight item, container and string-byte counts of a Cargo
    /// build JSON line are accounting targets on the input account. Exact
    /// records nothing; under the targets the line still scans to the same
    /// item total and each axis records its truthful figure. `depth` bounds
    /// the container stack and stays a physical refusal.
    #[test]
    fn preflight_counts_record_their_targets_and_depth_stays_physical() {
        let line = br#"{"first":1,"second":2,"third":3}"#;
        let scan = |items: usize, containers: usize, string_bytes: usize| {
            let mut limits = Limits::standard();
            limits.items = items;
            limits.containers = containers;
            limits.string_bytes = string_bytes;
            let mut allocation = Allocation::new(&limits);
            let counted = preflight_json(line, 0, &limits, &mut allocation)
                .expect("the line scans regardless of its accounting targets");
            assert_eq!(counted, 3);
            allocation
        };
        assert!(!scan(3, 1, 6).exceedances().is_exceeded());
        let mut under = scan(2, 0, 5);
        for (name, expected) in [
            ("Cargo build JSON items", (2, 3, 3)),
            ("Cargo build JSON containers", (0, 1, 1)),
            ("Cargo build JSON string bytes", (5, 6, 6)),
        ] {
            let axis = *under
                .exceedances()
                .axis(name)
                .unwrap_or_else(|| panic!("`{name}` records its axis"));
            assert_eq!(
                (axis.target, axis.first_requested, axis.high_water),
                expected
            );
        }
        assert!(under.items(0).is_ok(), "an exceedance never latches");

        let mut limits = Limits::standard();
        limits.depth = 1;
        let mut allocation = Allocation::new(&limits);
        preflight_json(br#"[1]"#, 0, &limits, &mut allocation).expect("exact depth scans");
        assert!(matches!(
            preflight_json(br#"[[1]]"#, 0, &limits, &mut allocation),
            Err(crate::Error::Limit {
                resource: "Cargo build JSON depth",
                limit: 1,
                actual: 2,
            })
        ));
    }

    #[test]
    fn retained_recipes_and_simultaneous_entries_share_one_item_budget() {
        let recipes = [("a", "b"), ("c", "d"), ("e", "f")];
        let evaluate = |item_limit| {
            let mut limits = Limits::standard();
            limits.items = item_limit;
            let mut allocation = Allocation::new(&limits);
            let mut retained = Vec::new();
            for (name, value) in recipes {
                allocation.items(1)?;
                retained
                    .try_reserve(1)
                    .map_err(|error| crate::Error::Input {
                        code: "test-recipe-allocation",
                        reason: error.to_string().into_boxed_str(),
                    })?;
                retained.push(Recipe::fact(
                    Role::BuildMessage,
                    name,
                    value,
                    &mut allocation,
                )?);
            }
            proof::evaluate(&retained, false, &limits, &mut allocation)?;
            Ok::<_, crate::Error>(allocation)
        };

        // The exact retained Recipe plus Entry item total is 18. At the
        // exact target nothing is recorded; one item less records the item
        // axis with a high water of exactly 18 — the simultaneous Entry fanout
        // did not reset usage — and the work still completes unlatched.
        let exact =
            evaluate(18).expect("the exact retained Recipe plus Entry item boundary admits");
        assert!(!exact.exceedances().is_exceeded());
        assert_eq!(exact.usage().items, 18);
        let mut one_under =
            evaluate(17).expect("one item under the target is charged, not refused");
        let axis = *one_under
            .exceedances()
            .axis(INPUT_ITEMS_RESOURCE)
            .expect("the simultaneous Entry fanout must not reset usage");
        assert_eq!(
            (axis.target, axis.first_requested, axis.high_water),
            (17, 18, 18)
        );
        assert!(one_under.items(0).is_ok(), "an exceedance never latches");
    }

    #[test]
    fn retained_recipe_and_entry_string_bytes_are_cumulative() {
        let evaluate = |byte_limit| {
            let limits = Limits::standard();
            let mut allocation =
                Allocation::with_limits(limits.items, byte_limit, limits.string_bytes);
            allocation.items(1)?;
            let retained = [Recipe::fact(
                Role::BuildMessage,
                "n",
                "01234567890123456789012345678901",
                &mut allocation,
            )?];
            proof::evaluate(&retained, false, &limits, &mut allocation)?;
            Ok::<_, crate::Error>(allocation)
        };

        // The exact retained Recipe plus Entry byte total is 66. At the
        // exact target nothing is recorded; one byte less records the byte
        // axis with a high water of exactly 66 — the Entry clone retained the
        // prior Recipe bytes — and the work still completes unlatched.
        let exact =
            evaluate(66).expect("the exact retained Recipe plus Entry byte boundary admits");
        assert!(!exact.exceedances().is_exceeded());
        assert_eq!(exact.usage().bytes, 66);
        let mut one_under =
            evaluate(65).expect("one byte under the target is charged, not refused");
        let axis = *one_under
            .exceedances()
            .axis(INPUT_BYTES_RESOURCE)
            .expect("the Entry clone must retain prior Recipe bytes");
        assert_eq!(
            (axis.target, axis.first_requested, axis.high_water),
            (65, 66, 66)
        );
        assert!(one_under.bytes(0).is_ok(), "an exceedance never latches");
    }

    #[test]
    fn build_artifact_cfg_env_paths_share_exact_owned_allocation_boundaries() {
        let limits = Limits::standard();
        let exercise = |items, bytes| -> crate::Result<_> {
            let mut allocation = Allocation::with_limits(items, bytes, limits.string_bytes);
            let retained = strings(
                Some(&serde_json::json!(["a", "bb"])),
                "artifact filenames",
                &mut allocation,
            )?;
            let environment = pairs(Some(&serde_json::json!([["k", "v"]])), &mut allocation)?;
            allocation.path(Path::new("xy"))?;
            charge_serialized_value(&serde_json::json!({"x": 1}), &mut allocation)?;
            Ok((allocation, retained, environment))
        };

        let (unbounded, retained, environment) = exercise(usize::MAX, usize::MAX)
            .expect("unbounded fixture reveals the exact retained counters");
        let usage = unbounded.usage();
        let (exact, exact_retained, exact_environment) = exercise(usage.items, usage.bytes)
            .expect("exact Build artifact/cfg/env/path counters seal");
        assert_eq!(
            (exact.usage().items, exact.usage().bytes),
            (usage.items, usage.bytes)
        );
        assert!(!exact.exceedances().is_exceeded());
        assert_eq!(retained, exact_retained);
        assert_eq!(environment, exact_environment);
        // One item (or byte) under the exact total is charged, not
        // refused; the account records the axis with a high water equal to the
        // exact counter, which proves the counter without a refusal.
        let (mut under_items, retained_under, environment_under) =
            exercise(usage.items.saturating_sub(1), usage.bytes)
                .expect("one item under the exact total still completes");
        let items_axis = *under_items
            .exceedances()
            .axis(INPUT_ITEMS_RESOURCE)
            .expect("the item axis records the exact total");
        assert_eq!(
            (items_axis.target, items_axis.high_water),
            ((usage.items - 1) as u64, usage.items as u64)
        );
        assert!(under_items
            .exceedances()
            .axis(INPUT_BYTES_RESOURCE)
            .is_none());
        assert!(under_items.items(0).is_ok(), "an exceedance never latches");
        assert_eq!(
            (retained_under, environment_under),
            (retained.clone(), environment.clone())
        );
        let (mut under_bytes, _, _) = exercise(usage.items, usage.bytes.saturating_sub(1))
            .expect("one byte under the exact total still completes");
        let bytes_axis = *under_bytes
            .exceedances()
            .axis(INPUT_BYTES_RESOURCE)
            .expect("the byte axis records the exact total");
        assert_eq!(
            (bytes_axis.target, bytes_axis.high_water),
            ((usage.bytes - 1) as u64, usage.bytes as u64)
        );
        assert!(under_bytes
            .exceedances()
            .axis(INPUT_ITEMS_RESOURCE)
            .is_none());
        assert!(under_bytes.bytes(0).is_ok(), "an exceedance never latches");
    }

    #[test]
    fn exact_file_reader_accepts_zero_one_and_max_boundaries() {
        let path = std::env::temp_dir().join(format!(
            "rawr-input-reader-{}-{}",
            std::process::id(),
            READ_ID.fetch_add(1, Ordering::Relaxed)
        ));
        fs::write(&path, []).expect("the empty reader fixture is written");
        assert!(read_file("reader fixture", &path, 0)
            .expect("zero bytes fit the zero boundary")
            .is_empty());
        fs::write(&path, b"x").expect("the one-byte reader fixture is written");
        assert_eq!(
            read_file("reader fixture", &path, 1).expect("one byte fits the exact boundary"),
            b"x"
        );
        assert!(read_file("reader fixture", &path, 0).is_err());
        fs::remove_file(path).expect("the reader fixture is retired");
    }

    #[test]
    fn registry_and_all_baseline_file_classes_share_exact_read_lineage() {
        let root = std::env::temp_dir().join(format!(
            "rawr-accounted-reader-{}-{}",
            std::process::id(),
            READ_ID.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&root).expect("the accounted reader root is created");
        let files = [
            ("registry-marker", b"registry".as_slice()),
            ("authority", b"authority".as_slice()),
            ("probe", b"probe".as_slice()),
            ("config", b"config".as_slice()),
        ];
        for (name, bytes) in files {
            fs::write(root.join(name), bytes).expect("the accounted input is written");
        }
        let total_bytes = files.iter().map(|(_, bytes)| bytes.len()).sum::<usize>();
        let exercise = |items, bytes| -> crate::Result<_> {
            OWNED_ACCOUNTED_READS.with(|owned| owned.set(0));
            let limits = Limits::standard();
            let mut allocation = Allocation::with_limits(items, bytes, limits.string_bytes);
            for (name, _) in files {
                let _ = read_file_budgeted(
                    "combined fallback/baseline input",
                    &root.join(name),
                    limits.json_bytes,
                    &mut allocation,
                )?;
            }
            Ok(allocation)
        };

        let exact = exercise(files.len(), total_bytes)
            .expect("registry and every Baseline file class fit exact caps");
        assert_eq!(
            (exact.usage().items, exact.usage().bytes),
            (files.len(), total_bytes)
        );
        assert!(!exact.exceedances().is_exceeded());
        assert_eq!(
            OWNED_ACCOUNTED_READS.with(std::cell::Cell::get),
            files.len()
        );
        // One item (or byte) under the exact total is an accounting
        // exceedance, not a refusal: every accounted read still happens through
        // the owned lineage and the account records the axis with a high water
        // equal to the exact total.
        let mut under_items = exercise(files.len().saturating_sub(1), total_bytes)
            .expect("one item under the exact total still reads every file");
        assert_eq!(
            OWNED_ACCOUNTED_READS.with(std::cell::Cell::get),
            files.len()
        );
        let items_axis = *under_items
            .exceedances()
            .axis(INPUT_ITEMS_RESOURCE)
            .expect("the item axis records the exact read count");
        assert_eq!(
            (
                items_axis.target,
                items_axis.first_requested,
                items_axis.high_water
            ),
            (
                files.len().saturating_sub(1) as u64,
                files.len() as u64,
                files.len() as u64
            )
        );
        assert!(under_items
            .exceedances()
            .axis(INPUT_BYTES_RESOURCE)
            .is_none());
        assert!(under_items.items(0).is_ok(), "an exceedance never latches");
        let mut under_bytes = exercise(files.len(), total_bytes.saturating_sub(1))
            .expect("one byte under the exact total still reads every file");
        assert_eq!(
            OWNED_ACCOUNTED_READS.with(std::cell::Cell::get),
            files.len()
        );
        let bytes_axis = *under_bytes
            .exceedances()
            .axis(INPUT_BYTES_RESOURCE)
            .expect("the byte axis records the exact read total");
        assert_eq!(
            (
                bytes_axis.target,
                bytes_axis.first_requested,
                bytes_axis.high_water
            ),
            (
                total_bytes.saturating_sub(1) as u64,
                total_bytes as u64,
                total_bytes as u64
            )
        );
        assert!(under_bytes
            .exceedances()
            .axis(INPUT_ITEMS_RESOURCE)
            .is_none());
        assert!(under_bytes.bytes(0).is_ok(), "an exceedance never latches");
        fs::remove_dir_all(root).expect("the accounted reader root is retired");
    }

    #[test]
    fn pinned_env_dep_records_preserve_set_empty_and_unset_values() {
        let environment = dependency::environment(
            "target: source.rs\n# env-dep:SET=value=with=equals\n# env-dep:EMPTY=\n# env-dep:UNSET\n",
            &Limits::standard(),
            &mut Allocation::new(&Limits::standard()),
        )
        .expect("the pinned rustc env-dep grammar decodes");
        assert_eq!(
            environment.as_ref(),
            [
                (Box::from("EMPTY"), Some(Box::from(""))),
                (Box::from("SET"), Some(Box::from("value=with=equals"))),
                (Box::from("UNSET"), None),
            ]
        );
    }

    #[test]
    fn malformed_and_duplicate_env_dep_records_reject() {
        for text in [
            "target: source.rs\n# env-dep:\n",
            "target: source.rs\n# env-dep\n",
            "target: source.rs\n# env-dep:DUP=first\n# env-dep:DUP=second\n",
        ] {
            let limits = Limits::standard();
            let error = dependency::environment(text, &limits, &mut Allocation::new(&limits))
                .expect_err("malformed or duplicate env-dep records reject");
            assert!(
                format!("{error}").contains("env-dep"),
                "unexpected rejection: {error}"
            );
        }
        let limits = Limits::standard();
        let skew = dependency::Dependency::read(
            "unsupported-cargo-commit",
            Path::new("/not/read/when/skewed.d"),
            Path::new("/not/read/when/skewed"),
            &limits,
            &mut Allocation::new(&limits),
        )
        .err()
        .expect("dep-info decoding is pinned to one Cargo/rustc layout");
        assert!(format!("{skew}").contains("cargo-commit-skew"));
    }

    #[cfg(feature = "rustc-driver")]
    #[test]
    fn compiler_extern_join_rejects_every_wrong_cargo_key() {
        let output = output("dependency", "lib", "lib");
        let artifact_path = PathBuf::from("/fixture/libdependency.rlib");
        let mut artifact = artifact(&output);
        artifact.outputs = Box::from([artifact_path.clone()]);
        let build = Build {
            evidence: Evidence {
                usage: super::Usage { items: 0, bytes: 0 },
                packages: BTreeMap::new().into(),
                scripts: Box::default(),
                artifacts: Box::from([artifact]),
                environment: Box::default(),
            },
            package_files: BTreeMap::new(),
        };
        let graph = unit::BuildGraph::fixture_artifacts(Box::from([output.clone()]));
        build
            .validate_extern_artifact(
                &graph,
                output.semantic_key(),
                output.package_id(),
                output.target(),
                output.target_kinds(),
                output.crate_types(),
                output.crate_name(),
                output.features(),
                &artifact_path,
            )
            .expect("the exact Cargo key and public filename join");
        let semantic_error = build
            .validate_extern_artifact(
                &graph,
                "wrong-semantic-unit",
                output.package_id(),
                output.target(),
                output.target_kinds(),
                output.crate_types(),
                output.crate_name(),
                output.features(),
                &artifact_path,
            )
            .expect_err("a wrong semantic Cargo unit key rejects");
        assert!(format!("{semantic_error}").contains("extern-artifact-join"));
        for (package, target, artifact) in [
            ("wrong", output.target(), artifact_path.as_path()),
            (
                output.package_id(),
                Path::new("/wrong/src/lib.rs"),
                artifact_path.as_path(),
            ),
            (
                output.package_id(),
                output.target(),
                Path::new("/wrong/lib.rlib"),
            ),
        ] {
            let error = build
                .validate_extern_artifact(
                    &graph,
                    output.semantic_key(),
                    package,
                    target,
                    output.target_kinds(),
                    output.crate_types(),
                    output.crate_name(),
                    output.features(),
                    artifact,
                )
                .expect_err("a wrong Cargo key component must reject");
            assert!(format!("{error}").contains("extern-artifact-join"));
        }

        let duplicate_context = output
            .clone()
            .with_fixture_semantic_key("different-semantic-context");
        let ambiguous_graph =
            unit::BuildGraph::fixture_artifacts(Box::from([output.clone(), duplicate_context]));
        let ambiguous = build
            .validate_extern_artifact(
                &ambiguous_graph,
                output.semantic_key(),
                output.package_id(),
                output.target(),
                output.target_kinds(),
                output.crate_types(),
                output.crate_name(),
                output.features(),
                &artifact_path,
            )
            .expect_err("ambiguous Cargo semantic contexts reject authority");
        assert!(format!("{ambiguous}").contains("ambiguous-extern-artifact-join"));
    }

    fn output(name: &str, target_kind: &str, crate_type: &str) -> unit::Output {
        unit::Output::fixture(
            &format!("path+file:///fixture/{name}#0.1.0"),
            &format!("/fixture/{name}/src/lib.rs"),
            name,
            target_kind,
            crate_type,
        )
    }

    fn artifact(output: &unit::Output) -> Artifact {
        Artifact {
            package_id: output.package_id().into(),
            outputs: Box::default(),
            dep_info: PathBuf::from(format!("/fixture/{}.d", output.crate_name())),
            key: Key {
                package_id: output.package_id().into(),
                target: output.target().to_path_buf(),
                target_kinds: output.target_kinds().to_vec().into_boxed_slice(),
                crate_types: output.crate_types().to_vec().into_boxed_slice(),
                crate_name: output.crate_name().into(),
                features: output.features().to_vec().into_boxed_slice(),
            },
            profile: output.artifact_profile().clone(),
            fresh: false,
        }
    }

    fn script(package_id: &str, context: &str) -> Script {
        Script {
            compiler: None,
            package_id: package_id.into(),
            out_dir: PathBuf::from(format!("/fixture/build/{context}/out")),
            cfg: Box::default(),
            environment: Box::default(),
        }
    }

    /// A freshness or content input whose physical identity is unchanged
    /// takes its memoized digest instead of being read and hashed again; a
    /// rewritten file has a new identity and is hashed.
    #[test]
    fn an_unchanged_input_identity_is_not_hashed_again() {
        use crate::file_digest::{self, Domain, Identity};
        use sha2::Digest as _;

        let path = std::env::temp_dir().join(format!(
            "rawr-input-memo-{}-{}",
            std::process::id(),
            READ_ID.fetch_add(1, Ordering::Relaxed)
        ));
        fs::write(&path, b"original").expect("the memo fixture is written");
        let actual: [u8; 32] = sha2::Sha256::digest(b"original").into();
        assert_eq!(
            super::freshness_digest("memo fixture", &path, 8).unwrap(),
            actual
        );
        // A marked memo entry for the same identity proves the next reads
        // never touch the bytes.
        let identity = Identity::of(&fs::metadata(&path).unwrap());
        file_digest::insert(identity, Domain::Sha256, [7; 32]);
        assert_eq!(
            super::freshness_digest("memo fixture", &path, 8).unwrap(),
            [7; 32]
        );
        let mut allocation =
            Allocation::for_proof(Usage { items: 0, bytes: 0 }, &Limits::standard());
        assert_eq!(
            super::file_digest_accounted(&path, 64, &mut allocation).unwrap(),
            [7; 32]
        );
        fs::write(&path, b"replaced").expect("the memo fixture is rewritten");
        let replaced: [u8; 32] = sha2::Sha256::digest(b"replaced").into();
        assert_eq!(
            super::freshness_digest("memo fixture", &path, 8).unwrap(),
            replaced
        );
        fs::remove_file(&path).expect("the memo fixture is retired");
    }
}
