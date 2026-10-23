#![allow(dead_code)]

use std::fs;
use std::path::{Path, PathBuf};

use rawr_import::cargo::config::{Dependency, Environment};
use rawr_import::classify::representability::Representability;
use rawr_import::classify::Policy;
use rawr_import::extract;
use rawr_import::process::Native;
use rawr_import::surface::{Origin, Snapshot};

const SNAPSHOT_ENTRY: &str = "application.rs";
const SNAPSHOT_SOURCE: &[u8] = b"exact p3 surface";

pub fn generated() -> rawr_import::runner::Manifest<rawr_import::runner::Generated, ()> {
    let project = rawr_import::store::TestProject::new("rawr-p3-generate");
    generated_at(project.path())
}

pub fn generated_at(
    root: &Path,
) -> rawr_import::runner::Manifest<rawr_import::runner::Generated, ()> {
    request_at(root, SNAPSHOT_SOURCE, Surface::Complete)
        .0
        .generate(&rawr_import::cargo::Limits::standard())
        .expect("the deterministic generated manifest seals")
}

#[derive(Clone, Copy)]
pub enum Surface {
    Complete,
    First,
    /// Every demand but the outcome demand at this index.
    Without(usize),
}

pub struct Compiled;

impl rawr_import::surface::Runner for Compiled {
    fn compile_snapshot(&self) -> bool {
        true
    }
}

pub fn request_at(
    root: &Path,
    source: &[u8],
    surface: Surface,
) -> (
    rawr_import::bridge::GenerationRequestKey<()>,
    rawr_import::supervisor::snapshot::Bytes,
) {
    let mut supervisor = rawr_import::supervisor::Owner::new();
    let revision = supervisor.revision().expect("the first revision exists");
    request_at_revision(root, source, surface, revision)
}

pub fn request_at_revision(
    root: &Path,
    source: &[u8],
    surface: Surface,
    revision: rawr_import::supervisor::Revision,
) -> (
    rawr_import::bridge::GenerationRequestKey<()>,
    rawr_import::supervisor::snapshot::Bytes,
) {
    request_at_revision_with_brand(root, source, surface, revision, (), None)
}

pub fn compiled_request_at(
    root: &Path,
    source: &[u8],
) -> (
    rawr_import::bridge::GenerationRequestKey<Compiled>,
    rawr_import::supervisor::snapshot::Bytes,
) {
    let mut supervisor = rawr_import::supervisor::Owner::new();
    let revision = supervisor.revision().expect("the first revision exists");
    request_at_revision_with_brand(root, source, Surface::Complete, revision, Compiled, None)
}

/// A complete request under the shared node context the product always has,
/// which hot admission requires; its snapshot lives on disk under `root`.
pub fn dag_request_at(
    root: &Path,
    chain: &rawr_import::node::Chain,
    source: &[u8],
    surface: Surface,
) -> (
    rawr_import::bridge::GenerationRequestKey<()>,
    rawr_import::supervisor::snapshot::Bytes,
) {
    let mut supervisor = rawr_import::supervisor::Owner::new();
    let revision = supervisor.revision().expect("the first revision exists");
    request_at_revision_with_brand(root, source, surface, revision, (), Some(chain))
}

/// The shared node context for `root`, in the isolated test store.
pub fn dag_chain(root: &Path) -> rawr_import::node::Chain {
    rawr_import::store::isolate_for_tests();
    let context = rawr_import::context::Context::capture(
        root,
        Environment::capture().expect("the P3 DAG environment captures"),
        "aarch64-apple-darwin",
        &rawr_import::cargo::Limits::standard(),
    )
    .expect("the P3 DAG context captures");
    rawr_import::node::Chain::fixture(context).expect("the P3 DAG parent validates")
}

