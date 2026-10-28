//! Deterministic hot-union fixtures; no compiler or generated process runs.

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use super::*;
use crate::runner::{Built, Generated, Manifest};

#[path = "hot/outcome_tests.rs"]
mod outcome_tests;

struct Value<const KIND: u8>(Box<[u8]>);

impl<const KIND: u8> node::Value for Value<KIND> {
    const KIND: node::Kind = match KIND {
        7 => node::Kind::Checked,
        8 => node::Kind::ProfileProof,
        9 => node::Kind::NativeProduct,
        _ => node::Kind::Overlay,
    };
    const SCHEMA: u32 = 99;

    fn encode(&self, _: usize) -> Result<Box<[u8]>, node::Error> {
        Ok(self.0.clone())
    }

    fn decode(bytes: &[u8], _: usize) -> Result<Self, node::Error> {
        Ok(Self(bytes.into()))
    }
}

struct Fixture {
    retain: bool,
    root: PathBuf,
    context: crate::context::Context,
    owner: crate::supervisor::Owner,
    validations: Arc<AtomicUsize>,
    reject_at: Arc<AtomicUsize>,
    runtime_drift_at: Arc<AtomicUsize>,
    runtime_asset: Arc<std::sync::Mutex<Option<PathBuf>>>,
    _project: crate::store::TestProject,
}

impl Fixture {
    fn new() -> Self {
        let project = crate::store::TestProject::new("rawr-hot-union");
        let root = project.path().to_path_buf();
        fs::create_dir(root.join("cargo-home")).unwrap();
        fs::write(root.join("native.rs"), b"native").unwrap();
        fs::write(root.join("main.rs"), b"before").unwrap();
        fs::create_dir(root.join("runner-harness")).unwrap();
        fs::write(root.join("runner-harness/Cargo.lock"), b"version = 4\n\n[[package]]\nname = \"rawr-resolution-harness\"\nversion = \"0.0.0\"\n").unwrap();
        let context = context(&root);
        crate::cargo::toolchain::Attestation::fixture_current(&context, &root.join("toolchain"));
        Self {
            retain: false,
            root,
            context,
            owner: crate::supervisor::Owner::new(),
            validations: Arc::new(AtomicUsize::new(0)),
            reject_at: Arc::new(AtomicUsize::new(usize::MAX)),
            runtime_drift_at: Arc::new(AtomicUsize::new(usize::MAX)),
            runtime_asset: Arc::new(std::sync::Mutex::new(None)),
            _project: project,
        }
    }

    fn generation(
        &mut self,
        source: &[u8],
    ) -> (Manifest<Generated, ()>, crate::supervisor::snapshot::Bytes) {
        let context = context(&self.root);
        // Reacquire current lineage in the new graph instead of transplanting
        // the prior graph's receipt. This fixture may only reuse its toolchain.
        crate::cargo::toolchain::Attestation::fixture_replay(&context);
        let store = context.open_store().unwrap();
        let evaluator = node::Evaluator::new(&store, &crate::metrics::Noop);
        let toolchain = context.cached_toolchain().unwrap();
        let retained_parent = self.retain.then(|| {
            node::Chain::fixture_bundled_retained(
                context.clone(),
                &[node::Parent::validated(toolchain.as_ref())],
            )
            .unwrap()
        });
        let parents = retained_parent
            .as_ref()
            .map(|parent| vec![parent.parent()])
            .unwrap_or_default();
        let recipe = context
            .discover_with_parents::<Value<9>>(b"native", 4096, &parents)
            .unwrap();
        let native_path = self.root.join("native.rs");
        let validations = Arc::clone(&self.validations);
        let reject_at = Arc::clone(&self.reject_at);
        let runtime_drift_at = Arc::clone(&self.runtime_drift_at);
        let runtime_asset = Arc::clone(&self.runtime_asset);
        let native = evaluator
            .discover_budgeted(
                &recipe,
                move |_, budget| {
                    budget.paths(1)?;
                    budget.hash(6)?;
                    let validation = validations.fetch_add(1, Ordering::Relaxed) + 1;
                    if validation == runtime_drift_at.load(Ordering::Relaxed) {
                        corrupt_asset(runtime_asset.lock().unwrap().as_ref().unwrap());
                    }
                    if validation == reject_at.load(Ordering::Relaxed) {
                        return Ok(Box::from(&b"drift!"[..]));
                    }
                    fs::read(&native_path)
                        .map(Vec::into_boxed_slice)
                        .map_err(|source| node::Error::Io {
                            operation: "read deterministic native witness",
                            path: native_path.clone(),
                            source,
                        })
                },
                || Ok(Value::<9>(Box::from(&b"native"[..]))),
            )
            .unwrap();
        let chain = if let Some(parent) = retained_parent {
            let retained = parent.retention().unwrap().extend_current(&native).unwrap();
            node::Chain::validated_retained(context, &native, retained).unwrap()
        } else {
            node::Chain::validated(context, &native)
        };
        let checked = chain
            .direct::<Value<7>>(source, || Ok(Value(source.into())))
            .unwrap();
        let checked = match chain.retention() {
            Some(retained) => node::Chain::validated_retained(
                chain.context().clone(),
                &checked,
                retained.extend_current(&checked).unwrap(),
            )
            .unwrap(),
            None => chain.child(&checked),
        };
        let profile = checked
            .direct::<Value<8>>(b"profile", || Ok(Value(Box::from(&b"stable"[..]))))
            .unwrap();
        let profile = match checked.retention() {
            Some(retained) => node::Chain::validated_retained(
                checked.context().clone(),
                &profile,
                retained.extend_current(&profile).unwrap(),
            )
            .unwrap(),
            None => checked.child(&profile),
        };
        let path = self.root.join("main.rs");
        let path = path.to_str().unwrap();
        let limits = crate::cargo::Limits::standard();
        let mut digest = crate::surface::Snapshot::builder(&limits);
        digest.entry(path).unwrap();
        digest.file(path, source).unwrap();
        let snapshot = digest.seal().unwrap();
        let request = crate::bridge::fixture_generation(
            profile.clone(),
            &self.root,
            snapshot,
            self.owner.revision().unwrap(),
            &limits,
        )
        .unwrap();
        let manifest = request.generate(&limits).unwrap();
        assert_eq!(
            manifest.chain.as_ref().unwrap().parent().action(),
            profile.parent().action(),
            "Overlay must remain a separate application chain, not the Runner semantic parent"
        );
        let mut transport = crate::supervisor::snapshot::Bytes::capture(&limits);
        transport.entry(path).unwrap();
        transport.file(path, source).unwrap();
        let snapshot = transport.bind(manifest.request.snapshot, &limits).unwrap();
        (manifest, snapshot)
    }

