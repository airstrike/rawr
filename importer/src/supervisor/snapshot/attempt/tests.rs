use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

use super::{cargo, node, Scope};
use crate::supervisor::snapshot::Bytes;

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
        fs::write(root.join("main.rawr"), SOURCE).unwrap();
        fs::write(root.join("rawr.toml"), MANIFEST).unwrap();
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
    let entry = root.join("main.rawr");
    let manifest = root.join("rawr.toml");
    let limits = cargo::Limits::standard();
    let mut snapshot = crate::surface::Snapshot::builder(&limits);
    snapshot.entry(entry.to_str().unwrap()).unwrap();
    snapshot.file(entry.to_str().unwrap(), source).unwrap();
    snapshot.file(manifest.to_str().unwrap(), MANIFEST).unwrap();
    let mut framed = Bytes::capture(&limits);
    framed.entry(entry.to_str().unwrap()).unwrap();
    framed.file(entry.to_str().unwrap(), source).unwrap();
    framed.file(manifest.to_str().unwrap(), MANIFEST).unwrap();
    framed.bind(snapshot.seal().unwrap(), &limits).unwrap()
}

#[test]
fn original_attempt_accumulates_initial_post_compile_and_final_source_checks() {
    let fixture = Fixture::new();
    let scope = Scope::begin();
    let proof = scope
        .observe(&fixture.bytes, &cargo::Limits::standard())
        .unwrap();
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
    let proof = scope
        .observe(&fixture.bytes, &cargo::Limits::standard())
        .unwrap();
    let budget = scope.budget().clone();
    drop(scope);
    proof.revalidate().unwrap();
    assert!(budget.census_usage().unwrap().2 > (SOURCE.len() + MANIFEST.len()) as u64 * 2);
    fs::write(
        fixture.root.join("rawr.toml"),
        b"[package]\nname = 'next'\n",
    )
    .unwrap();
    assert!(matches!(
        proof.revalidate(),
        Err(node::Error::Corrupt { .. })
    ));
}

#[test]
fn resident_observation_records_each_exhausted_original_graph_cap() {
    let fixture = Fixture::new();
    let limits = cargo::Limits::standard();
    // Exact per-observation charges, measured on a fresh original account.
    let probe = Scope::begin();
    assert!(probe
        .observe(&fixture.bytes, &limits)
        .unwrap()
        .matches(&fixture.bytes));
    let (decoded, paths, hashed) = probe.budget().census_usage().unwrap();
    assert!(decoded > 0 && paths > 0 && hashed > 0);
    assert!(!probe.budget().exceedances().is_exceeded());
    // An exhausted original target never refuses the observation. Each
    // axis keeps charging the same cumulative account and records exactly the
    // observation's own charge past its target, and only that axis.
    for resource in ["decode", "paths", "hash"] {
        let scope = Scope::begin();
        match resource {
            "decode" => scope
                .budget()
                .decode(node::MAX_DECODED_BYTES_PER_GRAPH as usize)
                .unwrap(),
            "paths" => scope
                .budget()
                .paths(node::MAX_PATH_CHECKS_PER_GRAPH)
                .unwrap(),
            "hash" => scope
                .budget()
                .hash(node::MAX_HASHED_BYTES_PER_GRAPH as usize)
                .unwrap(),
            _ => unreachable!(),
        }
        let (expected, target, charged) = match resource {
            "decode" => (
                node::DECODED_RESOURCE,
                node::MAX_DECODED_BYTES_PER_GRAPH,
                decoded,
            ),
            "paths" => (
                node::PATHS_RESOURCE,
                node::MAX_PATH_CHECKS_PER_GRAPH as u64,
                paths as u64,
            ),
            "hash" => (
                node::HASHED_RESOURCE,
                node::MAX_HASHED_BYTES_PER_GRAPH,
                hashed,
            ),
            _ => unreachable!(),
        };
        let proof = scope
            .observe(&fixture.bytes, &limits)
            .expect("an accounting axis never refuses");
        assert!(proof.matches(&fixture.bytes));
        let record = scope.budget().exceedances();
        assert_eq!(record.axes().count(), 1, "{resource}");
        let axis = record.axis(expected).unwrap();
        assert_eq!(axis.target, target, "{resource}");
        assert_eq!(axis.high_water, target + charged, "{resource}");
        assert!(axis.first_requested > target && axis.first_requested <= axis.high_water);
    }
}

