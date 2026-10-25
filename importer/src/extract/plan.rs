//! Sealed metadata-probe plan and deterministic protocol capture.

mod candidate;
pub(crate) mod namespace;
mod search;

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::fs::{self, OpenOptions};
use std::io::{self, Read as _, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};

use super::{protocol, Allocation, ExternalBinding, Invocations};
use crate::cargo::{self, resolution, toolchain, unit};
use crate::process::{self, Program};
use crate::{Error, Result};

const PLAN_SCHEMA: u32 = 8;
const DIRECTORY: &str = "extraction";
pub(crate) const PLAN_FILE: &str = "plan.json";
const PROTOCOL_DIRECTORY: &str = "protocol";
const ATTEMPTS_FILE: &str = "attempts";
const TARGET_DIRECTORY: &str = "authoritative-target";
const ACTIVE_DIRECTORY: &str = "active";
const PROBE_SOURCE_FILE: &str = "probe.rs";
const PROBE_SOURCE: &[u8] = b"#![no_std]\npub struct RawrMetadataProbe;\n";
const CANONICAL_PATH_BYTES: usize = 1024;

/// One invocation plan proven by the initial normal Build unit graph.
pub(crate) struct Plan {
    protocol_directory: PathBuf,
    target_directory: PathBuf,
    artifact_directory: PathBuf,
    plan_file: PathBuf,
    probe_source: PathBuf,
    compiler_commit: Box<str>,
    host: Box<str>,
    selected_target: Box<str>,
    entries: Box<[Entry<Prepared>]>,
    demand: Box<[cargo::DemandPath]>,
    reuse: Option<PathBuf>,
    allocation: Allocation,
    graph_budget: Option<crate::node::GraphBudget>,
}

/// One exact artifact-bound probe and its expected control bytes.
struct Probe {
    protocol_directory: PathBuf,
    artifact_directory: PathBuf,
    target_directory: PathBuf,
    plan_file: PathBuf,
    probe_source: PathBuf,
    compiler_commit: Box<str>,
    host: Box<str>,
    selected_target: Box<str>,
    entries: Vec<Entry<Sealed>>,
    artifacts: Vec<(PathBuf, u64, [u8; 32])>,
    plan_bytes: Vec<u8>,
    dependency_directories: Box<[PathBuf]>,
    namespace: Option<Namespace>,
    reuse: Option<PathBuf>,
    allocation: Allocation,
    graph_budget: Option<crate::node::GraphBudget>,
}

enum Namespace {
    Cold(search::Search, namespace::Snapshot),
    Replay(search::Search, Arc<namespace::Retained>),
}

/// Complete sidecars retained only after the Build postconditions held.
pub(crate) struct Capture {
    search: Option<search::Search>,
    namespace: Option<Arc<namespace::Retained>>,
    document: protocol::Document,
    /// Sealed replay decoded only the handshakes; `into_parts` decodes the
    /// complete document from the digest-checked sidecars.
    deferred: bool,
    document_bytes: usize,
    invocations: Invocations,
    sidecars: Box<[(PathBuf, [u8; 32])]>,
    controls: Box<[(PathBuf, [u8; 32])]>,
    artifacts: Box<[(PathBuf, u64, [u8; 32])]>,
    graph_budget: Option<crate::node::GraphBudget>,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct Prepared;

#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(deny_unknown_fields)]
struct Sealed {
    artifact: Artifact,
}

#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(deny_unknown_fields)]
struct Entry<State> {
    package: Package,
    root: Root,
    unit_key: Box<str>,
    dependencies: Box<[Box<str>]>,
    target: Box<str>,
    target_kind: Box<str>,
    target_kinds: Box<[Box<str>]>,
    crate_types: Box<[Box<str>]>,
    crate_name: Box<str>,
    edition: Box<str>,
    profile: Box<str>,
    features: Box<[Box<str>]>,
    invocation_nonce: Box<str>,
    unit_correlation: Box<str>,
    #[serde(flatten)]
    state: State,
}

#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(deny_unknown_fields)]
struct Artifact {
    crate_name: Box<str>,
    path: Box<str>,
    length: u64,
    digest: Box<str>,
    profile: Box<str>,
    fresh: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(deny_unknown_fields)]
struct Package {
    cargo_id: Box<str>,
    name: Box<str>,
    version: Box<str>,
    source: Box<str>,
}

#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum Root {
    Selected { aliases: Box<[Box<str>]> },
}

#[derive(Serialize)]
#[serde(deny_unknown_fields)]
struct Document<'a> {
    schema: u32,
    protocol_schema: u32,
    canonical_items: usize,
    canonical_bytes: usize,
    compiler_commit: &'a str,
    host: &'a str,
    selected_target: &'a str,
    entries: &'a [Entry<Sealed>],
    dependency_directories: &'a [Box<str>],
    /// Paths through wholesale crate re-exports to extract. Omitted
    /// when empty so every undemanded plan keeps its exact bytes.
    #[serde(skip_serializing_if = "<[_]>::is_empty")]
    demand: &'a [cargo::DemandPath],
}

impl Plan {
    pub(crate) fn prepare(
        harness_root: &Path,
        graph: &unit::BuildGraph,
        proof: &toolchain::Proof,
        selected_target: &str,
        limits: &cargo::Limits,
    ) -> Result<Self> {
        let units = graph.extraction();
        let mut allocation = Allocation::new(limits);
        // The unit count is an accounting target recorded on the plan's
        // canonical account; an empty selection is the correctness failure.
        allocation.exceeded("extraction plan units", limits.packages, units.len());
        if units.is_empty() {
            return Err(extraction(
                "missing-extraction-unit",
                "the initial Build graph selected no normal library units",
            ));
        }
        let run_nonce = run_nonce(
            graph.digest(),
            proof.rustc_commit(),
            selected_target,
            &mut allocation,
        )?;
        let extraction_root = join_path(harness_root, Path::new(DIRECTORY), &mut allocation)?;
        let root = join_path(
            &extraction_root,
            Path::new(ACTIVE_DIRECTORY),
            &mut allocation,
        )?;
        let protocol_directory = join_path(&root, Path::new(PROTOCOL_DIRECTORY), &mut allocation)?;
        let probe_source = join_path(&root, Path::new(PROBE_SOURCE_FILE), &mut allocation)?;
        let target_directory =
            join_path(harness_root, Path::new(TARGET_DIRECTORY), &mut allocation)?;
        fresh_directory(&root, "active extraction directory")?;
        create_directory(&protocol_directory, "extraction protocol directory")?;
        fresh_directory(&target_directory, "authoritative Build target directory")?;
        let root = canonical_under(
            harness_root,
            &root,
            "extraction run directory",
            &mut allocation,
        )?;
        let protocol_directory = canonical_under(
            &root,
            &protocol_directory,
            "extraction protocol directory",
            &mut allocation,
        )?;
        publish(&probe_source, PROBE_SOURCE, &mut allocation)?;
        let probe_source = canonical_under(
            &root,
            &probe_source,
            "metadata probe source",
            &mut allocation,
        )?;
        let target_directory = canonical_under(
            harness_root,
            &target_directory,
            "authoritative Build target directory",
            &mut allocation,
        )?;

        let entries = prepare_entries(units, &run_nonce, limits, &mut allocation)?;
        let plan_file = join_path(&root, Path::new(PLAN_FILE), &mut allocation)?;
        let artifact_directory = allocation.clone_path(&target_directory)?;
        Ok(Self {
            protocol_directory,
            target_directory,
            artifact_directory,
            plan_file,
            probe_source,
            compiler_commit: allocation.owned(proof.rustc_commit())?,
            host: allocation.owned(proof.host())?,
            selected_target: allocation.owned(selected_target)?,
            entries: entries.into_boxed_slice(),
            demand: Box::default(),
            reuse: None,
            allocation,
            graph_budget: None,
        })
    }

    /// Prepare controls in the directories of an unsealed resolution attempt.
    pub(crate) fn prepare_in(
        layout: &resolution::layout::Layout,
        graph: &unit::BuildGraph,
        proof: &toolchain::Proof,
        selected_target: &str,
        limits: &cargo::Limits,
    ) -> Result<Self> {
        Self::from_layout(layout.paths(), graph, proof, selected_target, limits, true).map(
            |mut plan| {
                plan.graph_budget = Some(layout.budget().clone());
                plan
            },
        )
    }