    fn build(&self, manifest: Manifest<Generated, ()>) -> Manifest<Built, ()> {
        self.build_with_capture(manifest, |_| {}).unwrap()
    }

    fn build_with_capture(
        &self,
        manifest: Manifest<Generated, ()>,
        captured: impl FnOnce(&Path),
    ) -> Result<Manifest<Built, ()>, crate::runner::BuildError> {
        self.build_with_drift(manifest, false, captured)
    }

    fn build_with_drift(
        &self,
        manifest: Manifest<Generated, ()>,
        after_pending: bool,
        captured: impl FnOnce(&Path),
    ) -> Result<Manifest<Built, ()>, crate::runner::BuildError> {
        let manifest = build_fixture(manifest, after_pending, captured, &crate::metrics::Noop)?;
        *self.runtime_asset.lock().unwrap() = Some(asset(&manifest));
        Ok(manifest)
    }

    fn prepare(
        &mut self,
        live: &Manifest<Built, ()>,
        source: &[u8],
    ) -> crate::process::hot::Prepared<()> {
        let (mut manifest, snapshot) = self.generation(source);
        let mut read_set = replay_hot(&mut manifest, live).unwrap();
        read_set.bind_snapshot(&snapshot).unwrap();
        read_set.pin_prepared().unwrap();
        let epoch = self.owner.epoch().unwrap();
        let key = crate::supervisor::GenerationKey::seal(
            manifest.generated.build_request_key(),
            manifest.request.snapshot,
            &manifest.request.canonical,
            manifest.revision,
            epoch,
            &live.native_identity(),
        );
        crate::process::hot::Prepared {
            previous: key,
            next: key,
            nonce: [0; 16],
            route: None,
            manifest,
            read_set,
        }
    }
}

/// Reuse the existing deterministic artifact producer with an actual derived request.
pub(crate) fn fixture_build(
    manifest: Manifest<Generated, ()>,
) -> Result<Manifest<Built, ()>, crate::runner::BuildError> {
    build_fixture(manifest, false, |_| {}, &crate::metrics::Current)
}

fn build_fixture(
    mut manifest: Manifest<Generated, ()>,
    after_pending: bool,
    captured: impl FnOnce(&Path),
    sink: &impl crate::metrics::Sink,
) -> Result<Manifest<Built, ()>, crate::runner::BuildError> {
    let chain = manifest.profile.node_chain().unwrap().clone();
    let context = chain.context();
    let store = context.open_store().unwrap();
    let toolchain = context.cached_toolchain().unwrap();
    let inputs = Inputs::capture(&manifest).unwrap();
    let parallelism = crate::process::parallelism::Proof::discover().unwrap();
    let authority = authority(&inputs, &toolchain, parallelism, context).unwrap();
    let parents = [chain.parent(), node::Parent::validated(toolchain.as_ref())];
    let recipe = context
        .discover_with_parents::<Product>(&authority, PRODUCT_BYTES, &parents)
        .unwrap();
    let validation_store = store.clone();
    let validation = inputs.clone();
    let limits = context.limits();
    let product = node::Evaluator::new(&store, sink).discover_bundled_budgeted(
        &recipe,
        move |value, budget| value.revalidate(&validation_store, &validation, budget),
        |attempt| {
            materialize(&manifest, &attempt.generated())?;
            let path = executable(&attempt.target());
            fs::create_dir(path.parent().unwrap()).unwrap();
            fs::write(&path, b"deterministic executable").unwrap();
            fs::create_dir_all(attempt.build().join("opaque/out/empty")).unwrap();
            fs::write(
                attempt.build().join("opaque/out/runtime.asset"),
                b"runtime data",
            )
            .unwrap();
            let budget = store.graph_budget();
            let artifact = crate::runner::artifact::BuiltArtifactProof::seal_budgeted(
                &manifest.generated,
                &path,
                &attempt.target(),
                &[],
                &limits,
                budget,
            )
            .unwrap();
            // The executable read is a physical capture-class I/O bound; the
            // node product bound stays MAX_CONTENT_BYTES.
            let executable = Evidence::capture(&path, crate::process::CAPTURE_BYTES, budget)?;
            let carrier = Evidence::capture(
                &attempt.generated().join("rawr-generated-manifest.carrier"),
                limits.node_output_bytes(),
                budget,
            )?;
            let retained = retained::Stored::capture(
                &attempt.root(),
                attempt.reference(),
                retained::Mode::Pending,
                budget,
            )?;
            let seal = bundle_seal(
                attempt.reference(),
                manifest.generated.digest(),
                &carrier,
                &executable,
                artifact.digest(),
                &retained,
                budget,
            )?;
            let product = Product::capture(
                attempt.reference(),
                frozen_mode(executable.mode),
                executable.bytes,
                executable.digest,
                &seal,
                manifest.generated.digest(),
                artifact.digest(),
                &retained,
                budget,
            )?;
            let root = attempt.root();
            let mut captured = Some(captured);
            if !after_pending {
                captured.take().unwrap()(&root);
            }
            let pending = attempt.seal_budgeted(seal, budget)?;
            let bundled = node::Bundled::validate(product, pending, |product, pending| {
                product.revalidate_pending(pending, &inputs, budget)
            })?;
            if let Some(captured) = captured {
                captured(&root);
            }
            Ok(bundled)
        },
    )?;
    let pin = crate::store::bundle::pin_budgeted(
        store.root(),
        product.value().reference,
        store.graph_budget(),
    )
    .unwrap();
    let native = product
        .value()
        .native(&store, &inputs, &manifest.generated, &limits, pin)
        .unwrap();
    let runner = retain_runner(&chain, &product)?;
    let prepared = prepare_union(&runner, manifest.overlay.as_ref()).unwrap();
    manifest.chain = Some(runner);
    let manifest = manifest.built(native, Some(prepared));
    Ok(manifest)
}