#[test]
fn full_preparation_context_and_resident_proof_share_one_cumulative_account() {
    let fixture = Fixture::new();
    let scope = Scope::begin();
    let limits = cargo::Limits::standard();
    let proof = scope.observe(&fixture.bytes, &limits).unwrap();
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
    cloned
        .graph_budget()
        .paths(node::MAX_PATH_CHECKS_PER_GRAPH - before_fallback.1)
        .unwrap();
    assert!(!scope.budget().exceedances().is_exceeded());
    // The proof keeps charging the one cumulative account the context
    // clones share. A deleted source distinguishes that charge, recorded past
    // the exhausted target before the path is touched, from a fresh account:
    // the revalidation fails for its own I/O reason, never for accounting.
    fs::remove_file(fixture.root.join("main.rawr")).unwrap();
    assert!(matches!(
        proof.revalidate(),
        Err(node::Error::Io {
            operation: "revalidate bound Rawr snapshot source",
            ..
        })
    ));
    let (_, paths_after, _) = context.graph_budget().census_usage().unwrap();
    let axis = context
        .graph_budget()
        .exceedances()
        .axis(node::PATHS_RESOURCE)
        .copied()
        .expect("the shared account recorded the revalidation path checks");
    assert_eq!(axis.target, node::MAX_PATH_CHECKS_PER_GRAPH as u64);
    assert_eq!(axis.high_water, paths_after as u64);
    assert!(axis.first_requested > axis.target);
    assert_eq!(
        scope.budget().exceedances().axis(node::PATHS_RESOURCE),
        Some(&axis)
    );
    context
        .graph_budget()
        .paths(1)
        .expect("an accounting axis never refuses");
    assert_eq!(
        cloned
            .graph_budget()
            .exceedances()
            .axis(node::PATHS_RESOURCE)
            .map(|axis| axis.high_water),
        Some(paths_after as u64 + 1)
    );
}

/// A configured `[rust.limits]` graph target is the profile the
/// attempt's one account measures against on every route. The resident
/// observation's own path checks cross `graph-path-checks = 1` and are recorded
/// against that target; the owner route's context then shares the same bound
/// account, and a different profile cannot rebind it.
#[test]
fn configured_graph_targets_bind_the_attempt_account_and_record_path_exceedance() {
    let fixture = Fixture::new();
    let mut limits = cargo::Limits::standard();
    limits.configure("graph-path-checks", 1).unwrap();
    assert_eq!(limits.graph_targets().paths, 1);
    let scope = Scope::begin();
    let proof = scope
        .observe(&fixture.bytes, &limits)
        .expect("an accounting axis never refuses");
    assert!(proof.matches(&fixture.bytes));
    let (_, paths, _) = scope.budget().census_usage().unwrap();
    assert!(
        paths > 1,
        "the observation performs more than one path check"
    );
    let record = scope.budget().exceedances();
    assert_eq!(record.axes().count(), 1, "only the configured axis crossed");
    let axis = record
        .axis(node::PATHS_RESOURCE)
        .expect("the configured path-check target is crossed and recorded");
    assert_eq!(axis.target, 1);
    assert_eq!(axis.high_water, paths as u64);
    assert!(axis.first_requested > 1 && axis.first_requested <= axis.high_water);
    assert_eq!(scope.budget().targets(), limits.graph_targets());

    // The owner route captures its context in the same bound account.
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
        scope.budget_for(&limits).unwrap(),
    )
    .unwrap();
    assert!(context.graph_budget().shares_with(scope.budget()));
    assert_eq!(context.graph_budget().targets(), limits.graph_targets());
    context
        .graph_budget()
        .paths(1)
        .expect("an accounting axis never refuses");
    assert_eq!(
        scope
            .budget()
            .exceedances()
            .axis(node::PATHS_RESOURCE)
            .unwrap()
            .high_water,
        paths as u64 + 1
    );

    // One attempt has one profile: another profile is drift, not a rebind.
    assert!(matches!(
        scope.budget_for(&cargo::Limits::standard()),
        Err(node::Error::State(_))
    ));
    assert!(matches!(
        scope.observe(&fixture.bytes, &cargo::Limits::standard()),
        Err(node::Error::State(_))
    ));
    assert_eq!(scope.budget().targets(), limits.graph_targets());
    assert_eq!(
        scope.budget().census_usage().unwrap().1,
        paths + 1,
        "the refused drift charged nothing"
    );
    // An unbound scope measures against the standard targets, and the standard
    // profile then binds it as the same account.
    let standard = Scope::begin();
    assert_eq!(standard.budget().targets(), node::GraphTargets::STANDARD);
    assert!(standard
        .budget_for(&cargo::Limits::standard())
        .unwrap()
        .shares_with(standard.budget()));
}