    /// Reconstruct the fixed expected plan and validate its existing controls and
    /// outputs without publishing files or launching the metadata Probe.
    #[allow(
        clippy::too_many_arguments,
        reason = "replay joins the existing fixed owner observations"
    )]
    #[cfg(test)]
    pub(crate) fn replay_in(
        layout: &resolution::layout::Paths,
        graph: &unit::BuildGraph,
        proof: &toolchain::Proof,
        selected_target: &str,
        build: &crate::input::build::Build,
        metadata: &unit::Metadata,
        limits: &cargo::Limits,
        graph_budget: &crate::node::GraphBudget,
    ) -> Result<Capture> {
        Self::replay_with_namespace(
            layout,
            graph,
            proof,
            selected_target,
            build,
            metadata,
            limits,
            graph_budget,
            None,
            None,
            &[],
        )
    }

    #[allow(
        clippy::too_many_arguments,
        reason = "replay binds original owner evidence"
    )]
    pub(crate) fn replay_with_namespace(
        layout: &resolution::layout::Paths,
        graph: &unit::BuildGraph,
        proof: &toolchain::Proof,
        selected_target: &str,
        build: &crate::input::build::Build,
        metadata: &unit::Metadata,
        limits: &cargo::Limits,
        graph_budget: &crate::node::GraphBudget,
        namespace: Option<Arc<namespace::Retained>>,
        inventory: Option<resolution::bundle::Inventory<'_>>,
        demand: &[cargo::DemandPath],
    ) -> Result<Capture> {
        let mut plan = Self::from_layout(layout, graph, proof, selected_target, limits, false)?
            .demand(demand)?;
        plan.graph_budget = Some(graph_budget.clone());
        let mut probe = plan.bind(build, graph, limits)?;
        if let Some(retained) = namespace {
            let (search, roots, explicit) = probe.namespace_inputs(proof.native_paths().1)?;
            if retained.roots != roots || retained.explicit != explicit {
                return Err(extraction(
                    "compiler-namespace",
                    "retained compiler search inputs differ",
                ));
            }
            // The shared decoded namespace is never rewritten here; a frozen
            // bundle root's permission transition is interpreted where the
            // inventory revalidates it, under that replay's own origin.
            probe.namespace = Some(Namespace::Replay(search, retained));
        }
        probe.validate_retained_with_inventory(
            build,
            graph,
            metadata,
            limits,
            inventory.map(|inventory| (layout.root(), inventory)),
        )
    }

    fn from_layout(
        layout: &resolution::layout::Paths,
        graph: &unit::BuildGraph,
        proof: &toolchain::Proof,
        selected_target: &str,
        limits: &cargo::Limits,
        publish_source: bool,
    ) -> Result<Self> {
        let units = graph.extraction();
        let mut allocation = Allocation::new(limits);
        // The unit count is an accounting target recorded on the plan's
        // canonical account; an empty selection is the correctness failure.
        allocation.exceeded("extraction plan units", limits.packages, units.len());
        if units.is_empty() {
            return Err(extraction(
                "missing-extraction-unit",
                "the initial Build graph selected no normal library units",
            ));
        }
        let run_nonce = run_nonce(
            graph.digest(),
            proof.rustc_commit(),
            selected_target,
            &mut allocation,
        )?;
        let extraction = layout.extraction();
        let root = canonical_directory_under(
            layout.root(),
            extraction.root(),
            "extraction run directory",
            &mut allocation,
        )?;
        let protocol_directory = canonical_directory_under(
            &root,
            extraction.protocol(),
            "extraction protocol directory",
            &mut allocation,
        )?;
        let target_directory = canonical_directory_under(
            layout.root(),
            layout.final_form().target(),
            "authoritative Build target directory",
            &mut allocation,
        )?;
        // Pinned Cargo Layout keeps dependency artifacts below build_root even
        // when its command target directory names a separate final-output root.
        let artifact_directory = canonical_directory_under(
            layout.root(),
            layout.final_form().build(),
            "authoritative Build artifact directory",
            &mut allocation,
        )?;
        // Layout owns these fixed names. Validate its existing parent directories
        // before publishing, and leave every other output in the attempt intact.
        if publish_source {
            publish(extraction.source(), PROBE_SOURCE, &mut allocation)?;
        }
        let probe_source = canonical_under(
            &root,
            extraction.source(),
            "metadata probe source",
            &mut allocation,
        )?;
        if !publish_source && probe_source.as_os_str() != extraction.source().as_os_str() {
            return Err(Error::Path {
                field: "metadata probe source",
                path: probe_source,
                reason: "must retain its exact reserved path".into(),
            });
        }
        let entries = prepare_entries(units, &run_nonce, limits, &mut allocation)?;
        let plan_file = allocation.clone_path(extraction.plan())?;
        Ok(Self {
            protocol_directory,
            target_directory,
            artifact_directory,
            plan_file,
            probe_source,
            compiler_commit: allocation.owned(proof.rustc_commit())?,
            host: allocation.owned(proof.host())?,
            selected_target: allocation.owned(selected_target)?,
            entries: entries.into_boxed_slice(),
            demand: Box::default(),
            reuse: None,
            allocation,
            graph_budget: None,
        })
    }

    /// Extract the items these paths name through wholesale crate
    /// re-exports of the planned units.
    pub(crate) fn demand(mut self, demand: &[cargo::DemandPath]) -> Result<Self> {
        let mut retained = Vec::new();
        self.allocation.reserve(&mut retained, demand.len())?;
        for path in demand {
            retained.push(clone_strings(path, &mut self.allocation)?);
        }
        self.demand = retained.into_boxed_slice();
        Ok(self)
    }

    /// Let the driver keep and reuse unchanged crates' extraction
    /// bodies in `directory`, created here.
    pub(crate) fn reuse(mut self, directory: &Path) -> Result<Self> {
        fs::create_dir_all(directory).map_err(|source| Error::Io {
            operation: "create extraction reuse directory",
            path: directory.to_path_buf(),
            source,
        })?;
        self.reuse = Some(self.allocation.clone_path(directory)?);
        Ok(self)
    }

    pub(crate) fn target_directory(&self) -> &Path {
        &self.target_directory
    }

    pub(crate) fn protocol_directory(&self) -> &Path {
        &self.protocol_directory
    }

    pub(crate) fn command_target(&mut self) -> Result<PathBuf> {
        self.allocation.clone_path(&self.target_directory)
    }

    pub(crate) fn seal(
        self,
        native: &impl crate::process::ProductionReplay,
        build: &crate::input::build::Build,
        graph: &unit::BuildGraph,
        metadata: &unit::Metadata,
        limits: &cargo::Limits,
    ) -> Result<Capture> {
        self.bind(build, graph, limits)?
            .run(native, limits)?
            .validate_outputs(build, graph, metadata, limits, native.evidence_session())
    }

    fn bind(
        mut self,
        build: &crate::input::build::Build,
        graph: &unit::BuildGraph,
        limits: &cargo::Limits,
    ) -> Result<Probe> {
        let selected = build.selected_artifacts(graph, limits, &mut self.allocation)?;
        if selected.len() != self.entries.len() {
            return Err(extraction(
                "selected-artifact-cardinality",
                "Cargo artifacts do not cover every planned semantic unit",
            ));
        }
        let mut artifacts = Vec::new();
        let mut allocation = self.allocation;
        let prepared_entries = std::mem::take(&mut self.entries);
        let mut entries = Vec::new();
        allocation.reserve(&mut entries, prepared_entries.len())?;
        for entry in prepared_entries.into_vec() {
            let mut matches = selected
                .iter()
                .filter(|artifact| artifact.semantic_key.as_ref() == entry.unit_key.as_ref());
            let selected = matches
                .next()
                .ok_or_else(|| extraction("missing-selected-artifact", entry.unit_key.as_ref()))?;
            if matches.next().is_some() {
                return Err(extraction(
                    "ambiguous-selected-artifact",
                    entry.unit_key.as_ref(),
                ));
            }
            if selected.package_id != entry.package.cargo_id
                || selected.target != Path::new(entry.target.as_ref())
                || selected.target_kinds != entry.target_kinds
                || selected.crate_types != entry.crate_types
                || selected.crate_name != entry.crate_name
                || selected.features != entry.features
            {
                return Err(extraction(
                    "selected-artifact-unit-mismatch",
                    entry.unit_key.as_ref(),
                ));
            }
            let path = canonical_under(
                &self.artifact_directory,
                &selected.rmeta,
                "selected rmeta artifact",
                &mut allocation,
            )?;
            let profile = std::str::from_utf8(selected.profile.as_bytes())
                .map_err(|error| extraction("selected-artifact-profile", &error.to_string()))?;
            let digest = allocation.hex(selected.digest)?;
            let artifact = Artifact {
                crate_name: allocation.owned(&selected.crate_name)?,
                path: allocation.owned(path_string(&path, "selected rmeta artifact")?)?,
                length: selected.length,
                digest,
                profile: allocation.owned(profile)?,
                fresh: selected.fresh,
            };
            insert_artifact(
                &mut artifacts,
                path,
                selected.length,
                selected.digest,
                limits,
                &mut allocation,
            )?;
            entries.push(entry.seal(artifact));
        }
        artifacts.sort_unstable_by(|left, right| left.0.cmp(&right.0));
        let probe_directories = build.probe_directories(graph, limits, &mut allocation)?;
        let mut dependency_directories = Vec::new();
        allocation.reserve(&mut dependency_directories, probe_directories.len())?;
        for directory in &probe_directories {
            dependency_directories
                .push(allocation.owned(path_string(directory, "probe artifact directory")?)?);
        }
        dependency_directories.sort_unstable();
        dependency_directories.dedup();
        let document = Document {
            schema: PLAN_SCHEMA,
            protocol_schema: protocol::SCHEMA,
            canonical_items: limits.canonical_items,
            canonical_bytes: limits.canonical_bytes,
            compiler_commit: &self.compiler_commit,
            host: &self.host,
            selected_target: &self.selected_target,
            entries: &entries,
            dependency_directories: &dependency_directories,
            demand: &self.demand,
        };
        let mut measured = CountedBytes { bytes: 0 };
        serde_json::to_writer(&mut measured, &document).map_err(|error| Error::Extraction {
            code: "probe-plan-encoding",
            reason: error.to_string().into_boxed_str(),
        })?;
        // The plan document size is an accounting target recorded on
        // the canonical account, never a refusal.
        allocation.exceeded(
            "metadata probe plan bytes",
            limits.json_bytes,
            measured.bytes,
        );
        allocation.items(1)?;
        allocation.bytes(measured.bytes)?;
        let mut plan_bytes = Vec::new();
        plan_bytes
            .try_reserve_exact(measured.bytes)
            .map_err(|error| Error::Allocation {
                operation: "probe plan allocation",
                source: error,
            })?;
        serde_json::to_writer(&mut plan_bytes, &document).map_err(|error| Error::Extraction {
            code: "probe-plan-encoding",
            reason: error.to_string().into_boxed_str(),
        })?;
        if plan_bytes.len() != measured.bytes {
            return Err(extraction(
                "probe-plan-measurement",
                "probe plan changed between measurement and encoding",
            ));
        }
        Ok(Probe {
            protocol_directory: self.protocol_directory,
            artifact_directory: self.artifact_directory,
            target_directory: self.target_directory,
            plan_file: self.plan_file,
            probe_source: self.probe_source,
            compiler_commit: self.compiler_commit,
            host: self.host,
            selected_target: self.selected_target,
            entries,
            artifacts,
            plan_bytes,
            dependency_directories: probe_directories,
            namespace: None,
            reuse: self.reuse,
            allocation,
            graph_budget: self.graph_budget,
        })
    }
}

impl Probe {
    fn namespace_inputs(
        &mut self,
        rustc: &Path,
    ) -> Result<(search::Search, Vec<PathBuf>, Vec<PathBuf>)> {
        let graph = self
            .graph_budget
            .as_ref()
            .ok_or_else(|| extraction("compiler-namespace", "original graph absent"))?;
        let search = search::Search::observe(
            rustc,
            &self.compiler_commit,
            &self.host,
            &self.selected_target,
            &mut self.allocation,
            graph,
        )?;
        let mut roots = Vec::new();
        graph
            .decode((self.dependency_directories.len() + 1) * std::mem::size_of::<PathBuf>())
            .map_err(artifact_graph_error)?;
        self.allocation
            .reserve(&mut roots, self.dependency_directories.len() + 1)?;
        for path in self
            .dependency_directories
            .iter()
            .chain(std::iter::once(&search.library))
        {
            graph
                .decode(path.as_os_str().len())
                .map_err(artifact_graph_error)?;
            roots.push(self.allocation.clone_path(path)?);
        }
        let mut explicit = Vec::new();
        graph
            .decode(self.entries.len() * std::mem::size_of::<PathBuf>())
            .map_err(artifact_graph_error)?;
        self.allocation.reserve(&mut explicit, self.entries.len())?;
        for entry in &self.entries {
            let path = Path::new(entry.state.artifact.path.as_ref());
            graph
                .decode(path.as_os_str().len())
                .map_err(artifact_graph_error)?;
            explicit.push(self.allocation.clone_path(path)?);
        }

        Ok((search, roots, explicit))
    }

    fn run(
        mut self,
        native: &impl crate::process::ProductionReplay,
        limits: &cargo::Limits,
    ) -> Result<Self> {
        if self.graph_budget.is_some() {
            let (search, roots, explicit) = self.namespace_inputs(native.rustc())?;
            let graph = self
                .graph_budget
                .as_ref()
                .ok_or_else(|| extraction("compiler-namespace", "original graph absent"))?;
            let snapshot = namespace::Snapshot::capture(
                &roots,
                &explicit,
                &mut self.allocation,
                graph,
                process::CAPTURE_BYTES,
            )?;
            self.namespace = Some(Namespace::Cold(search, snapshot));
        }
        let allocation = &mut self.allocation;
        publish(&self.plan_file, &self.plan_bytes, allocation)?;
        let mut arguments = Vec::new();
        allocation.reserve(&mut arguments, 6)?;
        allocation.bytes(
            "--probe"
                .len()
                .saturating_add(native.rustc().as_os_str().len())
                .saturating_add(self.plan_file.as_os_str().len())
                .saturating_add(self.protocol_directory.as_os_str().len())
                .saturating_add(self.probe_source.as_os_str().len()),
        )?;
        arguments.push("--probe".into());
        arguments.push(native.rustc().as_os_str().to_owned());
        arguments.push(self.plan_file.as_os_str().to_owned());
        arguments.push(self.protocol_directory.as_os_str().to_owned());
        arguments.push(self.probe_source.as_os_str().to_owned());
        if let Some(reuse) = &self.reuse {
            allocation.bytes(reuse.as_os_str().len())?;
            arguments.push(reuse.as_os_str().to_owned());
        }
        let command_directory = self
            .plan_file
            .parent()
            .ok_or_else(|| extraction("probe-plan-parent", "plan has no parent"))?;
        let command_directory = allocation.clone_path(command_directory)?;
        let command = process::Command::probe(arguments, command_directory, Box::default())?;
        let _ = native
            .run(&command, limits.process_limits())?
            .checked(Program::Driver)?;
        Ok(self)
    }

    #[cfg(test)]
    fn validate_retained(
        self,
        build: &crate::input::build::Build,
        graph: &unit::BuildGraph,
        metadata: &unit::Metadata,
        limits: &cargo::Limits,
    ) -> Result<Capture> {
        self.validate_retained_with_inventory(build, graph, metadata, limits, None)
    }