fn context(root: &Path) -> crate::context::Context {
    crate::context::Context::capture(
        root,
        crate::cargo::config::Environment::from_pairs([(
            "CARGO_HOME",
            root.join("cargo-home").to_str().unwrap(),
        )])
        .unwrap(),
        crate::cargo::toolchain::HOST,
        &crate::cargo::Limits::standard(),
    )
    .unwrap()
}

fn pins(store: &crate::store::Store) -> BTreeMap<PathBuf, Vec<u8>> {
    let mut values = BTreeMap::new();
    for dir in [
        "ledgers/projects",
        "pins/live",
        "pins/last-good",
        "pins/prepared",
    ] {
        for entry in fs::read_dir(store.root().join(dir)).unwrap() {
            let path = entry.unwrap().path();
            let path = if path.is_dir() {
                path.join("root")
            } else {
                path
            };
            values.insert(path.clone(), fs::read(path).unwrap());
        }
    }
    values
}

fn asset(manifest: &Manifest<Built, ()>) -> PathBuf {
    manifest
        .artifact()
        .executable()
        .ancestors()
        .nth(3)
        .unwrap()
        .join("build/opaque/out/runtime.asset")
}

fn corrupt_asset(path: &Path) {
    use std::os::unix::fs::PermissionsExt as _;
    let metadata = fs::metadata(path).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o644)).unwrap();
    fs::write(path, b"changed data").unwrap();
    fs::set_permissions(path, metadata.permissions()).unwrap();
    assert_eq!(fs::metadata(path).unwrap().len(), metadata.len());
}

#[test]
fn cold_and_hot_runner_keep_exact_bundle_retention() {
    let mut fixture = Fixture::new();
    fixture.retain = true;
    let (cold, _) = fixture.generation(b"before");
    let built = fixture.build(cold);
    let current = built.chain.as_ref().unwrap();
    current
        .retention()
        .unwrap()
        .validate_storage(&current.context().open_store().unwrap())
        .unwrap();
    let (mut next, _) = fixture.generation(b"after");
    replay_hot(&mut next, &built).unwrap();
    let replayed = next.chain.as_ref().unwrap();
    assert_eq!(current.parent().action(), replayed.parent().action());
    let retained = replayed.retention().unwrap();
    retained
        .validate_storage(&replayed.context().open_store().unwrap())
        .unwrap();
    corrupt_asset(&asset(&built));
    // Storage retention is not native-content authority: the retained bundle
    // inventory commitment stays exact, while actual native replay rejects drift.
    assert!(built.revalidate_native().is_err());
}

#[test]
fn startup_handoff_removes_only_the_three_attachment_refreshes() {
    for startup in [false, true] {
        let mut fixture = Fixture::new();
        fixture.retain = true;
        let (cold, snapshot) = fixture.generation(b"before");
        let mut built = fixture.build(cold);
        // Evaluation's final union has already completed. Count subsequent
        // startup consumers, using the same retained fixture for both routes.
        fixture.validations.store(0, Ordering::Relaxed);
        if startup {
            built.retain_startup(&snapshot).unwrap();
        } else {
            built.bind_snapshot(&snapshot).unwrap();
            built.pin_prepared().unwrap();
        }
        assert_eq!(
            fixture.validations.load(Ordering::Relaxed),
            if startup { 0 } else { 1 }
        );
        built.revalidate_prepared().unwrap(); // watcher checkpoint
        built.bind_startup_snapshot(&snapshot).unwrap();
        let read_set = built.take_prepared_read_set().unwrap();
        read_set
            .with_startup_pin(|| {
                assert_eq!(
                    fixture.validations.load(Ordering::Relaxed),
                    if startup { 2 } else { 5 }
                );
                crate::supervisor::bootstrap::admit_current(Some(&read_set), || Ok(()))
            })
            .unwrap()
            .unwrap();
        read_set.revalidate().unwrap(); // observer established
        read_set.pin_live_last_good().unwrap();
        assert_eq!(
            fixture.validations.load(Ordering::Relaxed),
            if startup { 5 } else { 8 }
        );
        // Including evaluation, these downstream schedules are six and nine.
    }
}