fn request_at_revision_with_brand<B>(
    root: &Path,
    source: &[u8],
    surface: Surface,
    revision: rawr_import::supervisor::Revision,
    brand: B,
    dag: Option<&rawr_import::node::Chain>,
) -> (
    rawr_import::bridge::GenerationRequestKey<B>,
    rawr_import::supervisor::snapshot::Bytes,
)
where
    B: rawr_import::surface::Runner,
{
    let limits = rawr_import::cargo::Limits::standard();
    let native = Native::pinned().expect("the pinned driver toolchain is installed");
    let graph = request(root, &limits)
        .resolve(&native, &limits)
        .and_then(|resolution| extract::from_resolution(resolution, &native, &limits))
        .expect("real facade evidence canonicalizes");
    let contract = graph
        .classify(&Policy::standard(), &limits)
        .expect("real facade evidence classifies");

    // The supervisor revalidates a DAG request's snapshot sources on disk.
    let entry = match dag {
        Some(_) => {
            let path = root.join(SNAPSHOT_ENTRY);
            fs::write(&path, source).expect("the P3 snapshot source writes");
            path.to_str()
                .expect("the P3 snapshot path is Unicode")
                .to_owned()
        }
        None => SNAPSHOT_ENTRY.to_owned(),
    };
    let snapshot = snapshot_with(&limits, &entry, source);
    let mut compiler = contract
        .compiler(brand, snapshot, &limits)
        .expect("the demand compiler starts");
    let mut position = 0_u32;
    let mut skip = match surface {
        Surface::Without(index) => Some(index),
        Surface::Complete | Surface::First => None,
    };
    for outcome in contract.outcomes() {
        require_outcome(
            &mut compiler,
            outcome,
            &entry,
            &limits,
            &mut position,
            &mut skip,
        );
        if matches!(surface, Surface::First) && position != 0 {
            break;
        }
        match outcome.representability() {
            Representability::Struct(plan) if plan.parameters().is_empty() => {
                if let Some(selector) = plan.constructor() {
                    compiler
                        .require_selected(
                            outcome.identity(),
                            selector
                                .try_clone()
                                .expect("the constructor selector duplicates"),
                            &entry,
                            position,
                            position + 1,
                        )
                        .expect("the constructor is demanded");
                    position += 1;
                }
                for field in plan.fields() {
                    compiler
                        .require_selected(
                            outcome.identity(),
                            field
                                .selector()
                                .try_clone()
                                .expect("the field selector duplicates"),
                            &entry,
                            position,
                            position + 1,
                        )
                        .expect("the field is demanded");
                    position += 1;
                }
            }
            Representability::Enum(plan) if plan.parameters().is_empty() => {
                for variant in plan
                    .variants()
                    .iter()
                    .filter(|variant| variant.constructible())
                {
                    compiler
                        .require_selected(
                            outcome.identity(),
                            variant
                                .selector()
                                .try_clone()
                                .expect("the variant selector duplicates"),
                            &entry,
                            position,
                            position + 1,
                        )
                        .expect("the variant is demanded");
                    position += 1;
                }
            }
            _ => {}
        }
        for implementation in outcome.implementations() {
            for associated in implementation.associated() {
                require_outcome(
                    &mut compiler,
                    associated,
                    &entry,
                    &limits,
                    &mut position,
                    &mut skip,
                );
            }
        }
    }
    let demand = compiler.seal().expect("the complete demand seals");
    let proof = contract.into_native_proof();
    let profile = match dag {
        Some(chain) => rawr_import::profile::Proof::neutral_with_chain(chain.clone()),
        None => rawr_import::profile::Proof::neutral(),
    };
    let request = demand
        .request(proof, profile, revision)
        .expect("the exact generation request seals");
    (request, snapshot_transport_with(&entry, source))
}