    fn validate_retained_with_inventory(
        mut self,
        build: &crate::input::build::Build,
        graph: &unit::BuildGraph,
        metadata: &unit::Metadata,
        limits: &cargo::Limits,
        inventory: Option<(&Path, resolution::bundle::Inventory<'_>)>,
    ) -> Result<Capture> {
        for (path, expected) in [
            (&self.plan_file, self.plan_bytes.as_slice()),
            (&self.probe_source, PROBE_SOURCE),
        ] {
            let bytes =
                read_bounded_accounted(path, &mut self.allocation, self.graph_budget.as_ref())?;
            if let Some((root, inventory)) = &inventory {
                inventory
                    .require_bytes(
                        root,
                        path,
                        &bytes,
                        self.graph_budget.as_ref().ok_or_else(|| {
                            extraction("resolution-inventory", "original graph absent")
                        })?,
                    )
                    .map_err(|source| Error::Node {
                        operation: "authenticate probe control",
                        source: Box::new(source),
                    })?;
            }
            if bytes.as_ref() != expected {
                return Err(extraction(
                    "metadata-probe-control-drift",
                    &path.display().to_string(),
                ));
            }
        }
        let mut capture = self.validate_outputs_with_inventory(
            build,
            graph,
            metadata,
            limits,
            None,
            inventory
                .as_ref()
                .map(|(root, inventory)| (*root, inventory)),
        )?;
        capture.revalidate_with_inventory(limits, inventory)?;
        Ok(capture)
    }

    fn validate_outputs(
        self,
        build: &crate::input::build::Build,
        graph: &unit::BuildGraph,
        metadata: &unit::Metadata,
        limits: &cargo::Limits,
        evidence: Option<&process::EvidenceSession>,
    ) -> Result<Capture> {
        self.validate_outputs_with_inventory(build, graph, metadata, limits, evidence, None)
    }

    fn validate_outputs_with_inventory(
        self,
        build: &crate::input::build::Build,
        graph: &unit::BuildGraph,
        metadata: &unit::Metadata,
        limits: &cargo::Limits,
        evidence: Option<&process::EvidenceSession>,
        inventory: Option<(&Path, &resolution::bundle::Inventory<'_>)>,
    ) -> Result<Capture> {
        let mut allocation = self.allocation;
        let entries = self.entries;
        let mut artifacts = self.artifacts;
        let plan_bytes = self.plan_bytes;
        let mut names = BTreeSet::new();
        if let Some(graph) = &self.graph_budget {
            graph.paths(1).map_err(attempt_graph_error)?;
            crate::store::path::input(&self.protocol_directory, graph)
                .map_err(attempt_graph_error)?;
        }
        let directory = fs::read_dir(&self.protocol_directory).map_err(|source| Error::Io {
            operation: "inspect extraction protocol directory",
            path: self.protocol_directory.clone(),
            source,
        })?;
        for (index, item) in directory.enumerate() {
            limited(
                "extraction protocol files",
                entries.len().saturating_add(1),
                index.saturating_add(1),
            )?;
            let item = item.map_err(|source| Error::Io {
                operation: "inspect extraction protocol entry",
                path: self.protocol_directory.clone(),
                source,
            })?;
            if let Some(graph) = &self.graph_budget {
                graph
                    .decode(CANONICAL_PATH_BYTES + std::mem::size_of::<PathBuf>())
                    .map_err(attempt_graph_error)?;
            }
            let path = directory_entry_path(&item, &mut allocation)?;
            if let Some(graph) = &self.graph_budget {
                graph.paths(1).map_err(attempt_graph_error)?;
                crate::store::path::input(&path, graph).map_err(attempt_graph_error)?;
            }
            let metadata = fs::symlink_metadata(&path).map_err(|source| Error::Io {
                operation: "inspect extraction protocol file",
                path: path.clone(),
                source,
            })?;
            if !metadata.is_file() || metadata.file_type().is_symlink() {
                return Err(Error::Path {
                    field: "extraction protocol file",
                    path,
                    reason: "must be a regular non-symlink file".into(),
                });
            }
            let name = path
                .file_name()
                .and_then(|value| value.to_str())
                .ok_or_else(|| Error::Extraction {
                    code: "invalid-sidecar-name",
                    reason: "sidecar filename is not UTF-8".into(),
                })?;
            admit_protocol_name(name.len(), self.graph_budget.as_ref())?;
            allocation.items(1)?;
            if !names.insert(allocation.owned(name)?) {
                return Err(extraction(
                    "duplicate-sidecar",
                    "duplicate protocol filename",
                ));
            }
        }
        let mut expected_names = BTreeSet::new();
        admit_protocol_name(ATTEMPTS_FILE.len(), self.graph_budget.as_ref())?;
        allocation.items(1)?;
        expected_names.insert(allocation.owned(ATTEMPTS_FILE)?);
        for entry in &entries {
            admit_protocol_name(
                entry.unit_correlation.len() + ".json".len(),
                self.graph_budget.as_ref(),
            )?;
            allocation.items(1)?;
            expected_names.insert(sidecar_name(
                &entry.unit_correlation,
                limits,
                &mut allocation,
            )?);
        }
        if names != expected_names {
            return Err(extraction(
                "missing-or-extra-sidecar",
                "the protocol directory does not exactly match the metadata-probe plan",
            ));
        }

        allocation.items(1)?;
        let mut document = Vec::new();
        let mut sidecars = Vec::new();
        append_document_prefix(&mut document, limits, &mut allocation)?;
        for (index, entry) in entries.iter().enumerate() {
            if index != 0 {
                append(&mut document, b",", limits, &mut allocation)?;
            }
            let name = sidecar_name(&entry.unit_correlation, limits, &mut allocation)?;
            let path = join_path(
                &self.protocol_directory,
                Path::new(name.as_ref()),
                &mut allocation,
            )?;
            let bytes = read_bounded_accounted(&path, &mut allocation, self.graph_budget.as_ref())?;
            if let Some((root, inventory)) = inventory {
                inventory
                    .require_bytes(
                        root,
                        &path,
                        &bytes,
                        self.graph_budget.as_ref().ok_or_else(|| {
                            extraction("resolution-inventory", "original graph absent")
                        })?,
                    )
                    .map_err(|source| Error::Node {
                        operation: "authenticate probe sidecar",
                        source: Box::new(source),
                    })?;
            }
            allocation.reserve(&mut sidecars, 1)?;
            sidecars.push((path, Sha256::digest(&bytes).into()));
            append(&mut document, &bytes, limits, &mut allocation)?;
        }
        append(&mut document, b"]}", limits, &mut allocation)?;
        // Bytes the sealed inventory authenticated were fully decoded when they
        // were sealed; every check below reads only their handshakes.
        let deferred = inventory.is_some() && evidence.is_none();
        let decoded = match evidence {
            Some(evidence) => evidence
                .observe_inline(process::EvidenceInlineStage::Decode, || {
                    protocol::decode_accounted(&document, limits, &mut allocation)
                })?,
            None if deferred => protocol::decode_handshakes(&document)?,
            None => protocol::decode_accounted(&document, limits, &mut allocation)?,
        };
        validate_fragments(
            &decoded.fragments,
            &entries,
            &self.compiler_commit,
            &self.host,
            &self.selected_target,
            limits,
            &mut allocation,
        )?;
        if let Some(graph) = &self.graph_budget {
            let path_bytes = self
                .protocol_directory
                .as_os_str()
                .len()
                .checked_add(1 + ATTEMPTS_FILE.len())
                .and_then(|bytes| bytes.checked_add(std::mem::size_of::<(PathBuf, [u8; 32])>()))
                .ok_or_else(|| {
                    extraction("compiler-attempt-completion", "path allocation overflow")
                })?;
            graph.decode(path_bytes).map_err(attempt_graph_error)?;
        }
        let attempts_path = join_path(
            &self.protocol_directory,
            Path::new(ATTEMPTS_FILE),
            &mut allocation,
        )?;
        let attempts_bytes =
            read_attempts(&attempts_path, &mut allocation, self.graph_budget.as_ref())?;
        if let Some((root, inventory)) = inventory {
            inventory
                .require_bytes(
                    root,
                    &attempts_path,
                    &attempts_bytes,
                    self.graph_budget.as_ref().ok_or_else(|| {
                        extraction("resolution-inventory", "original graph absent")
                    })?,
                )
                .map_err(|source| Error::Node {
                    operation: "authenticate probe attempts",
                    source: Box::new(source),
                })?;
        }
        let completion = protocol::attempt::Completion::decode(&attempts_bytes)
            .map_err(|reason| extraction("compiler-attempt-completion", reason))?;
        let arguments_digest = decoded
            .fragments
            .first()
            .ok_or_else(|| extraction("compiler-attempt-completion", "validated fragments absent"))?
            .handshake
            .probe_arguments_digest
            .as_str();
        completion
            .require_invocation(
                arguments_digest,
                entries.iter().map(|entry| protocol::attempt::Invocation {
                    nonce: &entry.invocation_nonce,
                    correlation: &entry.unit_correlation,
                }),
            )
            .map_err(|reason| extraction("compiler-attempt-completion", reason))?;
        let (namespace, search) = match self.namespace {
            Some(Namespace::Cold(before, snapshot)) => {
                let graph = self
                    .graph_budget
                    .as_ref()
                    .ok_or_else(|| extraction("compiler-namespace", "original graph absent"))?;
                let after = search::Search::observe(
                    &before.rustc,
                    &self.compiler_commit,
                    &self.host,
                    &self.selected_target,
                    &mut allocation,
                    graph,
                )?;
                if before != after {
                    return Err(extraction(
                        "compiler-namespace",
                        "compiler search selector changed",
                    ));
                }
                let retained = snapshot.complete(
                    &completion,
                    &mut allocation,
                    graph,
                    process::CAPTURE_BYTES,
                )?;
                // The shared holder's two counters are retained with the namespace.
                allocation.bytes(2 * std::mem::size_of::<usize>())?;
                (Some(Arc::new(retained)), Some(after))
            }
            Some(Namespace::Replay(before, retained)) => {
                let graph = self
                    .graph_budget
                    .as_ref()
                    .ok_or_else(|| extraction("compiler-namespace", "original graph absent"))?;
                let after = search::Search::observe(
                    &before.rustc,
                    &self.compiler_commit,
                    &self.host,
                    &self.selected_target,
                    &mut allocation,
                    graph,
                )?;
                if before != after {
                    return Err(extraction(
                        "compiler-namespace",
                        "compiler search selector changed",
                    ));
                }
                retained.require_selection(&completion, &mut allocation, graph)?;
                // validate_retained immediately calls Capture::revalidate for
                // this replay's current membership/content check.
                (Some(retained), Some(after))
            }
            None => (None, None),
        };
        // The namespace observes supplied candidates; content-role union and
        // persisted replay must still qualify any later omission.
        allocation.reserve(&mut sidecars, 1)?;
        sidecars.push((attempts_path, Sha256::digest(&attempts_bytes).into()));
        allocation.items(entries.len())?;
        let mut selected_roots = BTreeSet::new();
        for entry in &entries {
            selected_roots.insert(entry.unit_key.as_ref());
        }
        for fragment in &decoded.fragments {
            for loaded in &fragment.handshake.loaded_crates {
                let mut authority = None;
                for raw_path in &loaded.artifacts {
                    let path = canonical_under_either(
                        &self.artifact_directory,
                        &self.target_directory,
                        Path::new(raw_path),
                        "loaded compiler artifact",
                        &mut allocation,
                    )?;
                    let resolved =
                        match build.authority_for_artifact(graph, &path, &mut allocation)? {
                            crate::input::ArtifactJoin::Available(authority) => authority,
                            crate::input::ArtifactJoin::Missing => {
                                return Err(extraction("missing-loaded-artifact-join", raw_path));
                            }
                            crate::input::ArtifactJoin::Ambiguous => {
                                return Err(extraction("ambiguous-loaded-artifact-join", raw_path));
                            }
                        };
                    if resolved.crate_name.as_ref() != loaded.crate_name
                        || !graph
                            .artifact_reachable(&selected_roots, resolved.semantic_key.as_ref())?
                    {
                        return Err(extraction("unreachable-loaded-artifact", raw_path));
                    }
                    match &authority {
                        Some(previous) if previous != &resolved => {
                            return Err(extraction("split-loaded-crate-authority", raw_path));
                        }
                        Some(_) => {}
                        None => authority = Some(resolved),
                    }
                    let (length, digest) = artifact_digest(
                        &path,
                        process::CAPTURE_BYTES,
                        &mut allocation,
                        self.graph_budget.as_ref(),
                    )?;
                    insert_artifact(
                        &mut artifacts,
                        path,
                        length,
                        digest,
                        limits,
                        &mut allocation,
                    )?;
                }
            }
        }
        artifacts.sort_unstable_by(|left, right| left.0.cmp(&right.0));
        let mut external = Vec::new();
        for fragment in &decoded.fragments {
            for external_crate in &fragment.handshake.external_crates {
                let mut join = ExternalJoin::default();
                for artifact in &external_crate.artifacts {
                    join.observe(
                        build.authority_for_artifact(
                            graph,
                            Path::new(artifact),
                            &mut allocation,
                        )?,
                        artifact,
                    )?;
                }
                let package_identity = match join
                    .authority
                    .as_ref()
                    .and_then(|authority| metadata.package_identity(&authority.package_id))
                {
                    Some((name, version, source)) => Some((
                        allocation.owned(name)?,
                        allocation.owned(version)?,
                        allocation.owned(source)?,
                    )),
                    None => None,
                };
                let mut external_artifacts = Vec::new();
                allocation.reserve(&mut external_artifacts, external_crate.artifacts.len())?;
                for artifact in &external_crate.artifacts {
                    external_artifacts.push(allocation.owned(artifact)?);
                }
                allocation.reserve(&mut external, 1)?;
                external.push(ExternalBinding {
                    crate_name: allocation.owned(&external_crate.crate_name)?,
                    stable_crate_id: allocation.owned(&external_crate.stable_crate_id)?,
                    artifacts: external_artifacts.into_boxed_slice(),
                    authority: join.authority,
                    package: package_identity,
                });
            }
        }
        let mut controls = Vec::new();
        allocation.reserve(&mut controls, 2)?;
        controls.push((self.plan_file, Sha256::digest(&plan_bytes).into()));
        controls.push((self.probe_source, Sha256::digest(PROBE_SOURCE).into()));
        let invocations = Invocations::seal_document_with_allocation(&decoded, allocation, limits)?
            .bind_external(external, limits)?;
        Ok(Capture {
            search,
            namespace,
            document: decoded,
            deferred,
            document_bytes: document.len(),
            invocations,
            sidecars: sidecars.into_boxed_slice(),
            controls: controls.into_boxed_slice(),
            artifacts: artifacts.into_boxed_slice(),
            graph_budget: self.graph_budget,
        })
    }
}

/// Counts encoded bytes; only checked-arithmetic overflow stops the writer.
struct CountedBytes {
    bytes: usize,
}

#[derive(Default)]
struct ExternalJoin {
    authority: Option<crate::input::ArtifactAuthority>,
    missing: bool,
}

impl ExternalJoin {
    fn observe(&mut self, join: crate::input::ArtifactJoin, artifact: &str) -> Result<()> {
        match join {
            crate::input::ArtifactJoin::Missing => {
                if self.authority.is_some() {
                    return Err(extraction("partial-external-crate-authority", artifact));
                }
                self.missing = true;
            }
            crate::input::ArtifactJoin::Ambiguous => {
                return Err(extraction("ambiguous-external-artifact-join", artifact));
            }
            crate::input::ArtifactJoin::Available(resolved) => {
                if self.missing {
                    return Err(extraction("partial-external-crate-authority", artifact));
                }
                match &self.authority {
                    Some(previous) if previous != &resolved => {
                        return Err(extraction("split-external-crate-authority", artifact));
                    }
                    Some(_) => {}
                    None => self.authority = Some(resolved),
                }
            }
        }
        Ok(())
    }
}

fn insert_artifact(
    artifacts: &mut Vec<(PathBuf, u64, [u8; 32])>,
    path: PathBuf,
    length: u64,
    digest: [u8; 32],
    limits: &cargo::Limits,
    allocation: &mut Allocation,
) -> Result<()> {
    if let Some((_, previous_length, previous_digest)) =
        artifacts.iter().find(|artifact| artifact.0 == path)
    {
        if *previous_length != length || *previous_digest != digest {
            return Err(extraction(
                "conflicting-loaded-artifact",
                &path.display().to_string(),
            ));
        }
        return Ok(());
    }
    allocation.exceeded(
        "loaded compiler artifacts",
        limits.units,
        artifacts.len().saturating_add(1),
    );
    allocation.reserve(artifacts, 1)?;
    artifacts.push((path, length, digest));
    Ok(())
}

fn sidecar_name(
    correlation: &str,
    limits: &cargo::Limits,
    allocation: &mut Allocation,
) -> Result<Box<str>> {
    let length = correlation.len().saturating_add(".json".len());
    allocation.exceeded("compiler sidecar filename", limits.string_bytes, length);
    allocation.items(1)?;
    allocation.bytes(length)?;
    let mut name = String::new();
    name.try_reserve_exact(length)
        .map_err(|error| Error::Allocation {
            operation: "sidecar name allocation",
            source: error,
        })?;
    name.push_str(correlation);
    name.push_str(".json");
    Ok(name.into_boxed_str())
}

impl Write for CountedBytes {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        let Some(next) = self.bytes.checked_add(bytes.len()) else {
            return Err(io::Error::other(
                "metadata probe plan byte count overflowed",
            ));
        };
        self.bytes = next;
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl Capture {
    pub(crate) fn namespace(&self) -> Option<&namespace::Retained> {
        self.namespace.as_deref()
    }

    pub(super) fn allocation_mut(&mut self) -> &mut Allocation {
        &mut self.invocations.allocation
    }

    /// Borrow the artifact inputs in the order used by baseline capture.
    pub(crate) fn input_artifacts(&self) -> impl ExactSizeIterator<Item = (&Path, &[u8; 32])> {
        self.artifacts
            .iter()
            .map(|(path, _, digest)| (path.as_path(), digest))
    }

    pub(crate) fn artifact_inputs(
        &mut self,
        limits: &cargo::Limits,
    ) -> Result<Vec<(PathBuf, [u8; 32])>> {
        self.invocations.allocation.exceeded(
            "probe artifact input items",
            limits.items,
            self.artifacts.len(),
        );
        let bytes = self.artifacts.iter().fold(0_usize, |total, (path, _, _)| {
            total.saturating_add(path.as_os_str().len())
        });
        self.invocations.allocation.exceeded(
            "probe artifact input bytes",
            limits.json_bytes,
            bytes,
        );
        let mut inputs = Vec::new();
        self.invocations
            .allocation
            .reserve(&mut inputs, self.artifacts.len())?;
        for (path, _, digest) in &self.artifacts {
            self.invocations.allocation.bytes(path.as_os_str().len())?;
            inputs.push((path.clone(), *digest));
        }
        Ok(inputs)
    }

    pub(crate) fn revalidate(&mut self, limits: &cargo::Limits) -> Result<()> {
        self.revalidate_with_inventory(limits, None)
    }

    /// Every artifact, sidecar and namespace read here binds to the physical
    /// capture constant; the accounting limits play no part.
    fn revalidate_with_inventory(
        &mut self,
        _limits: &cargo::Limits,
        inventory: Option<(&Path, resolution::bundle::Inventory<'_>)>,
    ) -> Result<()> {
        if let Some(search) = &self.search {
            let graph = self
                .graph_budget
                .as_ref()
                .ok_or_else(|| extraction("compiler-namespace", "original graph absent"))?;
            search.revalidate(&mut self.invocations.allocation, graph)?;
        }
        if let Some((root, inventory)) = inventory {
            let graph = self
                .graph_budget
                .as_ref()
                .ok_or_else(|| extraction("resolution-inventory", "original graph absent"))?;
            inventory.complete(
                root,
                self.namespace.as_deref(),
                &mut self.invocations.allocation,
                graph,
                process::CAPTURE_BYTES,
            )?;
        } else if let Some(namespace) = &self.namespace {
            let graph = self
                .graph_budget
                .as_ref()
                .ok_or_else(|| extraction("compiler-namespace", "original graph absent"))?;
            namespace.revalidate(
                &mut self.invocations.allocation,
                graph,
                process::CAPTURE_BYTES,
            )?;
        }
        for (path, expected) in &self.controls {
            let bytes = read_bounded_accounted(
                path,
                &mut self.invocations.allocation,
                self.graph_budget.as_ref(),
            )?;
            let actual: [u8; 32] = Sha256::digest(bytes).into();
            if &actual != expected {
                return Err(extraction(
                    "metadata-probe-control-drift",
                    &path.display().to_string(),
                ));
            }
        }
        for (path, expected) in &self.sidecars {
            let bytes = if path.file_name() == Some(std::ffi::OsStr::new(ATTEMPTS_FILE)) {
                read_attempts(
                    path,
                    &mut self.invocations.allocation,
                    self.graph_budget.as_ref(),
                )?
            } else {
                read_bounded_accounted(
                    path,
                    &mut self.invocations.allocation,
                    self.graph_budget.as_ref(),
                )?
            };
            let actual: [u8; 32] = Sha256::digest(bytes).into();
            if &actual != expected {
                return Err(extraction(
                    "compiler-sidecar-drift",
                    &path.display().to_string(),
                ));
            }
        }
        for (path, expected_length, expected_digest) in &self.artifacts {
            let (length, digest) = artifact_digest(
                path,
                process::CAPTURE_BYTES,
                &mut self.invocations.allocation,
                self.graph_budget.as_ref(),
            )?;
            if length != *expected_length || digest != *expected_digest {
                return Err(extraction(
                    "selected-artifact-drift",
                    &path.display().to_string(),
                ));
            }
        }
        Ok(())
    }

    pub(crate) fn into_parts(
        mut self,
        limits: &cargo::Limits,
    ) -> Result<(protocol::Document, Invocations)> {
        self.invocations.allocation.exceeded(
            "compiler sidecar document bytes",
            limits.json_bytes,
            self.document_bytes,
        );
        if self.deferred {
            self.document = self.decode_sidecars(limits)?;
        }
        self.invocations.allocation.moved_document();
        Ok((self.document, self.invocations))
    }

    /// Each compiler sidecar, in plan order, as the semantic projection of its
    /// handshake and the SHA-256 of its body: what the API is a
    /// function of, without the run's nonce, argument digest, artifact paths,
    /// bytes or SVH. `None` when a sidecar changed since capture or is not a
    /// handshake followed by a body.
    pub(crate) fn semantic_sidecars(&self) -> Option<Vec<(Vec<u8>, [u8; 32])>> {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Shell {
            handshake: protocol::Handshake,
        }
        const BODY: &[u8] = b",\"reachable\":";
        let mut sidecars = Vec::new();
        for (path, expected) in &self.sidecars {
            if path.file_name() == Some(std::ffi::OsStr::new(ATTEMPTS_FILE)) {
                continue;
            }
            let bytes = fs::read(path).ok()?;
            if <[u8; 32]>::from(Sha256::digest(&bytes)) != *expected {
                return None;
            }
            // A JSON string cannot hold an unescaped quote, so the first
            // `,"reachable":` closes the handshake the driver writes first.
            let start = bytes
                .windows(BODY.len())
                .position(|window| window == BODY)?;
            let mut shell = bytes[..start].to_vec();
            shell.push(b'}');
            let handshake = serde_json::from_slice::<Shell>(&shell).ok()?.handshake;
            let projection = serde_json::to_vec(&semantic_handshake(&handshake)).ok()?;
            sidecars.push((projection, Sha256::digest(&bytes[start..]).into()));
        }
        Some(sidecars)
    }

    /// Re-read the sidecars in plan order, require their captured digests and
    /// decode the complete document once a consumer needs it.
    fn decode_sidecars(&mut self, limits: &cargo::Limits) -> Result<protocol::Document> {
        let allocation = &mut self.invocations.allocation;
        let mut document = Vec::new();
        append_document_prefix(&mut document, limits, allocation)?;
        let sidecars = self
            .sidecars
            .iter()
            .filter(|(path, _)| path.file_name() != Some(std::ffi::OsStr::new(ATTEMPTS_FILE)));
        for (index, (path, expected)) in sidecars.enumerate() {
            if index != 0 {
                append(&mut document, b",", limits, allocation)?;
            }
            let bytes = read_bounded_accounted(path, allocation, self.graph_budget.as_ref())?;
            if <[u8; 32]>::from(Sha256::digest(&bytes)) != *expected {
                return Err(extraction(
                    "compiler-sidecar-drift",
                    &path.display().to_string(),
                ));
            }
            append(&mut document, &bytes, limits, allocation)?;
        }
        append(&mut document, b"]}", limits, allocation)?;
        protocol::decode_accounted(&document, limits, allocation)
    }
}

fn prepare_entries(
    units: &[unit::Extraction],
    run_nonce: &str,
    limits: &cargo::Limits,
    allocation: &mut Allocation,
) -> Result<Vec<Entry<Prepared>>> {
    let mut entries = Vec::new();
    allocation.reserve(&mut entries, units.len())?;
    for unit in units {
        entries.push(entry(unit, run_nonce, limits, allocation)?);
    }
    entries.sort_unstable();
    if entries.windows(2).any(|pair| pair[0] == pair[1]) {
        return Err(extraction(
            "ambiguous-package-unit-correlation",
            "the initial Build graph produced duplicate extraction plans",
        ));
    }
    let mut correlations = BTreeSet::new();
    let mut nonces = BTreeSet::new();
    for entry in &entries {
        allocation.items(2)?;
        if !correlations.insert(entry.unit_correlation.as_ref())
            || !nonces.insert(entry.invocation_nonce.as_ref())
        {
            return Err(extraction(
                "duplicate-unit-correlation",
                entry.unit_correlation.as_ref(),
            ));
        }
    }
    Ok(entries)
}

fn entry(
    unit: &unit::Extraction,
    run_nonce: &str,
    limits: &cargo::Limits,
    allocation: &mut Allocation,
) -> Result<Entry<Prepared>> {
    allocation.items(1)?;
    let mut semantic = Vec::new();
    for value in [
        unit.package_id(),
        unit.target(),
        unit.target_kinds().first().map_or("", AsRef::as_ref),
        unit.crate_name(),
        unit.edition(),
        unit.semantic_key(),
    ] {
        frame(&mut semantic, value.as_bytes(), allocation)?;
    }
    let correlation = allocation.hex(Sha256::digest(&semantic))?;
    allocation.items(1)?;
    let mut invocation = Vec::new();
    frame(&mut invocation, run_nonce.as_bytes(), allocation)?;
    frame(&mut invocation, correlation.as_bytes(), allocation)?;
    let invocation_nonce = allocation.hex(Sha256::digest(&invocation))?;
    for value in [
        unit.package_id(),
        unit.package_name(),
        unit.package_version(),
        unit.package_source(),
        unit.target(),
        unit.target_kinds().first().map_or("", AsRef::as_ref),
        unit.crate_name(),
        unit.edition(),
        unit.semantic_key(),
    ] {
        allocation.exceeded(
            "metadata probe plan string",
            limits.string_bytes,
            value.len(),
        );
    }
    let mut dependency_keys = Vec::new();
    allocation.reserve(&mut dependency_keys, unit.dependencies().len())?;
    for dependency in unit.dependencies() {
        dependency_keys.push(allocation.owned(dependency.semantic_key())?);
    }
    dependency_keys.sort_unstable();
    dependency_keys.dedup();
    let target_kind = unit
        .target_kinds()
        .first()
        .ok_or_else(|| extraction("missing-target-kind", unit.semantic_key()))?;
    let target_kind = allocation.owned(target_kind)?;
    Ok(Entry {
        package: Package {
            cargo_id: allocation.owned(unit.package_id())?,
            name: allocation.owned(unit.package_name())?,
            version: allocation.owned(unit.package_version())?,
            source: allocation.owned(unit.package_source())?,
        },
        root: Root::Selected {
            aliases: clone_strings(unit.aliases(), allocation)?,
        },
        unit_key: allocation.owned(unit.semantic_key())?,
        dependencies: dependency_keys.into_boxed_slice(),
        target: allocation.owned(unit.target())?,
        target_kind,
        target_kinds: clone_strings(unit.target_kinds(), allocation)?,
        crate_types: clone_strings(unit.crate_types(), allocation)?,
        crate_name: allocation.owned(unit.crate_name())?,
        edition: allocation.owned(unit.edition())?,
        profile: allocation.owned(unit.profile())?,
        features: clone_strings(unit.features(), allocation)?,
        invocation_nonce,
        unit_correlation: correlation,
        state: Prepared,
    })
}

fn clone_strings(values: &[Box<str>], allocation: &mut Allocation) -> Result<Box<[Box<str>]>> {
    let mut cloned = Vec::new();
    allocation.reserve(&mut cloned, values.len())?;
    for value in values {
        cloned.push(allocation.owned(value)?);
    }
    Ok(cloned.into_boxed_slice())
}

impl Entry<Prepared> {
    fn seal(self, artifact: Artifact) -> Entry<Sealed> {
        Entry {
            package: self.package,
            root: self.root,
            unit_key: self.unit_key,
            dependencies: self.dependencies,
            target: self.target,
            target_kind: self.target_kind,
            target_kinds: self.target_kinds,
            crate_types: self.crate_types,
            crate_name: self.crate_name,
            edition: self.edition,
            profile: self.profile,
            features: self.features,
            invocation_nonce: self.invocation_nonce,
            unit_correlation: self.unit_correlation,
            state: Sealed { artifact },
        }
    }
}

fn validate_fragments(
    fragments: &[protocol::Fragment],
    entries: &[Entry<Sealed>],
    compiler_commit: &str,
    host: &str,
    selected_target: &str,
    limits: &cargo::Limits,
    allocation: &mut Allocation,
) -> Result<()> {
    allocation.exceeded("extraction fragments", limits.packages, fragments.len());
    if fragments.len() != entries.len() {
        return Err(extraction(
            "missing-unit-correlation",
            "the driver did not publish exactly one fragment per planned unit",
        ));
    }
    allocation.items(entries.len().saturating_add(fragments.len()))?;
    let mut expected = BTreeMap::new();
    for entry in entries {
        expected.insert(entry.unit_correlation.as_ref(), entry);
    }
    let mut observed = BTreeSet::new();
    let arguments_digest = fragments
        .first()
        .map(|fragment| fragment.handshake.probe_arguments_digest.as_str());
    for fragment in fragments {
        let handshake = &fragment.handshake;
        if Some(handshake.probe_arguments_digest.as_str()) != arguments_digest {
            return Err(extraction(
                "split-probe-invocation",
                "compiler fragments report different probe argument digests",
            ));
        }
        let Some(entry) = expected.get(handshake.unit_correlation.as_str()) else {
            return Err(extraction("extra-fragment", &handshake.unit_correlation));
        };
        let artifact = &entry.state.artifact;
        if !observed.insert(handshake.unit_correlation.as_str()) {
            return Err(extraction(
                "duplicate-unit-correlation",
                &handshake.unit_correlation,
            ));
        }
        let (Root::Selected { aliases: left }, protocol::Root::Selected { aliases: right }) =
            (&entry.root, &handshake.root);
        let root_matches = left
            .iter()
            .map(AsRef::as_ref)
            .eq(right.iter().map(String::as_str));
        if handshake.package.cargo_id != entry.package.cargo_id.as_ref()
            || handshake.package.name != entry.package.name.as_ref()
            || handshake.package.version != entry.package.version.as_ref()
            || handshake.package.source != entry.package.source.as_ref()
            || !root_matches
            || handshake.target != entry.target.as_ref()
            || handshake.target_kind != entry.target_kind.as_ref()
            || !handshake
                .target_kinds
                .iter()
                .map(String::as_str)
                .eq(entry.target_kinds.iter().map(AsRef::as_ref))
            || !handshake
                .crate_types
                .iter()
                .map(String::as_str)
                .eq(entry.crate_types.iter().map(AsRef::as_ref))
            || handshake.crate_name != entry.crate_name.as_ref()
            || handshake.edition != entry.edition.as_ref()
            || handshake.profile != entry.profile.as_ref()
            || !matches!(handshake.compilation, protocol::Compilation::Target)
            || !handshake
                .features
                .iter()
                .map(String::as_str)
                .eq(entry.features.iter().map(AsRef::as_ref))
            || handshake.invocation_nonce != entry.invocation_nonce.as_ref()
            || handshake.compiler_commit != compiler_commit
            || handshake.host != host
            || handshake.selected_target != selected_target
            || handshake.unit_key != entry.unit_key.as_ref()
            || handshake.artifact.path != artifact.path.as_ref()
            || handshake.artifact.length != artifact.length
            || handshake.artifact.digest != artifact.digest.as_ref()
            || handshake.artifact.profile != artifact.profile.as_ref()
            || handshake.artifact.fresh != artifact.fresh
            || handshake.artifact.probe_alias.is_empty()
            || handshake.artifact.crate_hash.is_empty()
            || !handshake
                .dependencies
                .iter()
                .map(String::as_str)
                .eq(entry.dependencies.iter().map(AsRef::as_ref))
        {
            return Err(extraction(
                "probe-plan-mismatch",
                &handshake.unit_correlation,
            ));
        }
        exact_hex(
            "probe argument digest",
            &handshake.probe_arguments_digest,
            64,
        )?;
        exact_hex(
            "selected StableCrateId",
            &handshake.artifact.stable_crate_id,
            16,
        )?;
    }
    Ok(())
}

fn run_nonce(
    graph: cargo::Digest,
    compiler_commit: &str,
    target: &str,
    allocation: &mut Allocation,
) -> Result<Box<str>> {
    allocation.items(1)?;
    let mut bytes = Vec::new();
    frame(&mut bytes, b"rawr.extract.run-nonce/v2", allocation)?;
    frame(&mut bytes, graph.as_bytes(), allocation)?;
    frame(&mut bytes, compiler_commit.as_bytes(), allocation)?;
    frame(&mut bytes, target.as_bytes(), allocation)?;
    allocation.hex(Sha256::digest(&bytes))
}

fn create_directory(path: &Path, field: &'static str) -> Result<()> {
    fs::create_dir_all(path).map_err(|source| Error::Io {
        operation: "create extraction directory",
        path: path.to_path_buf(),
        source,
    })?;
    let metadata = fs::symlink_metadata(path).map_err(|source| Error::Io {
        operation: "inspect extraction directory",
        path: path.to_path_buf(),
        source,
    })?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err(Error::Path {
            field,
            path: path.to_path_buf(),
            reason: "must be a real directory".into(),
        });
    }
    Ok(())
}

fn fresh_directory(path: &Path, field: &'static str) -> Result<()> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_dir() && !metadata.file_type().is_symlink() => {
            fs::remove_dir_all(path).map_err(|source| Error::Io {
                operation: "clear authoritative Build target directory",
                path: path.to_path_buf(),
                source,
            })?;
        }
        Ok(_) => {
            return Err(Error::Path {
                field,
                path: path.to_path_buf(),
                reason: "must be absent or a real directory".into(),
            });
        }
        Err(source) if source.kind() == io::ErrorKind::NotFound => {}
        Err(source) => {
            return Err(Error::Io {
                operation: "inspect authoritative Build target directory",
                path: path.to_path_buf(),
                source,
            });
        }
    }
    create_directory(path, field)
}

fn join_path(parent: &Path, child: &Path, allocation: &mut Allocation) -> Result<PathBuf> {
    let maximum = parent
        .as_os_str()
        .len()
        .saturating_add(1)
        .saturating_add(child.as_os_str().len());
    allocation.path_capacity(maximum)?;
    let path = parent.join(child);
    allocation.validate_path(&path, maximum)?;
    Ok(path)
}

fn canonical_under(
    parent: &Path,
    path: &Path,
    field: &'static str,
    allocation: &mut Allocation,
) -> Result<PathBuf> {
    canonical_under_either(parent, parent, path, field, allocation)
}

fn canonical_under_either(
    parent: &Path,
    other_parent: &Path,
    path: &Path,
    field: &'static str,
    allocation: &mut Allocation,
) -> Result<PathBuf> {
    let same_parent = parent == other_parent;
    let parent = canonical_path(parent, "canonicalize extraction parent", allocation)?;
    let other = if same_parent {
        None
    } else {
        Some(canonical_path(
            other_parent,
            "canonicalize extraction parent",
            allocation,
        )?)
    };
    let canonical = canonical_path(path, "canonicalize extraction path", allocation)?;
    if !canonical.starts_with(&parent)
        && !other
            .as_ref()
            .is_some_and(|root| canonical.starts_with(root))
    {
        return Err(Error::Path {
            field,
            path: canonical,
            reason: "escaped its sealed parent".into(),
        });
    }
    Ok(canonical)
}

fn canonical_path(
    path: &Path,
    operation: &'static str,
    allocation: &mut Allocation,
) -> Result<PathBuf> {
    allocation.path_capacity(CANONICAL_PATH_BYTES)?;
    let canonical = fs::canonicalize(path).map_err(|source| Error::Io {
        operation,
        path: path.to_path_buf(),
        source,
    })?;
    allocation.validate_path(&canonical, CANONICAL_PATH_BYTES)?;
    Ok(canonical)
}

fn canonical_directory_under(
    parent: &Path,
    path: &Path,
    field: &'static str,
    allocation: &mut Allocation,
) -> Result<PathBuf> {
    let canonical = canonical_under(parent, path, field, allocation)?;
    if canonical.as_os_str() != path.as_os_str() {
        return Err(Error::Path {
            field,
            path: canonical,
            reason: "must retain its exact reserved path".into(),
        });
    }
    let metadata = match fs::symlink_metadata(&canonical) {
        Ok(metadata) => metadata,
        Err(source) => {
            return Err(Error::Io {
                operation: "inspect reserved extraction directory",
                path: canonical,
                source,
            });
        }
    };
    if !metadata.is_dir() {
        return Err(Error::Path {
            field,
            path: canonical,
            reason: "must be a precreated directory".into(),
        });
    }
    Ok(canonical)
}

fn path_string<'a>(path: &'a Path, field: &'static str) -> Result<&'a str> {
    path.to_str().ok_or_else(|| Error::Path {
        field,
        path: path.to_path_buf(),
        reason: "must be UTF-8".into(),
    })
}

fn directory_entry_path(item: &fs::DirEntry, allocation: &mut Allocation) -> Result<PathBuf> {
    allocation.path_capacity(CANONICAL_PATH_BYTES)?;
    let path = item.path();
    allocation.validate_path(&path, CANONICAL_PATH_BYTES)?;
    Ok(path)
}

fn temporary_path(path: &Path, allocation: &mut Allocation) -> Result<PathBuf> {
    let process = std::process::id();
    let digits = if process == 0 {
        1
    } else {
        process.ilog10() as usize + 1
    };
    let length = "tmp-".len().saturating_add(digits);
    allocation.items(1)?;
    allocation.bytes(length)?;
    let mut extension = String::new();
    extension
        .try_reserve_exact(length)
        .map_err(|error| Error::Allocation {
            operation: "temporary path allocation",
            source: error,
        })?;
    write!(&mut extension, "tmp-{process}")
        .map_err(|error| extraction("temporary-path-render", &error.to_string()))?;
    let maximum = path
        .as_os_str()
        .len()
        .saturating_add(1)
        .saturating_add(extension.len());
    allocation.path_capacity(maximum)?;
    let temporary = path.with_extension(extension);
    allocation.validate_path(&temporary, maximum)?;
    Ok(temporary)
}

fn publish(path: &Path, bytes: &[u8], allocation: &mut Allocation) -> Result<()> {
    let temporary = temporary_path(path, allocation)?;
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)
        .map_err(|source| Error::Io {
            operation: "create metadata probe plan temporary",
            path: temporary.clone(),
            source,
        })?;
    file.write_all(bytes).map_err(|source| Error::Io {
        operation: "write metadata probe plan temporary",
        path: temporary.clone(),
        source,
    })?;
    file.sync_all().map_err(|source| Error::Io {
        operation: "sync metadata probe plan temporary",
        path: temporary.clone(),
        source,
    })?;
    fs::rename(&temporary, path).map_err(|source| Error::Io {
        operation: "publish metadata probe plan",
        path: path.to_path_buf(),
        source,
    })
}