#[test]
fn startup_handoff_rejects_drift_and_consumes_its_marker_once() {
    let mut fixture = Fixture::new();
    fixture.retain = true;
    let (cold, snapshot) = fixture.generation(b"before");
    let mut built = fixture.build(cold);
    fixture.validations.store(0, Ordering::Relaxed);
    fixture.reject_at.store(1, Ordering::Relaxed);
    built.retain_startup(&snapshot).unwrap();
    built.bind_startup_snapshot(&snapshot).unwrap();
    assert_eq!(fixture.validations.load(Ordering::Relaxed), 0);
    let read_set = built.take_prepared_read_set().unwrap();
    let calls = AtomicUsize::new(0);
    assert!(read_set
        .with_startup_pin(|| {
            calls.fetch_add(1, Ordering::Relaxed);
            Ok::<(), ()>(())
        })
        .is_err());
    assert_eq!(calls.load(Ordering::Relaxed), 0);
    assert_eq!(fixture.validations.load(Ordering::Relaxed), 1);
    fixture.reject_at.store(usize::MAX, Ordering::Relaxed);
    fixture.validations.store(0, Ordering::Relaxed);
    read_set
        .with_startup_pin(|| Ok::<(), ()>(()))
        .unwrap()
        .unwrap();
    assert_eq!(
        fixture.validations.load(Ordering::Relaxed),
        2,
        "a consumed marker falls back to ordinary pin/current validation"
    );
}

#[test]
fn startup_post_bind_failure_restores_predecessors_without_minting_a_marker() {
    let mut fixture = Fixture::new();
    fixture.retain = true;
    let (cold, snapshot) = fixture.generation(b"before");
    let mut live = fixture.build(cold);
    live.bind_snapshot(&snapshot).unwrap();
    live.pin_prepared().unwrap();
    live.pin_live_last_good().unwrap();
    fs::write(fixture.root.join("main.rs"), b"after!").unwrap();
    let (next, snapshot) = fixture.generation(b"after!");
    let mut built = fixture.build(next);
    let store = built
        .chain
        .as_ref()
        .unwrap()
        .context()
        .open_store()
        .unwrap();
    let before = pins(&store);
    let guards = store.fixture_pin_counts().unwrap();
    fixture.validations.store(0, Ordering::Relaxed);
    assert!(matches!(
        crate::store::transaction::with_post_bind_failure(|| built.retain_startup(&snapshot)),
        Err(node::Error::State("injected pin transaction failure"))
    ));
    assert_eq!(fixture.validations.load(Ordering::Relaxed), 0);
    assert_eq!(pins(&store), before);
    assert_eq!(store.fixture_pin_counts().unwrap(), guards);
    // No startup marker escaped: matching rebinding takes the ordinary current
    // check instead of the bookkeeping-only handoff path.
    built.bind_startup_snapshot(&snapshot).unwrap();
    assert_eq!(fixture.validations.load(Ordering::Relaxed), 1);
}

#[test]
fn startup_handoff_cannot_replace_snapshot_or_existing_ownership() {
    let mut fixture = Fixture::new();
    fixture.retain = true;
    let (cold, snapshot) = fixture.generation(b"before");
    let mut built = fixture.build(cold);
    built.retain_startup(&snapshot).unwrap();
    assert!(built.retain_startup(&snapshot).is_err());
    let store = built
        .chain
        .as_ref()
        .unwrap()
        .context()
        .open_store()
        .unwrap();
    let before = pins(&store);
    let (_, different) = fixture.generation(b"after!");
    assert!(built.bind_startup_snapshot(&different).is_err());
    assert_eq!(pins(&store), before);
    built.bind_startup_snapshot(&snapshot).unwrap();
}

#[test]
fn retained_storage_accounting_is_charged_on_every_refresh() {
    let mut fixture = Fixture::new();
    fixture.retain = true;
    let (cold, snapshot) = fixture.generation(b"before");
    let mut live = fixture.build(cold);
    live.bind_snapshot(&snapshot).unwrap();
    let chain = live.chain.as_ref().unwrap();
    let store = chain.context().open_store().unwrap();
    let budget = store.graph_budget();
    let retention = chain.retention().unwrap();
    let before = budget.census_usage().unwrap();
    retention.validate_storage(&store).unwrap();
    let first = budget.census_usage().unwrap();
    retention.validate_storage(&store).unwrap();
    let second = budget.census_usage().unwrap();
    let delta = |before: (u64, usize, u64), after: (u64, usize, u64)| {
        (after.0 - before.0, after.1 - before.1, after.2 - before.2)
    };
    let storage = delta(before, first);
    // An unchanged closure still incurs real original-account costs each time;
    // retention must not turn a previous refresh into free current authority.
    assert_eq!(storage, delta(first, second));
    assert!(storage.0 > 0 && storage.1 > 0 && storage.2 > 0);
    println!(
        "fixture-account\tstage=runner-storage\tdecoded={}\tpaths={}\thashed={}",
        storage.0, storage.1, storage.2
    );
    let before = budget.census_usage().unwrap();
    live.pin_prepared().unwrap();
    let prepared = delta(before, budget.census_usage().unwrap());
    println!(
        "fixture-account\tstage=prepared\tdecoded={}\tpaths={}\thashed={}",
        prepared.0, prepared.1, prepared.2
    );
    let before = budget.census_usage().unwrap();
    live.pin_live_last_good().unwrap();
    let admitted = delta(before, budget.census_usage().unwrap());
    println!(
        "fixture-account\tstage=live-last-good\tdecoded={}\tpaths={}\thashed={}",
        admitted.0, admitted.1, admitted.2
    );
}