pub fn snapshot_transport() -> rawr_import::supervisor::snapshot::Bytes {
    let limits = rawr_import::cargo::Limits::standard();
    let expected = snapshot(&limits);
    let mut capture = rawr_import::supervisor::snapshot::Bytes::capture(&limits);
    capture
        .entry(SNAPSHOT_ENTRY)
        .expect("the transport entry is bounded");
    capture
        .file(SNAPSHOT_ENTRY, SNAPSHOT_SOURCE)
        .expect("the transport source is bounded");
    capture
        .bind(expected, &limits)
        .expect("the moved transport matches its sealed snapshot")
}

pub fn other_snapshot_transport() -> rawr_import::supervisor::snapshot::Bytes {
    snapshot_transport_with(SNAPSHOT_ENTRY, b"other p3 surface")
}

fn snapshot_transport_with(entry: &str, source: &[u8]) -> rawr_import::supervisor::snapshot::Bytes {
    let limits = rawr_import::cargo::Limits::standard();
    let expected = snapshot_with(&limits, entry, source);
    let mut capture = rawr_import::supervisor::snapshot::Bytes::capture(&limits);
    capture
        .entry(entry)
        .expect("the other transport entry is bounded");
    capture
        .file(entry, source)
        .expect("the other transport source is bounded");
    capture
        .bind(expected, &limits)
        .expect("the other moved transport matches its own snapshot")
}

fn snapshot(limits: &rawr_import::cargo::Limits) -> Snapshot {
    snapshot_with(limits, SNAPSHOT_ENTRY, SNAPSHOT_SOURCE)
}

fn snapshot_with(limits: &rawr_import::cargo::Limits, entry: &str, source: &[u8]) -> Snapshot {
    let mut snapshot = Snapshot::builder(limits);
    snapshot.entry(entry).expect("the entry path is bounded");
    snapshot
        .file(entry, source)
        .expect("the source bytes are bounded");
    snapshot.seal().expect("the source snapshot seals")
}

fn require_outcome<B>(
    compiler: &mut rawr_import::surface::Compiler<'_, B>,
    outcome: rawr_import::classify::Outcome<'_>,
    entry: &str,
    limits: &rawr_import::cargo::Limits,
    position: &mut u32,
    skip: &mut Option<usize>,
) {
    if !outcome.representability().is_demandable()
        || has_unresolved_structural_parameters(outcome.representability())
    {
        return;
    }
    match skip {
        Some(0) => {
            *skip = None;
            return;
        }
        Some(remaining) => *remaining -= 1,
        None => {}
    }
    compiler
        .require(
            outcome.identity().clone(),
            Origin::checked(entry, *position, *position + 1, limits)
                .expect("the Rawr origin is bounded"),
        )
        .expect("the admitted outcome is demanded");
    *position += 1;
}

fn has_unresolved_structural_parameters(representability: &Representability) -> bool {
    match representability {
        Representability::Struct(plan) => !plan.parameters().is_empty(),
        Representability::Enum(plan) => !plan.parameters().is_empty(),
        _ => false,
    }
}

fn request(root: &Path, limits: &rawr_import::cargo::Limits) -> rawr_import::cargo::Request {
    rawr_import::store::isolate_for_tests();
    let environment = Environment::capture()
        .and_then(|environment| environment.setting("CARGO_NET_OFFLINE", "true"))
        .expect("the P3 environment is sealable");
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let importer = if manifest.ends_with("importer") {
        manifest
    } else {
        manifest.join("../importer")
    };
    let fixture = fs::canonicalize(importer.join("tests/fixtures/symbolic-api"))
        .expect("the immutable symbolic fixture exists");
    rawr_import::cargo::Request::builder(root, &root.join(".rawr/cache"), limits)
        .expect("the limits-bound request builder seals")
        .environment(environment)
        .policy(extract::Policy::manifest(limits).seal())
        .expect("the manifest policy fits")
        .dependency(
            Dependency::path("facade_a", &fixture, limits)
                .and_then(|dependency| dependency.package("symbolic-api"))
                .expect("the first facade alias is exact"),
        )
        .expect("the facade dependency fits")
        .build()
        .expect("the P3 request seals")
}