/// Hash graph-owned artifacts with the existing bounded physical-file reader.
/// The legacy extraction path retains its original allocation contract.
fn artifact_digest(
    path: &Path,
    limit: usize,
    allocation: &mut Allocation,
    graph: Option<&crate::node::GraphBudget>,
) -> Result<(u64, [u8; 32])> {
    let Some(graph) = graph else {
        let bytes = read_bounded_accounted(path, allocation, None)?;
        return Ok((bytes.len() as u64, Sha256::digest(bytes).into()));
    };
    graph.paths(1).map_err(artifact_graph_error)?;
    crate::store::path::input(path, graph).map_err(artifact_graph_error)?;
    let metadata = fs::symlink_metadata(path).map_err(|source| Error::Io {
        operation: "inspect compiler artifact",
        path: path.to_path_buf(),
        source,
    })?;
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return Err(extraction(
            "compiler-artifact-type",
            "expected a regular artifact",
        ));
    }
    let parent = path.parent().unwrap_or(path);
    if crate::runner::artifact::retained::directory(parent, graph).map_err(artifact_graph_error)?
        != parent
    {
        return Err(artifact_graph_error(crate::node::Error::OutsideRoot(
            path.to_path_buf(),
        )));
    }
    let digest = crate::runner::artifact::retained::hash(path, &metadata, limit, graph)
        .map_err(artifact_graph_error)?;
    Ok((metadata.len(), digest))
}