#[test]
fn retained_pin_transactions_validate_once_and_preserve_rollback() {
    let mut fixture = Fixture::new();
    fixture.retain = true;
    let (cold, snapshot) = fixture.generation(b"before");
    let mut live = fixture.build(cold);
    live.bind_snapshot(&snapshot).unwrap();
    fixture.validations.store(0, Ordering::Relaxed);
    live.pin_prepared().unwrap();
    assert_eq!(fixture.validations.load(Ordering::Relaxed), 1);
    fixture.validations.store(0, Ordering::Relaxed);
    live.pin_live_last_good().unwrap();
    assert_eq!(fixture.validations.load(Ordering::Relaxed), 1);

    fs::write(fixture.root.join("main.rs"), b"after!").unwrap();
    let prepared = fixture.prepare(&live, b"after!");
    let store = fixture.context.open_store().unwrap();
    let before = pins(&store);
    let guards = store.fixture_pin_counts().unwrap();
    let usage = store.fixture_project_usage().unwrap();
    fixture.validations.store(0, Ordering::Relaxed);
    let result = prepared
        .read_set
        .with_live_last_good(|| Err::<(), _>("rejected"));
    assert!(matches!(
        result,
        Ok(Err(crate::store::transaction::Rejected::Operation(
            "rejected"
        )))
    ));
    assert_eq!(fixture.validations.load(Ordering::Relaxed), 1);
    assert_eq!(pins(&store), before);
    assert_eq!(store.fixture_pin_counts().unwrap(), guards);
    assert_eq!(store.fixture_project_usage().unwrap(), usage);

    fixture.validations.store(0, Ordering::Relaxed);
    fixture.reject_at.store(1, Ordering::Relaxed);
    assert!(prepared
        .read_set
        .with_live_last_good(|| -> Result<(), &'static str> {
            panic!("post-bind freshness failure must prevent admission")
        })
        .is_err());
    assert_eq!(pins(&store), before);
    assert_eq!(store.fixture_pin_counts().unwrap(), guards);
    assert_eq!(store.fixture_project_usage().unwrap(), usage);
}

#[test]
fn retained_corruption_rejects_pre_spawn_even_after_bootstrap_takes_read_set() {
    for failure in 0..4 {
        let mut fixture = Fixture::new();
        let (cold, _) = fixture.generation(b"before");
        let mut built = fixture.build(cold);
        built.revalidate_native().unwrap();
        let executable = built.artifact().digest();
        let native = built.native_identity();
        let _prepared = built.take_prepared_read_set().unwrap();
        let store = built
            .chain
            .as_ref()
            .unwrap()
            .context()
            .open_store()
            .unwrap();
        let graph = store.graph_budget();
        // Spend the axis exactly to its target (charging past the
        // target records an exceedance and never refuses).
        let (decoded, paths, hashed) = graph.census_usage().unwrap();
        let exhausted = match failure {
            0 => {
                corrupt_asset(&asset(&built));
                None
            }
            1 => {
                graph
                    .paths(node::MAX_PATH_CHECKS_PER_GRAPH - paths)
                    .unwrap();
                Some((node::PATHS_RESOURCE, node::MAX_PATH_CHECKS_PER_GRAPH as u64))
            }
            2 => {
                graph
                    .hash((node::MAX_HASHED_BYTES_PER_GRAPH - hashed) as usize)
                    .unwrap();
                Some((node::HASHED_RESOURCE, node::MAX_HASHED_BYTES_PER_GRAPH))
            }
            3 => {
                graph
                    .decode((node::MAX_DECODED_BYTES_PER_GRAPH - decoded) as usize)
                    .unwrap();
                Some((node::DECODED_RESOURCE, node::MAX_DECODED_BYTES_PER_GRAPH))
            }
            _ => unreachable!(),
        };
        assert!(!graph.exceedances().is_exceeded());
        let calls = AtomicUsize::new(0);
        let result = built.revalidate_native().map(|_| {
            calls.fetch_add(1, Ordering::Relaxed);
        });
        match exhausted {
            None => {
                // Retained corruption is physical evidence: it still rejects.
                assert!(result.is_err());
                assert_eq!(calls.load(Ordering::Relaxed), 0);
            }
            Some((resource, target)) => {
                // An exhausted accounting axis never refuses: revalidation
                // behaves exactly like the healthy case and the graph records
                // the exceedance on that axis with a truthful high water.
                result.unwrap();
                assert_eq!(calls.load(Ordering::Relaxed), 1);
                let exceeded = graph.exceedances();
                let axis = exceeded.axis(resource).unwrap();
                assert_eq!(axis.target, target);
                assert!(axis.first_requested > target);
                let (decoded, paths, hashed) = graph.census_usage().unwrap();
                let charged = match failure {
                    1 => paths as u64,
                    2 => hashed,
                    _ => decoded,
                };
                assert_eq!(axis.high_water, charged);
                assert_eq!(exceeded.axes().count(), 1);
            }
        }
        assert_eq!(built.artifact().digest(), executable);
        assert_eq!(built.native_identity(), native);
    }
}

