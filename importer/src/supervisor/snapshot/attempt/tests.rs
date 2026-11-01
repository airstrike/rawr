use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

use super::Scope;
use crate::supervisor::snapshot::Bytes;
use crate::{cargo, node};

static NEXT: AtomicUsize = AtomicUsize::new(0);
const SOURCE: &[u8] = b"before";
const MANIFEST: &[u8] = b"[package]\nname = 'test'\n";

struct Fixture {
    root: PathBuf,
    bytes: Bytes,
}

impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "rawr-snapshot-attempt-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed),
        ));
        fs::create_dir(&root).unwrap();
        let root = fs::canonicalize(root).unwrap();
        fs::write(root.join("main.rs"), SOURCE).unwrap();
        fs::write(root.join("Cargo.toml"), MANIFEST).unwrap();
        let bytes = snapshot(&root, SOURCE);
        Self { root, bytes }
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.root).unwrap();
    }
}

fn snapshot(root: &std::path::Path, source: &[u8]) -> Bytes {
    let entry = root.join("main.rs");
    let manifest = root.join("Cargo.toml");
    let limits = cargo::Limits::standard();
    let mut snapshot = crate::surface::Snapshot::builder(&limits);
    snapshot.entry(entry.to_str().unwrap()).unwrap();
    snapshot.file(manifest.to_str().unwrap(), MANIFEST).unwrap();
    snapshot.file(entry.to_str().unwrap(), source).unwrap();
    let mut framed = Bytes::capture(&limits);
    framed.entry(entry.to_str().unwrap()).unwrap();
    framed.file(manifest.to_str().unwrap(), MANIFEST).unwrap();
    framed.file(entry.to_str().unwrap(), source).unwrap();
    framed.bind(snapshot.seal().unwrap()).unwrap()
}

#[test]
fn original_attempt_accumulates_initial_post_compile_and_final_source_checks() {
    let fixture = Fixture::new();
    let scope = Scope::begin();
    let proof = scope.observe(&fixture.bytes).unwrap();
    assert!(proof.matches(&fixture.bytes));
    assert!(!proof.matches(&snapshot(&fixture.root, b"after!")));
    let first = scope.budget().census_usage().unwrap();
    let contents = (SOURCE.len() + MANIFEST.len()) as u64;
    assert_eq!(first.0, fixture.bytes.framed.len() as u64);
    assert_eq!(first.2, contents * 2); // Bound bytes plus initial filesystem check.
    proof.revalidate().unwrap();
    let second = scope.budget().census_usage().unwrap();
    assert_eq!(second.0, first.0);
    assert!(second.1 > first.1);
    assert_eq!(second.2, contents * 3);
    proof.revalidate().unwrap();
    let third = scope.budget().census_usage().unwrap();
    assert_eq!(third.1 - second.1, second.1 - first.1);
    assert_eq!(third.2, contents * 4);
}

#[test]
fn freshness_keeps_its_original_scope_and_rejects_manifest_drift() {
    let fixture = Fixture::new();
    let scope = Scope::begin();
    let proof = scope.observe(&fixture.bytes).unwrap();
    let budget = scope.budget().clone();
    drop(scope);
    proof.revalidate().unwrap();
    assert!(budget.census_usage().unwrap().2 > (SOURCE.len() + MANIFEST.len()) as u64 * 2);
    fs::write(
        fixture.root.join("Cargo.toml"),
        b"[package]\nname = 'next'\n",
    )
    .unwrap();
    assert!(matches!(
        proof.revalidate(),
        Err(node::Error::SourceChanged(_))
    ));
}

#[test]
fn resident_observation_charges_the_original_graph() {
    let fixture = Fixture::new();
    let probe = Scope::begin();
    assert!(probe
        .observe(&fixture.bytes)
        .unwrap()
        .matches(&fixture.bytes));
    let (decoded, paths, hashed) = probe.budget().census_usage().unwrap();
    assert!(decoded > 0 && paths > 0 && hashed > 0);
}

#[test]
fn full_preparation_context_and_resident_proof_share_one_cumulative_account() {
    let fixture = Fixture::new();
    let scope = Scope::begin();
    let limits = cargo::Limits::standard();
    let proof = scope.observe(&fixture.bytes).unwrap();
    let before_fallback = scope.budget().census_usage().unwrap();
    let cargo_home = fixture.root.join("cargo-home");
    fs::create_dir(&cargo_home).unwrap();
    let environment =
        cargo::config::Environment::from_pairs([("CARGO_HOME", cargo_home.to_str().unwrap())])
            .unwrap();
    let context = crate::context::Context::capture_in(
        &fixture.root,
        environment,
        "attempt-target",
        &limits,
        scope.budget(),
    )
    .unwrap();
    assert!(context.graph_budget().shares_with(scope.budget()));
    assert_eq!(
        context.graph_budget().census_usage().unwrap(),
        before_fallback
    );
    let cloned = context.clone().without_toolchain_memo();
    assert!(cloned.graph_budget().shares_with(scope.budget()));
    // The proof keeps charging the one cumulative account the context
    // clones share; the revalidation of a deleted source charges its path
    // checks there and fails for its own I/O reason.
    fs::remove_file(fixture.root.join("main.rs")).unwrap();
    assert!(matches!(
        proof.revalidate(),
        Err(node::Error::Io {
            operation: "revalidate bound Rawr snapshot source",
            ..
        })
    ));
    let (_, paths_after, _) = context.graph_budget().census_usage().unwrap();
    assert!(paths_after > before_fallback.1);
    context.graph_budget().paths(1).unwrap();
    assert_eq!(
        cloned.graph_budget().census_usage().unwrap().1,
        paths_after + 1
    );
}