fn artifact_graph_error(source: crate::node::Error) -> Error {
    Error::Node {
        operation: "compiler artifact validation",
        source: Box::new(source),
    }
}

fn read_bounded_accounted(
    path: &Path,
    allocation: &mut Allocation,
    graph: Option<&crate::node::GraphBudget>,
) -> Result<Box<[u8]>> {
    let result = read_bounded_with_graph(path, Some(&mut *allocation), graph);
    result.map_err(|error| allocation.failure(error))
}

fn admit_protocol_name(length: usize, graph: Option<&crate::node::GraphBudget>) -> Result<()> {
    if let Some(graph) = graph {
        // Conservatively admit one entire pinned B-tree node for each insertion,
        // including key slots and edge/header storage, before the owned name.
        graph
            .decode(
                length + std::mem::size_of::<[Box<str>; 12]>() + std::mem::size_of::<[usize; 14]>(),
            )
            .map_err(attempt_graph_error)?;
    }
    Ok(())
}

fn attempt_graph_error(source: crate::node::Error) -> Error {
    Error::Node {
        operation: "compiler attempt completion",
        source: Box::new(source),
    }
}

fn read_attempts(
    path: &Path,
    allocation: &mut Allocation,
    graph: Option<&crate::node::GraphBudget>,
) -> Result<Box<[u8]>> {
    read_bounded_accounted(path, allocation, graph)
}