#[test]
fn retained_corruption_rejects_direct_hot_replay_and_final_cold_gate() {
    let mut fixture = Fixture::new();
    let (cold, snapshot) = fixture.generation(b"before");
    let mut built = fixture.build(cold);
    built.bind_snapshot(&snapshot).unwrap();
    built.pin_prepared().unwrap();
    built.revalidate_native().unwrap();
    built.revalidate_prepared().unwrap();
    let (mut current, _) = fixture.generation(b"before");
    let store = fixture.context.open_store().unwrap();
    let before = pins(&store);
    corrupt_asset(&asset(&built));
    let recorder = crate::metrics::recorder::Recorder::transform_at(10_000, 1).unwrap();
    let scope = recorder.scope().unwrap();
    assert!(replay_hot(&mut current, &built).is_err());
    assert!(crate::supervisor::bootstrap::admit_current::<()>(
        built.phase.read_set.as_ref(),
        || panic!("runtime drift must precede cold admission")
    )
    .is_err());
    drop(scope);
    no_execution(recorder);
    assert_eq!(pins(&store), before);
}

#[test]
fn retained_equal_allocation_rename_rejects_after_supervisor_controls_validate() {
    use std::os::unix::fs::PermissionsExt as _;
    let mut fixture = Fixture::new();
    let (cold, _) = fixture.generation(b"before");
    let built = fixture.build(cold);
    let asset = asset(&built);
    let parent = asset.parent().unwrap();
    let mode = fs::metadata(parent).unwrap().permissions();
    fs::set_permissions(parent, fs::Permissions::from_mode(0o755)).unwrap();
    fs::rename(&asset, parent.join("replace.asset")).unwrap();
    fs::set_permissions(parent, mode).unwrap();
    // A same-count rename is sealed-geometry drift, positive
    // candidate-local corruption at the complete bundle check, before the
    // retained-data replay.
    let result = built.revalidate_native();
    assert!(
        matches!(
            &result,
            Err(crate::runner::artifact::Error::Node(node::Error::CandidateCorrupt(corruption)))
                if format!("{corruption:?}").contains("fact: Geometry")
        ),
        "{result:?}"
    );
    assert!(built.revalidate_prepared().is_err());
}

#[test]
fn retained_post_bind_drift_rolls_back_before_hot_commit() {
    let mut fixture = Fixture::new();
    let (cold, _) = fixture.generation(b"before");
    let live = fixture.build(cold);
    live.pin_live_last_good().unwrap();
    fs::write(fixture.root.join("main.rs"), b"after!").unwrap();
    let prepared = fixture.prepare(&live, b"after!");
    prepared.revalidate_prepared().unwrap();
    let store = fixture.context.open_store().unwrap();
    let before = pins(&store);
    let guards = store.fixture_pin_counts().unwrap();
    let usage = store.fixture_project_usage().unwrap();
    let identity = live.native_identity();
    fixture.validations.store(0, Ordering::Relaxed);
    fixture.runtime_drift_at.store(2, Ordering::Relaxed);
    let mut transitions = Vec::new();
    assert!(prepared
        .commit(|transition| {
            transitions.push(transition);
            Ok(())
        })
        .is_err());
    assert_eq!(transitions, vec![crate::process::hot::Transition::Abort]);
    assert_eq!(pins(&store), before);
    assert_eq!(store.fixture_pin_counts().unwrap(), guards);
    assert_eq!(store.fixture_project_usage().unwrap(), usage);
    assert_eq!(live.native_identity(), identity);
}

#[test]
fn captured_runtime_drift_never_publishes_and_attempt_is_removed() {
    let mut fixture = Fixture::new();
    let (cold, _) = fixture.generation(b"before");
    let mut attempted = None;
    let result = fixture.build_with_capture(cold, |root| {
        attempted = Some(root.to_path_buf());
        corrupt_asset(&root.join("build/opaque/out/runtime.asset"));
    });
    assert!(result.is_err());
    assert!(!attempted.unwrap().exists());
    let store = fixture.context.open_store().unwrap();
    for schema in fs::read_dir(store.root().join("actions/runner")).unwrap() {
        assert!(fs::read_dir(schema.unwrap().path())
            .unwrap()
            .next()
            .is_none());
    }
}

#[test]
fn runtime_drift_after_pending_validation_cannot_mint_a_published_native_proof() {
    let mut fixture = Fixture::new();
    let (cold, _) = fixture.generation(b"before");
    let result = fixture.build_with_drift(cold, true, |root| {
        corrupt_asset(&root.join("build/opaque/out/runtime.asset"))
    });
    assert!(result.is_err());
}

fn no_execution(
    recorder: crate::metrics::recorder::Recorder,
) -> crate::metrics::recorder::Snapshot {
    let evidence = recorder.snapshot().unwrap();
    assert!(evidence.processes.iter().all(|row| row.count == 0));
    assert!(evidence
        .nodes
        .iter()
        .filter(|row| row.event == crate::metrics::NodeEvent::ExecutedMiss)
        .all(|row| row.count == 0));
    evidence
}