fn read_bounded_with_graph(
    path: &Path,
    allocation: Option<&mut Allocation>,
    graph: Option<&crate::node::GraphBudget>,
) -> Result<Box<[u8]>> {
    if let Some(graph) = graph {
        graph.paths(1).map_err(attempt_graph_error)?;
        crate::store::path::input(path, graph).map_err(attempt_graph_error)?;
    }
    let metadata = fs::symlink_metadata(path).map_err(|source| Error::Io {
        operation: "inspect compiler sidecar",
        path: path.to_path_buf(),
        source,
    })?;
    let length = usize::try_from(metadata.len()).unwrap_or(usize::MAX);
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return Err(Error::Path {
            field: "compiler sidecar",
            path: path.to_path_buf(),
            reason: "must be a regular non-symlink file".into(),
        });
    }
    if let Some(allocation) = allocation {
        allocation.items(1)?;
        allocation.bytes(length)?;
    }
    if let Some(graph) = graph {
        graph.decode(length).map_err(attempt_graph_error)?;
        graph.hash(length).map_err(attempt_graph_error)?;
        graph.paths(1).map_err(attempt_graph_error)?;
        crate::store::path::input(path, graph).map_err(attempt_graph_error)?;
    }
    let mut bytes = Vec::new();
    bytes
        .try_reserve_exact(length)
        .map_err(|error| Error::Allocation {
            operation: "sidecar allocation",
            source: error,
        })?;
    let mut file = fs::File::open(path).map_err(|source| Error::Io {
        operation: "open compiler sidecar",
        path: path.to_path_buf(),
        source,
    })?;
    bytes.resize(length, 0);
    if let Err(source) = file.read_exact(&mut bytes) {
        if source.kind() == std::io::ErrorKind::UnexpectedEof {
            return Err(extraction(
                "compiler-sidecar-read-drift",
                &path.display().to_string(),
            ));
        }
        return Err(Error::Io {
            operation: "read compiler sidecar",
            path: path.to_path_buf(),
            source,
        });
    }
    let mut extra = [0_u8; 1];
    let extra = file.read(&mut extra).map_err(|source| Error::Io {
        operation: "probe compiler sidecar growth",
        path: path.to_path_buf(),
        source,
    })?;
    if extra != 0 {
        return Err(extraction(
            "compiler-sidecar-read-drift",
            &path.display().to_string(),
        ));
    }
    Ok(bytes.into_boxed_slice())
}

/// Join one fragment into the sidecar document. The configurable `json_bytes`
/// target is recorded on the account.
fn append(
    bytes: &mut Vec<u8>,
    value: &[u8],
    limits: &cargo::Limits,
    allocation: &mut Allocation,
) -> Result<()> {
    let Some(actual) = bytes.len().checked_add(value.len()) else {
        return Err(allocation.failure(extraction(
            "sidecar-document-overflow",
            "the sidecar document length overflowed",
        )));
    };
    allocation.exceeded("compiler sidecar document bytes", limits.json_bytes, actual);
    allocation.bytes(value.len())?;
    bytes
        .try_reserve_exact(value.len())
        .map_err(|error| Error::Allocation {
            operation: "sidecar allocation",
            source: error,
        })?;
    bytes.extend_from_slice(value);
    Ok(())
}

fn append_document_prefix(
    bytes: &mut Vec<u8>,
    limits: &cargo::Limits,
    allocation: &mut Allocation,
) -> Result<()> {
    append(bytes, b"{\"schema\":", limits, allocation)?;
    let mut decimal = [0_u8; 10];
    let mut cursor = decimal.len();
    let mut schema = protocol::SCHEMA;
    loop {
        cursor = cursor.saturating_sub(1);
        let digit = u8::try_from(schema % 10)
            .map_err(|_| extraction("sidecar-schema-render", "decimal digit exceeded u8"))?;
        decimal[cursor] = b'0' + digit;
        schema /= 10;
        if schema == 0 {
            break;
        }
    }
    append(bytes, &decimal[cursor..], limits, allocation)?;
    append(bytes, b",\"fragments\":[", limits, allocation)
}

fn frame(bytes: &mut Vec<u8>, value: &[u8], allocation: &mut Allocation) -> Result<()> {
    let length = u64::try_from(value.len()).map_err(|_| Error::Extraction {
        code: "identity-frame",
        reason: "identity component exceeded u64".into(),
    })?;
    let additional = 8_usize.saturating_add(value.len());
    allocation.bytes(additional)?;
    bytes
        .try_reserve_exact(additional)
        .map_err(|error| Error::Allocation {
            operation: "identity frame allocation",
            source: error,
        })?;
    bytes.extend_from_slice(&length.to_le_bytes());
    bytes.extend_from_slice(value);
    Ok(())
}

fn exact_hex(field: &'static str, value: &str, length: usize) -> Result<()> {
    if value.len() == length && value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        Ok(())
    } else {
        Err(extraction("invalid-probe-handshake", field))
    }
}

fn limited(resource: &'static str, limit: usize, actual: usize) -> Result<()> {
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

fn extraction(code: &'static str, reason: &str) -> Error {
    Error::Extraction {
        code,
        reason: reason.into(),
    }
}

#[cfg(test)]
mod preparation;

#[cfg(test)]
mod validation;

/// Every handshake field but the run's nonce and argument digest, the unit
/// correlation, and the artifact's path, bytes, freshness and SVH.
/// Loaded and external crates keep their name, `StableCrateId` and
/// artifact count, not their paths.
fn semantic_handshake(handshake: &protocol::Handshake) -> serde_json::Value {
    let crates = |crates: &[protocol::ExternalCrate]| {
        crates
            .iter()
            .map(|item| {
                serde_json::json!([item.crate_name, item.stable_crate_id, item.artifacts.len()])
            })
            .collect::<Vec<_>>()
    };
    let package = &handshake.package;
    let artifact = &handshake.artifact;
    serde_json::json!({
        "schema": handshake.schema,
        "package": [package.cargo_id, package.name, package.version, package.source],
        "root": format!("{:?}", handshake.root),
        "target": handshake.target,
        "target_kind": handshake.target_kind,
        "target_kinds": handshake.target_kinds,
        "crate_types": handshake.crate_types,
        "crate_name": handshake.crate_name,
        "edition": handshake.edition,
        "profile": handshake.profile,
        "compilation": format!("{:?}", handshake.compilation),
        "host": handshake.host,
        "selected_target": handshake.selected_target,
        "features": handshake.features,
        "compiler_commit": handshake.compiler_commit,
        "unit_key": handshake.unit_key,
        "dependencies": handshake.dependencies,
        "artifact": [artifact.profile, artifact.stable_crate_id, artifact.probe_alias],
        "sysroot": handshake.sysroot.iter().map(|sysroot| serde_json::json!([
            sysroot.compiler_commit,
            sysroot.selected_target,
            sysroot.crate_name,
            sysroot.stable_crate_id,
        ])).collect::<Vec<_>>(),
        "loaded_crates": crates(&handshake.loaded_crates),
        "external_crates": crates(&handshake.external_crates),
    })
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::fs;
    use std::path::Path;

    use sha2::{Digest as _, Sha256};

    use super::{
        append, append_document_prefix, join_path, run_nonce, temporary_path, Capture, ExternalJoin,
    };
    use crate::cargo::{self, Limits};
    use crate::extract::{protocol, Allocation, InvocationAuthority, Invocations};
    use crate::input::{ArtifactAuthority, ArtifactJoin};
    use crate::Error;

    #[test]
    fn graph_artifact_hash_streams_and_keeps_original_refusals() {
        let root =
            std::env::temp_dir().join(format!("rawr-stream-artifact-{}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        let root = fs::canonicalize(root).unwrap();
        let path = root.join("libfixture.rmeta");
        let bytes = vec![93_u8; 2 * 64 * 1024 + 17];
        fs::write(&path, &bytes).unwrap();
        let graph = crate::node::GraphBudget::new();
        // Artifact bytes are streamed, not owned by the extraction decoder.
        let mut allocation = Allocation::with_limits(0, 0);
        let observed =
            super::artifact_digest(&path, bytes.len(), &mut allocation, Some(&graph)).unwrap();
        assert_eq!(
            observed,
            (bytes.len() as u64, Sha256::digest(&bytes).into())
        );
        // Streamed artifact bytes charge nothing on the canonical
        // account, so even a zero-target account records no axis.
        assert!(!allocation.exceedances().is_exceeded());
        assert_eq!(allocation.usage(), (0, 0));
        // The `limit` argument is the physical file-read bound: it refuses.
        assert!(
            super::artifact_digest(&path, bytes.len() - 1, &mut allocation, Some(&graph)).is_err()
        );
        // An unchanged artifact reuses its digest without a read; a new
        // status-change time gives it a new identity that is read again.
        let reused = crate::node::GraphBudget::new();
        assert_eq!(
            super::artifact_digest(&path, bytes.len(), &mut allocation, Some(&reused)).unwrap(),
            observed
        );
        assert_eq!(reused.census_usage().unwrap().2, 0);
        fs::set_permissions(&path, fs::metadata(&path).unwrap().permissions()).unwrap();
        // A graph at its hashed target still streams the artifact and
        // records the axis at exactly target + content bytes + the EOF probe.
        let spent = crate::node::GraphBudget::new();
        spent
            .hash(crate::node::MAX_HASHED_BYTES_PER_GRAPH as usize)
            .unwrap();
        assert_eq!(
            super::artifact_digest(&path, bytes.len(), &mut allocation, Some(&spent)).unwrap(),
            observed
        );
        let axis = spent
            .exceedances()
            .axis(crate::node::HASHED_RESOURCE)
            .copied()
            .unwrap();
        assert_eq!(axis.target, crate::node::MAX_HASHED_BYTES_PER_GRAPH);
        assert_eq!(
            axis.high_water,
            crate::node::MAX_HASHED_BYTES_PER_GRAPH + bytes.len() as u64 + 1
        );
        let linked = root.join("linked.rmeta");
        fs::hard_link(&path, &linked).unwrap();
        assert!(super::artifact_digest(&path, bytes.len(), &mut allocation, Some(&graph)).is_err());
        fs::remove_file(&linked).unwrap();
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(&path, &linked).unwrap();
            assert!(
                super::artifact_digest(&linked, bytes.len(), &mut allocation, Some(&graph))
                    .is_err()
            );
        }
        fs::remove_dir_all(&root).unwrap();
    }

    fn authority(key: &str, package: &str) -> ArtifactAuthority {
        ArtifactAuthority {
            semantic_key: key.into(),
            package_id: package.into(),
            target: "/target/libdep.rmeta".into(),
            target_kinds: Box::from([Box::<str>::from("lib")]),
            crate_types: Box::from([Box::<str>::from("rlib")]),
            crate_name: "dep".into(),
            features: Box::default(),
        }
    }

    fn code(error: Error) -> &'static str {
        match error {
            Error::Extraction { code, .. } => code,
            other => panic!("expected extraction error, got {other:?}"),
        }
    }

    #[test]
    fn ambiguous_and_split_external_authority_are_fatal() {
        let ambiguous = ExternalJoin::default()
            .observe(ArtifactJoin::Ambiguous, "/target/libdep.rmeta")
            .expect_err("an ambiguous Cargo artifact cannot become path-local");
        assert_eq!(code(ambiguous), "ambiguous-external-artifact-join");

        let mut split = ExternalJoin::default();
        split
            .observe(
                ArtifactJoin::Available(authority("unit-a", "dep 1.0.0")),
                "/target/libdep.rmeta",
            )
            .expect("the first exact Cargo authority is admissible");
        let split = split
            .observe(
                ArtifactJoin::Available(authority("unit-b", "dep 2.0.0")),
                "/target/libdep.rlib",
            )
            .expect_err("two Cargo authorities cannot become path-local");
        assert_eq!(code(split), "split-external-crate-authority");
    }

    #[test]
    fn missing_is_only_local_when_every_artifact_has_zero_join() {
        let mut missing = ExternalJoin::default();
        missing
            .observe(ArtifactJoin::Missing, "/target/libdep.rmeta")
            .expect("a pure zero join remains path-local evidence");
        assert!(missing.authority.is_none());

        let partial = missing
            .observe(
                ArtifactJoin::Available(authority("unit-a", "dep 1.0.0")),
                "/target/libdep.rlib",
            )
            .expect_err("a split missing/available join is fatal");
        assert_eq!(code(partial), "partial-external-crate-authority");
    }

    #[test]
    fn fixed_control_paths_and_temporary_path_have_exact_cumulative_boundaries() {
        fn paths(allocation: &mut Allocation) -> crate::Result<()> {
            let root = join_path(Path::new("/cache"), Path::new("extraction"), allocation)?;
            let active = join_path(&root, Path::new("active"), allocation)?;
            let plan = join_path(&active, Path::new("plan.json"), allocation)?;
            let _protocol = join_path(&active, Path::new("protocol"), allocation)?;
            let _probe = join_path(&active, Path::new("probe.rs"), allocation)?;
            let _temporary = temporary_path(&plan, allocation)?;
            Ok(())
        }

        let mut measured = Allocation::with_limits(usize::MAX, usize::MAX);
        paths(&mut measured).expect("the fixed path fanout is measurable");
        let (items, bytes) = measured.usage();
        let mut exact = Allocation::with_limits(items, bytes);
        paths(&mut exact).expect("the exact fixed path boundary accepts");
        assert_eq!(exact.usage(), (items, bytes));
        assert!(!exact.exceedances().is_exceeded());
        // One short of the cumulative charge still completes the fixed
        // path fanout and records the crossed axis at its truthful high water.
        let mut item_short = Allocation::with_limits(items - 1, usize::MAX);
        paths(&mut item_short).expect("an item target records instead of refusing");
        assert_eq!(item_short.usage(), (items, bytes));
        assert!(!item_short.is_refused());
        let axis = *item_short
            .exceedances()
            .axis("canonical allocation items")
            .expect("the item axis records");
        assert_eq!(
            (axis.target, axis.first_requested, axis.high_water),
            ((items - 1) as u64, items as u64, items as u64)
        );
        assert!(item_short
            .exceedances()
            .axis("canonical allocation bytes")
            .is_none());
        let mut byte_short = Allocation::with_limits(usize::MAX, bytes - 1);
        paths(&mut byte_short).expect("a byte target records instead of refusing");
        assert_eq!(byte_short.usage(), (items, bytes));
        assert!(!byte_short.is_refused());
        let axis = *byte_short
            .exceedances()
            .axis("canonical allocation bytes")
            .expect("the byte axis records");
        assert_eq!(
            (axis.target, axis.first_requested, axis.high_water),
            ((bytes - 1) as u64, bytes as u64, bytes as u64)
        );
        assert!(byte_short
            .exceedances()
            .axis("canonical allocation items")
            .is_none());
        // Terminal only through a genuine cause: a latched account owns no path.
        assert!(matches!(
            paths(&mut Allocation::refused()),
            Err(Error::Node { operation, .. })
                if operation == crate::extract::REFUSED_ACCOUNT_OPERATION
        ));
    }

    #[test]
    fn attempt_reads_preserve_original_accounts_at_exact_and_short_floors() {
        let path =
            std::env::temp_dir().join(format!("rawr-attempt-account-{}", std::process::id()));
        fs::write(&path, b"attempt").unwrap();
        let length = 7;
        let mut allocation = Allocation::with_limits(2, length + 3);
        allocation.items(1).unwrap();
        allocation.bytes(3).unwrap();
        let graph = crate::node::GraphBudget::new();
        graph
            .hash(crate::node::MAX_HASHED_BYTES_PER_GRAPH as usize - length)
            .unwrap();
        let bytes = super::read_attempts(&path, &mut allocation, Some(&graph)).unwrap();
        assert_eq!(&*bytes, b"attempt");
        // The exact floor fills the hashed target with no exceedance;
        // one more byte records the axis at exactly target + 1.
        assert!(!graph.exceedances().is_exceeded());
        assert_eq!(
            graph.census_usage().unwrap().2,
            crate::node::MAX_HASHED_BYTES_PER_GRAPH
        );
        graph.hash(1).unwrap();
        assert_eq!(
            graph
                .exceedances()
                .axis(crate::node::HASHED_RESOURCE)
                .unwrap()
                .high_water,
            crate::node::MAX_HASHED_BYTES_PER_GRAPH + 1
        );
        assert_eq!(allocation.usage(), (2, length + 3));
        assert!(!allocation.exceedances().is_exceeded());

        let short = crate::node::GraphBudget::new();
        short
            .hash(crate::node::MAX_HASHED_BYTES_PER_GRAPH as usize - length + 1)
            .unwrap();
        super::read_attempts(
            &path,
            &mut Allocation::new(&Limits::standard()),
            Some(&short),
        )
        .unwrap();
        let axis = short
            .exceedances()
            .axis(crate::node::HASHED_RESOURCE)
            .copied()
            .unwrap();
        assert_eq!(
            axis.first_requested,
            crate::node::MAX_HASHED_BYTES_PER_GRAPH + 1
        );
        assert_eq!(axis.high_water, crate::node::MAX_HASHED_BYTES_PER_GRAPH + 1);
        // The read charges exactly two path checks: one before inspection and
        // one before the open.
        let short = crate::node::GraphBudget::new();
        short
            .paths(crate::node::MAX_PATH_CHECKS_PER_GRAPH - 1)
            .unwrap();
        super::read_attempts(
            &path,
            &mut Allocation::new(&Limits::standard()),
            Some(&short),
        )
        .unwrap();
        let axis = short
            .exceedances()
            .axis(crate::node::PATHS_RESOURCE)
            .copied()
            .unwrap();
        assert_eq!(
            axis.high_water,
            crate::node::MAX_PATH_CHECKS_PER_GRAPH as u64 + 1
        );
        assert!(short
            .exceedances()
            .axis(crate::node::HASHED_RESOURCE)
            .is_none());
        // The canonical account's items and bytes are accounting
        // targets. One short of the exact floor still reads the attempt and
        // records the crossed axis at its truthful high water; the read's own
        // bound is the physical `MAX_WIRE_BYTES`.
        let mut short = Allocation::with_limits(2, length + 2);
        short.items(1).unwrap();
        short.bytes(3).unwrap();
        assert_eq!(
            &*super::read_attempts(&path, &mut short, None).unwrap(),
            b"attempt"
        );
        assert_eq!(short.usage(), (2, length + 3));
        assert!(!short.is_refused());
        let axis = *short
            .exceedances()
            .axis("canonical allocation bytes")
            .expect("the byte axis records");
        assert_eq!(
            (axis.target, axis.first_requested, axis.high_water),
            (
                (length + 2) as u64,
                (length + 3) as u64,
                (length + 3) as u64
            )
        );
        assert!(short
            .exceedances()
            .axis("canonical allocation items")
            .is_none());
        let mut short = Allocation::with_limits(1, length + 3);
        short.items(1).unwrap();
        short.bytes(3).unwrap();
        assert_eq!(
            &*super::read_attempts(&path, &mut short, None).unwrap(),
            b"attempt"
        );
        assert_eq!(short.usage(), (2, length + 3));
        let axis = *short
            .exceedances()
            .axis("canonical allocation items")
            .expect("the item axis records");
        assert_eq!(
            (axis.target, axis.first_requested, axis.high_water),
            (1, 2, 2)
        );
        assert!(short
            .exceedances()
            .axis("canonical allocation bytes")
            .is_none());
        // Terminal only through a genuine cause: a latched account reads nothing.
        assert!(matches!(
            super::read_attempts(&path, &mut Allocation::refused(), None),
            Err(Error::Node { operation, .. })
                if operation == crate::extract::REFUSED_ACCOUNT_OPERATION
        ));
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn repeated_control_and_sidecar_reads_keep_the_original_graph_budget() {
        let root = std::env::temp_dir().join(format!(
            "rawr-probe-replay-account-{}-{:?}",
            std::process::id(),
            std::thread::current().id(),
        ));
        fs::create_dir_all(&root).unwrap();
        let root = fs::canonicalize(root).unwrap();
        let control = root.join("plan.json");
        let sidecar = root.join("sidecar.json");
        fs::write(&control, b"control").unwrap();
        fs::write(&sidecar, b"sidecar").unwrap();
        let digest = |bytes: &[u8]| <[u8; 32]>::from(Sha256::digest(bytes));
        let limits = Limits::standard();
        let graph = crate::node::GraphBudget::new();
        graph
            .hash(crate::node::MAX_HASHED_BYTES_PER_GRAPH as usize - 28)
            .unwrap();
        let mut capture = Capture {
            search: None,
            namespace: None,
            document: protocol::Document {
                schema: protocol::SCHEMA,
                fragments: Vec::new(),
            },
            deferred: false,
            document_bytes: 0,
            invocations: Invocations {
                authority: InvocationAuthority {
                    expected: BTreeMap::new(),
                    external_units: BTreeMap::new(),
                },
                allocation: Allocation::new(&limits),
            },
            sidecars: Box::from([(sidecar, digest(b"sidecar"))]),
            controls: Box::from([(control, digest(b"control"))]),
            artifacts: Box::default(),
            graph_budget: Some(graph.clone()),
        };
        capture.revalidate(&limits).unwrap();
        capture.revalidate(&limits).unwrap();
        assert_eq!(capture.invocations.allocation.usage(), (4, 28));
        // Two revalidations hash exactly 28 bytes against the original
        // graph (no exceedance); one more byte, then a third revalidation,
        // record the hashed axis at exactly target + 1 + 14.
        assert!(!graph.exceedances().is_exceeded());
        assert_eq!(
            graph.census_usage().unwrap().2,
            crate::node::MAX_HASHED_BYTES_PER_GRAPH
        );
        graph.hash(1).unwrap();
        assert_eq!(
            graph
                .exceedances()
                .axis(crate::node::HASHED_RESOURCE)
                .unwrap()
                .high_water,
            crate::node::MAX_HASHED_BYTES_PER_GRAPH + 1
        );
        capture.revalidate(&limits).unwrap();
        assert_eq!(capture.invocations.allocation.usage(), (6, 42));
        assert_eq!(
            graph
                .exceedances()
                .axis(crate::node::HASHED_RESOURCE)
                .unwrap()
                .high_water,
            crate::node::MAX_HASHED_BYTES_PER_GRAPH + 15
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn moved_document_keeps_control_sidecar_and_artifact_revalidation_fail_closed() {
        let root = std::env::temp_dir().join(format!(
            "rawr-revalidation-{}-{:?}",
            std::process::id(),
            std::thread::current().id(),
        ));
        fs::create_dir_all(&root).expect("the revalidation root is created");
        let control = root.join("plan.json");
        let sidecar = root.join("sidecar.json");
        let artifact = root.join("libfixture.rmeta");
        fs::write(&control, b"control").expect("the control fixture is written");
        fs::write(&sidecar, b"sidecar").expect("the sidecar fixture is written");
        fs::write(&artifact, b"artifact").expect("the artifact fixture is written");
        let digest = |bytes: &[u8]| <[u8; 32]>::from(Sha256::digest(bytes));
        let limits = Limits::standard();
        let capture = || Capture {
            search: None,
            namespace: None,
            document: protocol::Document {
                schema: protocol::SCHEMA,
                fragments: Vec::new(),
            },
            deferred: false,
            document_bytes: 0,
            invocations: Invocations {
                authority: InvocationAuthority {
                    expected: BTreeMap::new(),
                    external_units: BTreeMap::new(),
                },
                allocation: Allocation::new(&limits),
            },
            sidecars: Box::from([(sidecar.clone(), digest(b"sidecar"))]),
            controls: Box::from([(control.clone(), digest(b"control"))]),
            artifacts: Box::from([(artifact.clone(), 8, digest(b"artifact"))]),
            graph_budget: None,
        };

        capture()
            .revalidate(&limits)
            .expect("untouched evidence revalidates");
        fs::write(&control, b"changed").expect("the control fixture is tampered");
        assert_eq!(
            code(
                capture()
                    .revalidate(&limits)
                    .expect_err("control drift rejects")
            ),
            "metadata-probe-control-drift",
        );
        fs::write(&control, b"control").expect("the control fixture is restored");
        fs::write(&sidecar, b"changed").expect("the sidecar fixture is tampered");
        assert_eq!(
            code(
                capture()
                    .revalidate(&limits)
                    .expect_err("sidecar drift rejects")
            ),
            "compiler-sidecar-drift",
        );
        fs::write(&sidecar, b"sidecar").expect("the sidecar fixture is restored");
        fs::write(&artifact, b"artifacx").expect("the artifact fixture is tampered");
        assert_eq!(
            code(
                capture()
                    .revalidate(&limits)
                    .expect_err("artifact drift rejects")
            ),
            "selected-artifact-drift",
        );
        fs::remove_dir_all(root).expect("the revalidation root is removed");
    }

    /// A sealed replay reads only the handshakes; the complete document
    /// is decoded on demand from the same digest-checked sidecars.
    #[test]
    fn deferred_capture_decodes_digest_checked_sidecars_on_demand() {
        let root = std::env::temp_dir().join(format!("rawr-deferred-{}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        let sidecar = root.join("unit.json");
        let id = "1".repeat(64);
        let fragment = serde_json::json!({
            "handshake": {
                "schema": protocol::SCHEMA,
                "package": {"cargo_id": "a 0.1.0", "name": "a", "version": "0.1.0", "source": "path+a"},
                "root": {"kind": "selected", "aliases": ["a"]},
                "target": "a", "target_kind": "lib", "target_kinds": ["lib"],
                "crate_types": ["lib"], "crate_name": "a", "edition": "2021", "profile": "dev",
                "compilation": "target", "host": "h", "selected_target": "h", "features": [],
                "probe_arguments_digest": id, "compiler_commit": "c", "invocation_nonce": id,
                "unit_correlation": id, "unit_key": "a", "dependencies": [],
                "artifact": {"path": "a", "length": 1, "digest": "d", "profile": "dev",
                    "fresh": true, "crate_hash": "x", "stable_crate_id": "y", "probe_alias": "a"},
                "sysroot": [], "loaded_crates": [], "external_crates": [],
            },
            "reachable": [], "definitions": [], "implementations": [],
            "scalar_predicate_proofs": [], "index": [],
        });
        let bytes = serde_json::to_vec(&fragment).unwrap();
        fs::write(&sidecar, &bytes).unwrap();
        let limits = Limits::standard();
        let mut allocation = Allocation::new(&limits);
        let mut document = Vec::new();
        append_document_prefix(&mut document, &limits, &mut allocation).unwrap();
        append(&mut document, &bytes, &limits, &mut allocation).unwrap();
        append(&mut document, b"]}", &limits, &mut allocation).unwrap();
        let full = protocol::decode(&document, &limits).unwrap();
        let handshakes = protocol::decode_handshakes(&document).unwrap();
        assert_eq!(handshakes, full);
        let capture = |digest: [u8; 32]| Capture {
            search: None,
            namespace: None,
            document: handshakes.clone(),
            deferred: true,
            document_bytes: document.len(),
            invocations: Invocations {
                authority: InvocationAuthority {
                    expected: BTreeMap::new(),
                    external_units: BTreeMap::new(),
                },
                allocation: Allocation::new(&limits),
            },
            sidecars: Box::from([(sidecar.clone(), digest)]),
            controls: Box::default(),
            artifacts: Box::default(),
            graph_budget: None,
        };
        let digest = <[u8; 32]>::from(Sha256::digest(&bytes));
        let (decoded, _) = capture(digest).into_parts(&limits).unwrap();
        assert_eq!(decoded, full);
        let Err(Error::Extraction { code, .. }) = capture([0; 32]).into_parts(&limits) else {
            panic!("a drifted sidecar must not decode");
        };
        assert_eq!(code, "compiler-sidecar-drift");
        fs::remove_dir_all(root).unwrap();
    }

    /// A sidecar's semantic key ignores the run's nonce, argument
    /// digest, artifact path, bytes and SVH, and follows its body and its
    /// semantic handshake fields.
    #[test]
    fn semantic_sidecars_follow_the_body_and_semantic_handshake_only() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../target/test-scratch")
            .join(format!("semantic-sidecars-{}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        let sidecar = root.join("unit.json");
        let limits = Limits::standard();
        let handshake = |nonce: &str, crate_hash: &str, crate_name: &str| {
            serde_json::json!({
                "schema": protocol::SCHEMA,
                "package": {"cargo_id": "a 0.1.0", "name": "a", "version": "0.1.0", "source": "path+a"},
                "root": {"kind": "selected", "aliases": ["a"]},
                "target": "a", "target_kind": "lib", "target_kinds": ["lib"],
                "crate_types": ["lib"], "crate_name": crate_name, "edition": "2021", "profile": "dev",
                "compilation": "target", "host": "h", "selected_target": "h", "features": [],
                "probe_arguments_digest": nonce, "compiler_commit": "c", "invocation_nonce": nonce,
                "unit_correlation": nonce, "unit_key": "a", "dependencies": [],
                "artifact": {"path": nonce, "length": 1, "digest": nonce, "profile": "dev",
                    "fresh": true, "crate_hash": crate_hash, "stable_crate_id": "y", "probe_alias": "a"},
                "sysroot": [], "loaded_crates": [{"crate_name": "b", "stable_crate_id": "z",
                    "artifacts": [nonce]}], "external_crates": [],
            })
        };
        // The driver writes the handshake first, then the body's fields.
        let driver = |handshake: serde_json::Value, body: &str| {
            format!("{{\"handshake\":{handshake},\"reachable\":[],\"definitions\":[{body}],\"implementations\":[],\"scalar_predicate_proofs\":[],\"index\":[]}}")
                .into_bytes()
        };
        let key = |bytes: &[u8], digest: Option<[u8; 32]>| {
            fs::write(&sidecar, bytes).unwrap();
            Capture {
                search: None,
                namespace: None,
                document: protocol::Document {
                    schema: protocol::SCHEMA,
                    fragments: Vec::new(),
                },
                deferred: true,
                document_bytes: 0,
                invocations: Invocations {
                    authority: InvocationAuthority {
                        expected: BTreeMap::new(),
                        external_units: BTreeMap::new(),
                    },
                    allocation: Allocation::new(&limits),
                },
                sidecars: Box::from([(
                    sidecar.clone(),
                    digest.unwrap_or_else(|| Sha256::digest(bytes).into()),
                )]),
                controls: Box::default(),
                artifacts: Box::default(),
                graph_budget: None,
            }
            .semantic_sidecars()
        };
        let first = key(&driver(handshake("n1", "h1", "a"), ""), None).expect("a driver sidecar");
        assert_eq!(first.len(), 1);
        let rerun = key(&driver(handshake("n2", "h2", "a"), ""), None).expect("a later run");
        assert_eq!(
            first, rerun,
            "nonce, paths, artifact bytes and SVH are not semantic"
        );
        let body = key(&driver(handshake("n1", "h1", "a"), "1"), None).unwrap();
        assert_eq!(body[0].0, first[0].0);
        assert_ne!(body[0].1, first[0].1, "a body change changes the key");
        let named = key(&driver(handshake("n1", "h1", "b"), ""), None).unwrap();
        assert_ne!(
            named[0].0, first[0].0,
            "a semantic handshake field changes the key"
        );
        let reordered = serde_json::to_vec(&serde_json::json!({
            "handshake": handshake("n1", "h1", "a"), "reachable": [], "definitions": [],
            "implementations": [], "scalar_predicate_proofs": [], "index": [],
        }))
        .unwrap();
        assert!(
            key(&reordered, None).is_none(),
            "only the driver's layout is keyed"
        );
        let bytes = driver(handshake("n1", "h1", "a"), "");
        assert!(
            key(&bytes, Some([0; 32])).is_none(),
            "a drifted sidecar is not keyed"
        );
        fs::remove_dir_all(root).unwrap();
    }

    /// The sidecar document size is an accounting target recorded on
    /// the canonical account, and sidecar reads never bind to the JSON target.
    #[test]
    fn sidecar_document_bytes_record_the_json_target_and_reads_are_unbounded() {
        assert_eq!(crate::process::CAPTURE_BYTES, 256 * 1024 * 1024);
        let mut limits = Limits::standard();
        limits.json_bytes = 1;
        let mut allocation = Allocation::new(&limits);
        let mut bytes = Vec::new();
        append(&mut bytes, b"]}", &limits, &mut allocation)
            .expect("a document above the JSON target still appends");
        assert_eq!(bytes, b"]}");
        let axis = *allocation
            .exceedances()
            .axis("compiler sidecar document bytes")
            .expect("the document size records its axis");
        assert_eq!(
            (axis.target, axis.first_requested, axis.high_water),
            (1, 2, 2)
        );
        assert!(!allocation.is_refused());

        let root = std::env::temp_dir().join(format!(
            "rawr-sidecar-physical-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        fs::create_dir_all(&root).unwrap();
        let path = root.join("fragment.json");
        fs::write(&path, b"{}\n").unwrap();
        let read = super::read_bounded_with_graph(&path, Some(&mut allocation), None)
            .expect("a sidecar above the JSON target reads");
        assert_eq!(&*read, b"{}\n");
        fs::remove_dir_all(root).unwrap();
    }

    /// Artifact hashing in namespace capture, completion, and
    /// inventory revalidation binds to the physical capture constant that
    /// `Probe::run` passes, never to the configured `json_bytes` target, so a
    /// tiny JSON target cannot refuse a real compiler artifact.
    #[test]
    fn artifact_hashing_binds_to_the_physical_capture_bound_not_the_json_target() {
        assert_eq!(crate::process::CAPTURE_BYTES, 268_435_456);
        let root = std::env::temp_dir().join(format!(
            "rawr-artifact-physical-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        fs::create_dir_all(&root).unwrap();
        let root = fs::canonicalize(root).unwrap();
        let path = root.join("libfixture.rmeta");
        let bytes = vec![41_u8; 4096];
        fs::write(&path, &bytes).unwrap();
        let mut limits = Limits::standard();
        limits.json_bytes = 1;
        let mut allocation = Allocation::new(&limits);
        // The artifact is far above the JSON target and still hashes whole
        // under the physical bound, with nothing recorded on the account.
        assert_eq!(
            super::artifact_digest(
                &path,
                crate::process::CAPTURE_BYTES,
                &mut allocation,
                Some(&crate::node::GraphBudget::new())
            )
            .unwrap(),
            (bytes.len() as u64, Sha256::digest(&bytes).into())
        );
        assert!(!allocation.exceedances().is_exceeded());
        // Bound to the JSON target instead, the same digest would refuse.
        assert!(super::artifact_digest(
            &path,
            limits.json_bytes,
            &mut allocation,
            Some(&crate::node::GraphBudget::new())
        )
        .is_err());
        // Namespace capture hashes the same candidate under the same bound.
        let roots = vec![root.clone()];
        super::namespace::Snapshot::capture(
            &roots,
            &[],
            &mut allocation,
            &crate::node::GraphBudget::new(),
            crate::process::CAPTURE_BYTES,
        )
        .expect("a candidate above the JSON target captures under the physical bound");
        assert!(!allocation.exceedances().is_exceeded());
        assert!(super::namespace::Snapshot::capture(
            &roots,
            &[],
            &mut allocation,
            &crate::node::GraphBudget::new(),
            limits.json_bytes
        )
        .is_err());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn capture_document_prefix_uses_the_decoder_schema_authority() {
        let limits = Limits::standard();
        let mut allocation = Allocation::new(&limits);
        let mut bytes = Vec::new();
        append_document_prefix(&mut bytes, &limits, &mut allocation)
            .expect("the Capture prefix is bounded");
        append(&mut bytes, b"]}", &limits, &mut allocation)
            .expect("the empty Capture document closes");
        let decoded = protocol::decode(&bytes, &limits)
            .expect("the Capture encoder and protocol decoder share one schema authority");
        assert_eq!(decoded.schema, protocol::SCHEMA);

        let stale = format!(
            "{{\"schema\":{},\"fragments\":[]}}",
            protocol::SCHEMA.saturating_sub(1)
        );
        assert_eq!(
            code(protocol::decode(stale.as_bytes(), &limits).expect_err("schema drift rejects")),
            "unknown-sidecar-schema",
        );
    }

    #[test]
    fn run_nonce_is_semantic_and_stable_across_process_local_state() {
        let limits = Limits::standard();
        let graph = cargo::digest(b"unit graph");
        let nonce = |graph, commit: &str, target: &str| {
            run_nonce(graph, commit, target, &mut Allocation::new(&limits))
                .expect("the semantic run nonce seals")
        };
        let expected = nonce(graph, "compiler", "aarch64-apple-darwin");
        assert_eq!(nonce(graph, "compiler", "aarch64-apple-darwin"), expected);
        assert_ne!(
            nonce(
                cargo::digest(b"changed graph"),
                "compiler",
                "aarch64-apple-darwin"
            ),
            expected
        );
        assert_ne!(
            nonce(graph, "changed compiler", "aarch64-apple-darwin"),
            expected
        );
        assert_ne!(
            nonce(graph, "compiler", "x86_64-unknown-linux-gnu"),
            expected
        );
    }
}