#[test]
fn hot_union_replays_the_same_runner_with_only_current_application_and_overlay() {
    let mut fixture = Fixture::new();
    let (cold, snapshot) = fixture.generation(b"before");
    let mut live = fixture.build(cold);
    live.bind_snapshot(&snapshot).unwrap();
    live.pin_prepared().unwrap();
    live.pin_live_last_good().unwrap();
    let old_overlay = live.overlay.as_ref().unwrap().action();
    let old_profile = live.profile.node_chain().unwrap().parent().action();
    let runner = live.chain.as_ref().unwrap().parent().action();
    fs::write(fixture.root.join("main.rs"), b"after!").unwrap();
    let prepared = fixture.prepare(&live, b"after!");
    assert_eq!(
        prepared.manifest.chain.as_ref().unwrap().parent().action(),
        runner
    );
    let proof = prepared.read_set.proof();
    assert!(proof.contains(runner));
    // A Rawr-only edit reuses the generated overlay (its source never reads
    // the snapshot).
    assert_eq!(
        prepared.manifest.overlay.as_ref().unwrap().action(),
        old_overlay
    );
    assert!(proof.contains(old_overlay));
    assert!(!proof.contains(old_profile));
    prepared.revalidate_prepared().unwrap();
    prepared.admit(|| Ok(())).unwrap();
    let current_snapshot = prepared.manifest.request.snapshot;
    live.accept_hot(prepared.manifest, prepared.read_set);
    assert_eq!(
        live.request.snapshot.as_bytes(),
        current_snapshot.as_bytes()
    );
    assert_eq!(live.chain.as_ref().unwrap().parent().action(), runner);
    live.revalidate_prepared().unwrap();
}

#[test]
fn hot_union_rejects_silent_rawr_and_native_drift_at_both_admission_checks() {
    for native in [false, true] {
        for final_gate in [false, true] {
            let mut fixture = Fixture::new();
            let (cold, _) = fixture.generation(b"before");
            let live = fixture.build(cold);
            let prepared = fixture.prepare(&live, b"before");
            prepared.revalidate_prepared().unwrap();
            let store = fixture.context.open_store().unwrap();
            let before = pins(&store);
            fs::write(
                fixture
                    .root
                    .join(if native { "native.rs" } else { "main.rs" }),
                b"drift!",
            )
            .unwrap();
            let calls = AtomicUsize::new(0);
            let recorder = crate::metrics::recorder::Recorder::transform_at(10_000, 1).unwrap();
            let scope = recorder.scope().unwrap();
            if final_gate {
                assert!(prepared
                    .admit(|| {
                        calls.fetch_add(1, Ordering::Relaxed);
                        Ok(())
                    })
                    .is_err());
            } else {
                assert!(prepared.revalidate_prepared().is_err());
            }
            assert_eq!(calls.load(Ordering::Relaxed), 0);
            drop(scope);
            no_execution(recorder);
            assert_eq!(pins(&store), before);
        }
    }
}

#[test]
fn hot_profile_rejection_restores_exact_pins_and_success_replaces_the_current_proof() {
    let mut fixture = Fixture::new();
    let (cold, _) = fixture.generation(b"before");
    let live = fixture.build(cold);
    live.pin_live_last_good().unwrap();
    fs::write(fixture.root.join("main.rs"), b"after!").unwrap();
    let prepared = fixture.prepare(&live, b"after!");
    let store = fixture.context.open_store().unwrap();
    let before = pins(&store);
    let guards = store.fixture_pin_counts().unwrap();
    assert!(matches!(
        prepared.admit::<()>(|| Err(crate::process::RunnerError::ProfileRejected(
            "fixture".into()
        ))),
        Err(crate::process::RunnerError::ProfileRejected(_))
    ));
    assert_eq!(pins(&store), before);
    assert_eq!(store.fixture_pin_counts().unwrap(), guards);
    prepared.admit(|| Ok(())).unwrap();
    assert_ne!(pins(&store), before);
}

#[test]
fn hot_exact_runner_missing_rejects_without_discovery_or_execution() {
    let mut fixture = Fixture::new();
    let (cold, _) = fixture.generation(b"before");
    let live = fixture.build(cold);
    let (mut manifest, _) = fixture.generation(b"before");
    let action = live.chain.as_ref().unwrap().parent().action();
    let store = fixture.context.open_store().unwrap();
    let path = store
        .root()
        .join("actions/runner")
        .join(node::STORAGE_DIRECTORY)
        .join(action.directory());
    let hidden = path.with_extension("hidden");
    fs::rename(&path, &hidden).unwrap();
    let recorder = crate::metrics::recorder::Recorder::transform_at(10_000, 1).unwrap();
    let scope = recorder.scope().unwrap();
    let result = replay_hot(&mut manifest, &live);
    drop(scope);
    let evidence = no_execution(recorder);
    assert_eq!(
        evidence
            .nodes
            .iter()
            .find(|row| row.kind == crate::metrics::NodeKind::Runner
                && row.event == crate::metrics::NodeEvent::RejectedHit)
            .unwrap()
            .count,
        1
    );
    fs::rename(hidden, path).unwrap();
    assert!(
        matches!(result, Err(node::Error::StaleReadSet { action: missing }) if missing == action)
    );
}

#[test]
fn hot_exact_runner_replay_records_one_validation_hit_without_execution() {
    let mut fixture = Fixture::new();
    let (cold, _) = fixture.generation(b"before");
    let live = fixture.build(cold);
    let (mut manifest, _) = fixture.generation(b"before");
    let recorder = crate::metrics::recorder::Recorder::transform_at(10_000, 1).unwrap();
    let scope = recorder.scope().unwrap();
    replay_hot(&mut manifest, &live).unwrap();
    drop(scope);
    let evidence = no_execution(recorder);
    assert_eq!(
        evidence
            .nodes
            .iter()
            .find(|row| row.kind == crate::metrics::NodeKind::Runner
                && row.event == crate::metrics::NodeEvent::ValidatedHit)
            .unwrap()
            .count,
        1
    );
}

#[test]
fn hot_transition_aborts_validation_and_profile_failures_and_preserves_abort_errors() {
    use crate::process::hot::Transition;
    use crate::process::RunnerError;

    for drift in [false, true] {
        for abort_failure in [false, true] {
            let mut fixture = Fixture::new();
            let (cold, _) = fixture.generation(b"before");
            let live = fixture.build(cold);
            let prepared = fixture.prepare(&live, b"before");
            if drift {
                fs::write(fixture.root.join("main.rs"), b"drift!").unwrap();
            }
            let mut transitions = Vec::new();
            let error = prepared
                .commit(|transition| {
                    transitions.push(transition);
                    match transition {
                        Transition::Commit => Err(RunnerError::ProfileRejected(
                            "fixture profile rejection".into(),
                        )),
                        Transition::Abort if abort_failure => Err(RunnerError::OwnerLost {
                            reason: "fixture abort failure".into(),
                        }),
                        Transition::Abort => Ok(()),
                    }
                })
                .unwrap_err();
            assert_eq!(
                transitions,
                if drift {
                    vec![Transition::Abort]
                } else {
                    vec![Transition::Commit, Transition::Abort]
                }
            );
            assert_eq!(error.is_owner_lost(), abort_failure);
            let failure = match error {
                RunnerError::HotCleanup { failure, cleanup } => {
                    assert!(abort_failure);
                    assert!(matches!(*cleanup, RunnerError::OwnerLost { .. }));
                    *failure
                }
                failure => {
                    assert!(!abort_failure);
                    failure
                }
            };
            assert!(if drift {
                matches!(failure, RunnerError::Node(_))
            } else {
                matches!(failure, RunnerError::ProfileRejected(_))
            });
        }
    }
}

#[test]
fn hot_post_bind_drift_rolls_back_before_any_commit_callback() {
    let mut fixture = Fixture::new();
    let (cold, _) = fixture.generation(b"before");
    let live = fixture.build(cold);
    live.pin_live_last_good().unwrap();
    fs::write(fixture.root.join("main.rs"), b"after!").unwrap();
    let prepared = fixture.prepare(&live, b"after!");
    let store = fixture.context.open_store().unwrap();
    let before = pins(&store);
    let guards = store.fixture_pin_counts().unwrap();
    let usage = store.fixture_project_usage().unwrap();
    fixture.validations.store(0, Ordering::Relaxed);
    fixture.reject_at.store(2, Ordering::Relaxed);
    assert!(matches!(
        prepared.admit::<()>(|| panic!("post-bind drift must precede HotCommit")),
        Err(crate::process::RunnerError::Node(
            node::Error::StaleReadSet { .. }
        ))
    ));
    assert_eq!(fixture.validations.load(Ordering::Relaxed), 2);
    assert_eq!(pins(&store), before);
    assert_eq!(store.fixture_pin_counts().unwrap(), guards);
    assert_eq!(store.fixture_project_usage().unwrap(), usage);
}

#[test]
fn snapshot_binding_cannot_change_after_ownership_and_new_context_has_its_own_budget() {
    let mut fixture = Fixture::new();
    let (cold, before) = fixture.generation(b"before");
    let mut live = fixture.build(cold);
    live.bind_snapshot(&before).unwrap();
    live.pin_prepared().unwrap();
    let (_, after) = fixture.generation(b"after!");
    assert!(live
        .phase
        .read_set
        .as_mut()
        .unwrap()
        .bind_snapshot(&after)
        .is_err());
    live.bind_snapshot(&before).unwrap();
    let old_store = live.chain.as_ref().unwrap().context().open_store().unwrap();
    // Exhaust the old evaluation without altering the new evaluation's budget.
    // The spent old graph still revalidates and records its own path
    // exceedance; the new context's graph is separate and stays clean.
    old_store.graph_budget().exhaust_paths();
    assert!(!old_store.graph_budget().exceedances().is_exceeded());
    live.revalidate_prepared().unwrap();
    let old = old_store.graph_budget().exceedances();
    let axis = old.axis(node::PATHS_RESOURCE).unwrap();
    assert_eq!(axis.target, node::MAX_PATH_CHECKS_PER_GRAPH as u64);
    assert_eq!(
        axis.high_water,
        old_store.graph_budget().census_usage().unwrap().1 as u64
    );
    let prepared = fixture.prepare(&live, b"before");
    let new_graph = prepared
        .manifest
        .chain
        .as_ref()
        .unwrap()
        .context()
        .graph_budget();
    assert!(!new_graph.shares_with(old_store.graph_budget()));
    prepared.revalidate_prepared().unwrap();
    assert!(!new_graph.exceedances().is_exceeded());
    assert_eq!(
        old_store
            .graph_budget()
            .exceedances()
            .axis(node::PATHS_RESOURCE),
        Some(axis)
    );
    prepared.admit(|| Ok(())).unwrap();
}

#[test]
fn cold_final_gate_rejects_drift_after_activation_before_admit() {
    for native in [false, true] {
        let mut fixture = Fixture::new();
        let (cold, snapshot) = fixture.generation(b"before");
        let mut built = fixture.build(cold);
        built.bind_snapshot(&snapshot).unwrap();
        built.pin_prepared().unwrap();
        built.revalidate_prepared().unwrap();
        let proof = built.phase.read_set.as_ref().unwrap();
        fs::write(
            fixture
                .root
                .join(if native { "native.rs" } else { "main.rs" }),
            b"drift!",
        )
        .unwrap();
        assert!(
            crate::supervisor::bootstrap::admit_current::<()>(Some(proof), || panic!(
                "final cold drift must reject before admission"
            ),)
            .is_err()
        );
    }
}
