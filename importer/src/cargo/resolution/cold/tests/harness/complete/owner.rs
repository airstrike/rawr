//! Real semantic-node publication and replay over the complete cold fixture.

use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use super::*;
use crate::cargo::resolution::owner;
use crate::node;

thread_local! {
    static ATTEMPT: RefCell<Option<PathBuf>> = const { RefCell::new(None) };
    /// Registry packages reachable from the protected unit in this thread's fixture.
    static REGISTRY: Cell<usize> = const { Cell::new(0) };
    /// Final-form lock bytes the last script wrote.
    static LOCK_BYTES: Cell<usize> = const { Cell::new(0) };
    /// Open the next owner fixture at this exact (removed) root, so two
    /// limits profiles run over byte-identical paths.
    static FIXTURE_ROOT: RefCell<Option<PathBuf>> = const { RefCell::new(None) };
}

/// Gallery's prior harness lock holds 345 packages, 338 of them from crates.io,
/// in 85,521 bytes; this many synthetic registry packages renders about that.
const GALLERY_REGISTRY: usize = 340;

/// What one owner cohort's cold → repair → warm run observed.
struct Measured {
    lock: usize,
    /// Owner replays the warm graph validated.
    warm_replays: usize,
}

fn script(
    request: &cargo::Request,
    cached: &cargo::CachedToolchain,
    layout: &Layout,
) -> test_launch::Script {
    ATTEMPT.with(|slot| *slot.borrow_mut() = Some(layout.root().to_path_buf()));
    let native = process::Native::from_attestation(&cached.attestation);
    let (metadata, units) = documents_scaled(request, layout.final_form(), REGISTRY.get());
    let lock = render_lock(&metadata, LockScope::Final);
    LOCK_BYTES.set(lock.len());
    eprintln!(
        "gallery-scale documents: metadata {} bytes, units {} bytes, lock {} bytes, root {}",
        metadata.len(),
        units.len(),
        lock.len(),
        layout.root().display()
    );
    let mut steps = super::super::built::fresh_steps_with_lock(
        request,
        &native,
        layout,
        &metadata,
        &units,
        Some(lock),
    );
    let (build_bytes, files) = artifacts(layout, &metadata, &request.target);
    steps.push(expected_reply(
        request,
        &native,
        layout.final_form(),
        Command::Build,
        true,
        move |_| test_launch::Reply::exit(0, build_bytes, Vec::new(), files),
    ));
    steps.push(probe(layout, &native, &cached.context, Outcome::Success));
    test_launch::Script::new(
        layout.root().to_path_buf(),
        // Gallery-scale replies write two artifact files per package plus the
        // metadata, units and lock documents.
        test_launch::FileLimits {
            count: 1024,
            bytes: 4 * 1024 * 1024,
        },
        steps,
    )
    .expect("complete owner cold script")
}

fn no_cold(_: &cargo::Request, _: &cargo::CachedToolchain, _: &Layout) -> test_launch::Script {
    panic!("an unchanged current owner must not enter mutable preparation")
}

fn snapshot(root: &Path) -> BTreeMap<PathBuf, (Vec<u8>, std::time::SystemTime, bool)> {
    let mut pending = vec![root.to_path_buf()];
    let mut files = BTreeMap::new();
    while let Some(path) = pending.pop() {
        let metadata = fs::symlink_metadata(&path).unwrap();
        assert!(!metadata.file_type().is_symlink());
        if metadata.is_dir() {
            pending.extend(
                fs::read_dir(path)
                    .unwrap()
                    .map(|entry| entry.unwrap().path()),
            );
        } else {
            files.insert(
                path.strip_prefix(root).unwrap().to_path_buf(),
                (
                    fs::read(&path).unwrap(),
                    metadata.modified().unwrap(),
                    metadata.permissions().readonly(),
                ),
            );
        }
    }
    files
}

#[test]
fn complete_owner_node_reuses_without_children_or_mutating_bundle() {
    let Fixture {
        root,
        mut request,
        context,
        store,
    } = Fixture::open();
    let limits = Limits::standard();
    let cached = || cargo::CachedToolchain {
        context: context.clone(),
        attestation: context.cached_toolchain().unwrap(),
    };
    // Seed authentication describes protected sources and the fixed command
    // cohort, independent of the actual owner's later Attempt namespace.
    Fixture::reserve_parts(&context, &store, |_, _| {
        bind_universe(&mut request, &cached());
        Ok(())
    });
    let universe = request.universe.clone();
    let cold_metrics = crate::metrics::recorder::Recorder::transform(1000).unwrap();
    let scope = cold_metrics.scope().unwrap();
    let cold =
        owner::evaluate_scripted(request, cached(), &limits, &crate::metrics::Current, script)
            .expect("actual semantic node cold publication");
    let cold_identity = *cold.node().value().completed_identity();
    let cold_action = cold.node().receipt().action();
    let cold_content = cold.node().content().digest();
    let cold_api = cold
        .publish(&limits)
        .expect("current owner publishes the real API");
    let api_identity = *cold_api.fingerprint().as_bytes();
    drop(scope);
    let cold_metrics = cold_metrics.snapshot().unwrap();
    assert_eq!(
        api_count(&cold_metrics, crate::metrics::NodeEvent::ExecutedMiss),
        1
    );
    let bundle = ATTEMPT.with(|slot| slot.borrow_mut().take().unwrap());
    let before = snapshot(&bundle);
    let mut request = Fixture::request(&root, context.environment().clone());
    request.universe = universe.clone();
    let warm_metrics = crate::metrics::recorder::Recorder::transform(1000).unwrap();
    let scope = warm_metrics.scope().unwrap();
    let warm_api = test_launch::Script::new(
        root.clone(),
        test_launch::FileLimits { count: 0, bytes: 0 },
        Vec::new(),
    )
    .unwrap()
    .run(|| {
        let warm = owner::evaluate_scripted(
            request,
            cached(),
            &limits,
            &crate::metrics::Current,
            no_cold,
        )
        .expect("warm semantic node validates");
        assert_eq!(warm.node().value().completed_identity(), &cold_identity);
        assert_eq!(warm.node().receipt().action(), cold_action);
        assert_eq!(warm.node().content().digest(), cold_content);
        warm.publish(&limits)
    })
    .expect("warm owner and API launch no child")
    .expect("warm API publishes");
    drop(scope);
    let warm_metrics = warm_metrics.snapshot().unwrap();
    assert_eq!(
        api_count(&warm_metrics, crate::metrics::NodeEvent::ExecutedMiss),
        0
    );
    assert_eq!(
        api_count(&warm_metrics, crate::metrics::NodeEvent::ValidatedHit),
        1
    );
    assert_eq!(warm_api.fingerprint().as_bytes(), &api_identity);
    assert_eq!(
        snapshot(&bundle),
        before,
        "warm owner does not rewrite sealed files or permissions"
    );

    let mut request = Fixture::request(&root, context.environment().clone());
    request.universe = universe.clone();
    let pending = owner::evaluate_scripted(
        request,
        cached(),
        &limits,
        &crate::metrics::Current,
        no_cold,
    )
    .unwrap();
    let source = root.join("authority/.cargo/config.toml");
    let original = fs::read(&source).unwrap();
    fs::write(&source, b"[net]\noffline = false\n").unwrap();
    assert!(
        pending.publish(&limits).is_err(),
        "API hit cannot promote an owner held across source changes"
    );
    fs::write(&source, original).unwrap();

    let mut request = Fixture::request(&root, context.environment().clone());
    request.universe = universe.clone();
    let pending = owner::evaluate_scripted(
        request,
        cached(),
        &limits,
        &crate::metrics::Current,
        no_cold,
    )
    .unwrap();
    let attestation = cached();
    let executable = attestation
        .attestation
        .value()
        .proof()
        .executables()
        .find(|(name, _, _, _)| *name == "cargo")
        .unwrap()
        .1
        .to_path_buf();
    assert!(
        executable.starts_with(&root),
        "only mutate the fixture executable"
    );
    let original = fs::read(&executable).unwrap();
    fs::write(&executable, b"changed fixture executable").unwrap();
    assert!(
        pending.publish(&limits).is_err(),
        "API publication must refresh transitive executable authority"
    );
    fs::write(&executable, original).unwrap();

    let mut request = Fixture::request(&root, context.environment().clone());
    request.universe = universe;
    let foreign = owner::evaluate_scripted(
        request,
        cached(),
        &limits,
        &crate::metrics::Current,
        no_cold,
    )
    .unwrap();
    assert!(
        foreign.publish_input(&node::GraphBudget::new()).is_err(),
        "foreign graph cannot consume completed inputs"
    );
    drop((cold_api, warm_api));
    drop(context);
    store.retire_fixture().unwrap();
    fs::remove_dir_all(root).unwrap();
}

fn api_count(
    snapshot: &crate::metrics::recorder::Snapshot,
    event: crate::metrics::NodeEvent,
) -> u64 {
    snapshot
        .nodes
        .iter()
        .find(|count| count.kind == crate::metrics::NodeKind::Api && count.event == event)
        .unwrap()
        .count
}

fn api_continuation_fixture(limits: &Limits) -> Fixture {
    let root = FIXTURE_ROOT.with(|root| root.borrow_mut().take());
    let mut fixture = Fixture::open_at(limits, root.as_deref());
    let cached = cargo::CachedToolchain {
        context: fixture.context.clone(),
        attestation: fixture.context.cached_toolchain().unwrap(),
    };
    Fixture::reserve_parts(&fixture.context, &fixture.store, |layout, _| {
        let (metadata, _) = documents_scaled(&fixture.request, layout.final_form(), REGISTRY.get());
        bind_universe_seeded(&mut fixture.request, &cached, seed_lock(&metadata));
        Ok(())
    });
    fixture
}

#[cfg(unix)]
#[test]
fn protected_owner_rebuild_reuses_derived_semantic_suffix_and_successor() {
    protected_owner_rebuild(0, &Limits::standard());
}

/// The owner route (`prepare_cached_in`) captures its context in the
/// original attempt scope and keeps that scope's graph account on later routes.
#[test]
fn owner_route_context_shares_the_attempt_scope_account() {
    let limits = Limits::standard();
    let fixture = Fixture::open_with(&limits);
    let scope = crate::supervisor::snapshot::attempt::Scope::begin();
    let context = owner::scoped_context_for_test(&fixture.request, &limits, &scope)
        .expect("the owner route captures its context in the attempt scope");
    assert!(context.graph_budget().shares_with(scope.budget()));
    let again = owner::scoped_context_for_test(&fixture.request, &limits, &scope).unwrap();
    assert!(again.graph_budget().shares_with(scope.budget()));
    assert_eq!(again.identity(), context.identity());
    drop((context, again));
    fixture.retire();
}

/// A Gallery-shaped closure completes cold → repair → warm. The warm graph
/// replays its owner once and every later consumer reuses that replay while
/// its stamps hold.
#[cfg(unix)]
#[test]
fn gallery_scale_owner_graphs_complete_and_warm_reuses_one_replay() {
    let limits = Limits::standard();
    let measured =
        profile(GALLERY_REGISTRY, &limits).unwrap_or_else(|panic| std::panic::resume_unwind(panic));
    assert!(measured.lock >= 80 * 1024, "lock is Gallery-sized");
    assert!(
        measured.warm_replays <= 1,
        "warm: {} owner replays; a repeated owner replay returned",
        measured.warm_replays
    );
}

/// Reduced cohorts complete cold → repair → warm.
#[cfg(unix)]
#[test]
fn scaled_owner_graphs_complete_cold_repair_and_warm() {
    let limits = Limits::standard();
    let mut failed = None;
    for registry in [40, 120] {
        match profile(registry, &limits) {
            Ok(_) => eprintln!("cohort({registry}) completed cold → repair → warm"),
            Err(panic) => {
                failed.get_or_insert(panic);
            }
        }
    }
    if let Some(panic) = failed {
        std::panic::resume_unwind(panic);
    }
}

/// Run one cohort, whether or not it completes.
#[cfg(unix)]
fn profile(registry: usize, limits: &Limits) -> std::thread::Result<Measured> {
    let result = std::panic::catch_unwind(|| protected_owner_rebuild(registry, limits));
    if result.is_err() {
        eprintln!(
            "cohort({registry}) failed (lock {} bytes)",
            LOCK_BYTES.get()
        );
    }
    REGISTRY.set(0);
    result
}

#[cfg(unix)]
fn protected_owner_rebuild(registry: usize, limits: &Limits) -> Measured {
    use std::os::unix::fs::PermissionsExt as _;

    REGISTRY.set(registry);
    let Fixture {
        root,
        request,
        context,
        store,
    } = api_continuation_fixture(limits);
    let limits = *limits;
    let universe = request.universe.clone();
    let cached = cargo::CachedToolchain {
        context: context.clone(),
        attestation: context.cached_toolchain().unwrap(),
    };
    let cold = owner::evaluate_scripted(request, cached, &limits, &crate::metrics::Current, script)
        .unwrap();
    let action = cold.node().receipt().action();
    let prepared = node::Chain::validated(context.clone(), cold.node())
        .prepare_read_set()
        .unwrap();
    prepared.pin_live_last_good().unwrap();
    let api = cold.publish(&limits).unwrap();
    let api_content = api.node_chain().unwrap().parent().content();
    let api_semantics = api.semantic_fingerprint();
    let source = api_source(&limits);
    let adapter = api_adapter();
    let completed = crate::composition::evaluate_retained_semantic(
        api,
        &adapter,
        &crate::classify::Policy::standard(),
        source,
        &limits,
        |contract, _, _| {
            Ok::<_, ()>((
                contract
                    .compiler((), source, &limits)
                    .unwrap()
                    .seal()
                    .unwrap(),
                ApiChecked,
            ))
        },
    )
    .unwrap();
    let cold_suffix = derived_suffix(completed, &adapter, source, &limits, Expect::Cold, action);
    // Shutdown releases the OS guards, but leaves the durable prior roots.
    drop(prepared);
    let old_bundle = ATTEMPT.with(|slot| slot.borrow_mut().take().unwrap());
    let seal = old_bundle.join("seal");
    let frozen = fs::metadata(&seal).unwrap().permissions();
    let mut corrupt = fs::read(&seal).unwrap();
    corrupt[0] ^= 1;
    fs::set_permissions(&seal, fs::Permissions::from_mode(0o600)).unwrap();
    fs::write(&seal, corrupt).unwrap();
    fs::set_permissions(&seal, frozen).unwrap();
    let predecessor = snapshot(&old_bundle);

    let reopen = || {
        let mut request = Fixture::request(&root, context.environment().clone());
        request.universe = universe.clone();
        let fresh = crate::context::Context::capture_config(
            context.project(),
            &request.config,
            context.target(),
            &limits,
        )
        .unwrap();
        cargo::toolchain::Attestation::fixture_replay(&fresh);
        let cached = cargo::CachedToolchain {
            attestation: fresh.cached_toolchain().unwrap(),
            context: fresh.clone(),
        };
        (fresh, request, cached)
    };
    let (repair_context, request, cached) = reopen();
    let repaired =
        owner::evaluate_scripted(request, cached, &limits, &crate::metrics::Current, script)
            .expect("protected predecessor cannot block the distinct completed action");
    let repair_action = repaired.node().receipt().action();
    let repair_content = repaired.node().content().digest();
    let repair_completion = *repaired.node().value().completed_identity();
    let fresh_bundle = ATTEMPT.with(|slot| slot.borrow_mut().take().unwrap());
    assert_eq!(
        old_bundle.parent(),
        fresh_bundle.parent(),
        "same execution key"
    );
    assert_ne!(old_bundle, fresh_bundle);
    assert_ne!(action, repair_action);
    let repaired_api = repaired.retain_api(&limits).unwrap();
    assert_eq!(
        owned_identity(&repaired_api.product).1,
        api_content,
        "the entire encoded API Product converges across the protected rebuild"
    );
    let recorder = crate::metrics::recorder::Recorder::transform_at(100_000, 1).unwrap();
    let scope = recorder.scope().unwrap();
    let completed = crate::composition::evaluate_api_semantic(
        repaired_api,
        &adapter,
        &crate::classify::Policy::standard(),
        source,
        &limits,
        |_, _, _| -> std::result::Result<(crate::Demand<crate::Sealed, ()>, ApiChecked), ()> {
            panic!("protected rebuild must reuse the checked source")
        },
    )
    .unwrap();
    assert_eq!(
        completed.contract.raw_graph().semantic_fingerprint(),
        api_semantics
    );
    let repaired_api_identity = completed.contract.raw_fingerprint();
    assert!(completed
        .contract
        .node_chain()
        .unwrap()
        .parent()
        .read_set()
        .contains(repair_action));
    assert!(!completed
        .contract
        .node_chain()
        .unwrap()
        .parent()
        .read_set()
        .contains(action));
    drop(scope);
    assert_prefix_hits(recorder);
    // The repair graph spends its discoverable lookups rechecking the corrupt
    // predecessor after each wait. Crossing the lookup target is
    // recorded, not refused, so the runner lookup proceeds and the cold runner
    // is reused; semantic profile/overlay content converges as before.
    let repaired_suffix = derived_suffix(
        completed,
        &adapter,
        source,
        &limits,
        Expect::Repair,
        repair_action,
    );
    assert_ne!(cold_suffix.exact_input, repaired_suffix.exact_input);
    assert_eq!(cold_suffix.semantic, repaired_suffix.semantic);
    assert_eq!(cold_suffix.contents, repaired_suffix.contents);
    assert_eq!(cold_suffix.actions, repaired_suffix.actions);
    #[cfg(feature = "test-fixtures")]
    assert_eq!(cold_suffix.artifact.2, repaired_suffix.artifact.2);
    assert_eq!(snapshot(&old_bundle), predecessor);
    // Two resolution bundles, plus the one cold runner the repair graph reuses.
    assert_eq!(
        crate::store::bundle::count_bundles(store.root()).unwrap(),
        if cfg!(feature = "test-fixtures") {
            3
        } else {
            2
        }
    );
    store.fixture_physical_inventory().unwrap();

    owner::REPLAYS.with(|replays| replays.set(0));
    let (warm_context, request, cached) = reopen();
    let warm =
        owner::evaluate_scripted(request, cached, &limits, &crate::metrics::Current, no_cold)
            .unwrap();
    assert_eq!(warm.node().receipt().action(), repair_action);
    assert_eq!(warm.node().content().digest(), repair_content);
    assert_eq!(warm.node().value().completed_identity(), &repair_completion);
    let warm_api = warm.publish(&limits).unwrap();
    assert_eq!(warm_api.fingerprint(), repaired_api_identity);
    assert_eq!(
        warm_api.node_chain().unwrap().parent().content(),
        api_content
    );
    assert_eq!(warm_api.semantic_fingerprint(), api_semantics);
    let recorder = crate::metrics::recorder::Recorder::transform_at(100_000, 1).unwrap();
    let scope = recorder.scope().unwrap();
    let completed = crate::composition::evaluate_retained_semantic(
        warm_api,
        &adapter,
        &crate::classify::Policy::standard(),
        source,
        &limits,
        |_, _, _| -> std::result::Result<(crate::Demand<crate::Sealed, ()>, ApiChecked), ()> {
            panic!("warm current graph must reuse the checked source")
        },
    )
    .unwrap();
    drop(scope);
    assert_prefix_hits(recorder);
    let warm_suffix = derived_suffix(
        completed,
        &adapter,
        source,
        &limits,
        Expect::Warm,
        repair_action,
    );
    let warm_replays = validates();
    assert_eq!(warm_suffix.exact_input, repaired_suffix.exact_input);
    assert_eq!(warm_suffix.semantic, repaired_suffix.semantic);
    assert_eq!(warm_suffix.contents, repaired_suffix.contents);
    assert_eq!(warm_suffix.actions, repaired_suffix.actions);
    #[cfg(feature = "test-fixtures")]
    assert_eq!(warm_suffix.artifact, repaired_suffix.artifact);
    assert_eq!(warm_suffix.files, repaired_suffix.files);
    assert_eq!(cold_suffix.files, repaired_suffix.files);
    assert_eq!(snapshot(&old_bundle), predecessor);
    drop(warm_context);
    drop(repair_context);
    drop(context);
    // This fixture establishes coexistence and reuse, not post-admission cleanup.
    store.retire_fixture().unwrap();
    fs::remove_dir_all(&root).unwrap();
    REGISTRY.set(0);
    Measured {
        lock: LOCK_BYTES.get(),
        warm_replays,
    }
}

/// A later consumer whose stamps all hold reuses the replay's result
/// without replaying; a new stamp on a bundle member replays in full, reusing
/// the staged lock and receipt only while the sealed commitments name them,
/// and current control hashing still refuses drift.
#[cfg(unix)]
#[test]
fn staged_owner_replay_reuses_exact_controls_and_refuses_bundle_drift() {
    use std::os::unix::fs::PermissionsExt as _;

    let Fixture {
        root,
        request,
        context,
        store,
    } = api_continuation_fixture(&Limits::standard());
    let limits = Limits::standard();
    let cached = cargo::CachedToolchain {
        context: context.clone(),
        attestation: context.cached_toolchain().unwrap(),
    };
    let cold = owner::evaluate_scripted(request, cached, &limits, &crate::metrics::Current, script)
        .unwrap();
    let hits = || owner::STAGE_HITS.with(std::cell::Cell::get);
    owner::REPLAYS.with(|replays| replays.set(0));
    let before = hits();
    let prepared = node::Chain::validated(context.clone(), cold.node())
        .prepare_read_set()
        .unwrap();
    assert_eq!(validates(), 0, "an unchanged consumer does not replay");
    assert_eq!(hits(), before);
    drop(prepared);

    let bundle = ATTEMPT.with(|slot| slot.borrow_mut().take().unwrap());
    let lock = crate::cargo::resolution::layout::Paths::observe(bundle.clone())
        .unwrap()
        .final_form()
        .lock()
        .to_path_buf();
    let frozen = fs::metadata(&lock).unwrap().permissions();
    // Same bytes, new status-change time: the next consumer replays in full.
    fs::set_permissions(&lock, frozen.clone()).unwrap();
    let prepared = node::Chain::validated(context.clone(), cold.node())
        .prepare_read_set()
        .unwrap();
    assert_eq!(validates(), 1, "a changed stamp replays once");
    assert_eq!(
        hits() - before,
        3,
        "the replay reuses lock, receipt and the decoded record"
    );
    drop(prepared);
    let mut drifted = fs::read(&lock).unwrap();
    *drifted.last_mut().unwrap() ^= 1;
    fs::set_permissions(&lock, fs::Permissions::from_mode(0o600)).unwrap();
    fs::write(&lock, drifted).unwrap();
    fs::set_permissions(&lock, frozen).unwrap();
    let before = hits();
    let refused = node::Chain::validated(context.clone(), cold.node())
        .prepare_read_set()
        .err()
        .expect("a staged parse never certifies a drifted bundle lock");
    // Current control hashing classifies the changed member as positive bundle
    // corruption before any staged value could be consulted.
    assert!(
        matches!(refused, node::Error::CandidateCorrupt(_)),
        "{refused:?}"
    );
    assert_eq!(hits(), before, "drift is refused before staged reuse");
    drop(context);
    store.retire_fixture().unwrap();
    fs::remove_dir_all(root).unwrap();
}

fn validates() -> usize {
    owner::REPLAYS.with(Cell::get)
}

/// A stored resolution whose recorded inputs changed misses and is
/// recaptured, whether the change comes before the next run or between the
/// run's replay and the use of its result (#609, D664).
#[cfg(unix)]
#[test]
fn changed_inputs_recapture_the_stored_resolution() {
    let Fixture {
        root,
        request,
        context,
        store,
    } = api_continuation_fixture(&Limits::standard());
    let limits = Limits::standard();
    let universe = request.universe.clone();
    let cached = cargo::CachedToolchain {
        context: context.clone(),
        attestation: context.cached_toolchain().unwrap(),
    };
    let cold = owner::evaluate_scripted(request, cached, &limits, &crate::metrics::Current, script)
        .unwrap();
    let action = cold.node().receipt().action();
    drop(cold);
    let source = root.join("rawr-iced/src/lib.rs");
    let change = |text: &[u8]| {
        let mut bytes = fs::read(&source).unwrap();
        bytes.extend_from_slice(text);
        fs::write(&source, bytes).unwrap();
    };
    let reopen = || {
        let mut request = Fixture::request(&root, context.environment().clone());
        request.universe = universe.clone();
        let fresh = crate::context::Context::capture_config(
            context.project(),
            &request.config,
            context.target(),
            &limits,
        )
        .unwrap();
        cargo::toolchain::Attestation::fixture_replay(&fresh);
        let cached = cargo::CachedToolchain {
            attestation: fresh.cached_toolchain().unwrap(),
            context: fresh.clone(),
        };
        (fresh, request, cached)
    };
    change(b"// changed before the run\n");
    let (first, request, cached) = reopen();
    let rebuilt =
        owner::evaluate_scripted(request, cached, &limits, &crate::metrics::Current, script)
            .expect("a changed input is a miss, rebuilt");
    assert_ne!(rebuilt.node().receipt().action(), action);
    let action = rebuilt.node().receipt().action();
    // The input changes again after this run's replay and before its result
    // is used: the use says the entry is stale, and the run starts again.
    change(b"// changed during the run\n");
    let stale = rebuilt
        .publish(&limits)
        .err()
        .expect("a stale replay is never used");
    assert!(stale.is_stale(), "{stale:?}");
    drop(first);
    let (second, request, cached) = reopen();
    let again =
        owner::evaluate_scripted(request, cached, &limits, &crate::metrics::Current, script)
            .expect("the run started again rebuilds the entry");
    assert_ne!(again.node().receipt().action(), action);
    again
        .publish(&limits)
        .expect("the rebuilt entry is current");
    drop(second);
    drop(context);
    store.retire_fixture().unwrap();
    fs::remove_dir_all(root).unwrap();
}

/// A later consumer in one graph reuses the staged record interpretation
/// without decoding any record wire, and so does a fresh graph, which takes
/// the last replay's comparison of the inputs whole while every stamp it took
/// holds, and compares again once one moves (#1759).
#[cfg(unix)]
#[test]
fn staged_owner_record_and_comparison_are_kept_across_graphs_until_a_stamp_moves() {
    let Fixture {
        root,
        request,
        context,
        store,
    } = api_continuation_fixture(&Limits::standard());
    let limits = Limits::standard();
    let universe = request.universe.clone();
    let cached = cargo::CachedToolchain {
        context: context.clone(),
        attestation: context.cached_toolchain().unwrap(),
    };
    owner::REPLAYS.with(|replays| replays.set(0));
    let hits = || owner::STAGE_HITS.with(Cell::get);
    let cold = owner::evaluate_scripted(request, cached, &limits, &crate::metrics::Current, script)
        .unwrap();
    let cold_validates = validates();
    let before = hits();
    let prepared = node::Chain::validated(context.clone(), cold.node())
        .prepare_read_set()
        .unwrap();
    assert_eq!(
        validates(),
        cold_validates,
        "an unchanged consumer does not replay"
    );
    drop(prepared);
    let bundle = ATTEMPT.with(|slot| slot.borrow().clone().unwrap());
    let record = crate::cargo::resolution::layout::Paths::observe(bundle.clone())
        .unwrap()
        .record()
        .to_path_buf();
    fs::set_permissions(&record, fs::metadata(&record).unwrap().permissions()).unwrap();
    let prepared = node::Chain::validated(context.clone(), cold.node())
        .prepare_read_set()
        .unwrap();
    assert_eq!(
        hits() - before,
        3,
        "the consumer after a new stamp reuses lock, receipt and the decoded record"
    );
    assert_eq!(
        validates(),
        cold_validates + 1,
        "one changed stamp, one replay"
    );
    drop(prepared);
    drop(cold);

    // A fresh Context opens a fresh graph, as each check of a running owner
    // does. What this process kept answers its replays: no record is decoded
    // and no input read again (#1759).
    let warm = |universe: &Option<cargo::universe::Catalog>| {
        let mut request = Fixture::request(&root, context.environment().clone());
        request.universe = universe.clone();
        let fresh = crate::context::Context::capture_config(
            context.project(),
            &request.config,
            context.target(),
            &limits,
        )
        .unwrap();
        cargo::toolchain::Attestation::fixture_replay(&fresh);
        let cached = cargo::CachedToolchain {
            attestation: fresh.cached_toolchain().unwrap(),
            context: fresh.clone(),
        };
        let first = validates();
        let before = hits();
        let views = crate::cargo::resolution::replay::VIEWS.with(Cell::get);
        let warm =
            owner::evaluate_scripted(request, cached, &limits, &crate::metrics::Current, no_cold)
                .unwrap();
        drop(warm);
        drop(fresh);
        let after = crate::cargo::resolution::replay::VIEWS.with(Cell::get);
        (
            validates() - first,
            hits() - before,
            [0, 1, 2].map(|view| after[view] - views[view]),
        )
    };
    let (replays, staged, views) = warm(&universe);
    assert!(replays > 0, "the warm graph replays the owner");
    assert_eq!(
        staged,
        3 * replays,
        "every replay of the warm graph reuses the staged values"
    );
    assert_eq!(views, [0, 0, replays], "parsed, compared, kept");

    // The same bytes written again move a stamp: the workspace manifest the
    // request names, and the bundle's lock. The next graph compares its
    // inputs again, and decodes nothing.
    let manifest = root.join("authority/Cargo.toml");
    fs::write(&manifest, fs::read(&manifest).unwrap()).unwrap();
    let (_, _, views) = warm(&universe);
    assert_eq!(
        views[..2],
        [0, 1],
        "the manifest's new stamp compares again"
    );
    let lock = crate::cargo::resolution::layout::Paths::observe(bundle)
        .unwrap()
        .final_form()
        .lock()
        .to_path_buf();
    fs::set_permissions(&lock, fs::metadata(&lock).unwrap().permissions()).unwrap();
    let (_, _, views) = warm(&universe);
    assert_eq!(views[..2], [0, 1], "the lock's new stamp compares again");
    let (replays, _, views) = warm(&universe);
    assert_eq!(views, [0, 0, replays], "and then keeps it");
    drop(context);
    store.retire_fixture().unwrap();
    fs::remove_dir_all(root).unwrap();
}

/// Record drift between replays, whether a same-length change, a
/// truncation or an extension, is refused at the next consumer as positive
/// bundle corruption before any staged interpretation could be consulted.
#[cfg(unix)]
#[test]
fn staged_owner_record_refuses_record_drift_between_replays() {
    use std::os::unix::fs::PermissionsExt as _;

    type Drift = fn(&mut Vec<u8>);
    let drifts: [(&str, Drift); 3] = [
        ("same-length change", |bytes| {
            *bytes.last_mut().unwrap() ^= 1
        }),
        ("truncation", |bytes| {
            bytes.pop();
        }),
        ("extension", |bytes| bytes.push(0)),
    ];
    for (name, drift) in drifts {
        let Fixture {
            root,
            request,
            context,
            store,
        } = api_continuation_fixture(&Limits::standard());
        let limits = Limits::standard();
        let cached = cargo::CachedToolchain {
            context: context.clone(),
            attestation: context.cached_toolchain().unwrap(),
        };
        let hits = || owner::STAGE_HITS.with(Cell::get);
        let cold =
            owner::evaluate_scripted(request, cached, &limits, &crate::metrics::Current, script)
                .unwrap();
        let before = hits();
        let prepared = node::Chain::validated(context.clone(), cold.node())
            .prepare_read_set()
            .unwrap();
        assert_eq!(
            hits(),
            before,
            "{name}: an unchanged consumer does not replay"
        );
        drop(prepared);

        let bundle = ATTEMPT.with(|slot| slot.borrow_mut().take().unwrap());
        let record = crate::cargo::resolution::layout::Paths::observe(bundle.clone())
            .unwrap()
            .record()
            .to_path_buf();
        let frozen = fs::metadata(&record).unwrap().permissions();
        let original = fs::read(&record).unwrap();
        let mut drifted = original.clone();
        drift(&mut drifted);
        assert_ne!(drifted, original);
        fs::set_permissions(&record, fs::Permissions::from_mode(0o600)).unwrap();
        fs::write(&record, &drifted).unwrap();
        fs::set_permissions(&record, frozen).unwrap();
        let before = hits();
        let refused = node::Chain::validated(context.clone(), cold.node())
            .prepare_read_set()
            .err()
            .unwrap_or_else(|| {
                panic!("{name}: a staged interpretation never certifies a drifted record")
            });
        assert!(
            matches!(refused, node::Error::CandidateCorrupt(_)),
            "{name}: {refused:?}"
        );
        assert_eq!(
            hits(),
            before,
            "{name}: drift is refused before staged reuse"
        );
        drop(cold);
        drop(context);
        store.retire_fixture().unwrap();
        fs::remove_dir_all(root).unwrap();
    }
}

struct Suffix {
    exact_input: [u8; 32],
    semantic: [[u8; 32]; 8],
    contents: [node::Digest; 6],
    actions: [node::ActionKey; 6],
    /// Every generated file's exact bytes, by path.
    files: Vec<(Box<str>, Box<[u8]>)>,
    #[cfg(feature = "test-fixtures")]
    artifact: (node::ActionKey, PathBuf, [u8; 32]),
}

fn derived_suffix(
    completed: crate::composition::Completed<crate::checked::Retained>,
    adapter: &crate::profile::trusted::Adapter<ApiProfile>,
    source: crate::surface::Snapshot,
    limits: &Limits,
    expect: Expect,
    owner: node::ActionKey,
) -> Suffix {
    let recorder = crate::metrics::recorder::Recorder::transform_at(100_000, 1).unwrap();
    let scope = recorder.scope().unwrap();
    let crate::composition::Completed {
        contract,
        profile,
        facade,
        checked,
    } = completed;
    assert_eq!(
        contract.raw_fingerprint(),
        profile.graph_fingerprint(),
        "profile is rebound to the exact current API"
    );
    let generated =
        crate::composition::profile::prepare_retained(adapter, &profile, &checked).unwrap();
    let symbolic = checked
        .replay_demand((), &contract, source, limits)
        .unwrap()
        .with_trusted_runner(false, &facade, &contract)
        .unwrap();
    let contents = [
        contract.node_chain().unwrap().parent().content(),
        profile.node_chain().unwrap().parent().content(),
        facade.node_chain().parent().content(),
        owned_identity(&checked.product).1,
        owned_identity(&generated.demand).1,
        owned_identity(&generated.generated).1,
    ];
    let actions = [
        contract.node_chain().unwrap().parent().action(),
        profile.node_chain().unwrap().parent().action(),
        facade.node_chain().parent().action(),
        owned_identity(&checked.product).0,
        owned_identity(&generated.demand).0,
        owned_identity(&generated.generated).0,
    ];
    let mut semantic = [[0; 32]; 8];
    semantic[0] = *contract.fingerprint().as_bytes();
    semantic[1] = *profile.rules_digest();
    semantic[2] = checked.checked_fingerprint();
    semantic[3] = *generated.demand_digest();
    semantic[4] = *generated.fingerprint();
    drop(scope);
    // Ordinary callbacks can observe exact identity even when the separately
    // audited source path reuses all of its semantic products.
    let observed_raw = *contract.raw_fingerprint().as_bytes();
    let ordinary = crate::checked::evaluate(&contract, &profile, &facade, source, || {
        Ok::<_, ()>((
            contract
                .compiler((), source, limits)
                .unwrap()
                .seal()
                .unwrap(),
            RawChecked(*contract.raw_fingerprint().as_bytes()),
        ))
    })
    .unwrap();
    assert_eq!(ordinary.checked_payload(), &observed_raw);
    let scope = recorder.scope().unwrap();
    let native = contract.into_native_proof();
    let exact_input = *native.fingerprint().as_bytes();
    semantic[5] = *native.native().unwrap().as_bytes();
    let request = symbolic
        .request_retained(
            native,
            generated,
            crate::supervisor::Owner::new().revision().unwrap(),
        )
        .unwrap();
    semantic[6] = *request.build_request_key().as_bytes();
    let manifest = request.generate().unwrap();
    semantic[7] = manifest.generated_manifest().digest();
    let files = manifest
        .files
        .iter()
        .map(|file| (Box::from(file.path()), Box::from(file.bytes())))
        .collect();
    assert!(manifest
        .chain
        .as_ref()
        .unwrap()
        .parent()
        .read_set()
        .contains(owner));
    #[cfg(feature = "test-fixtures")]
    let artifact = {
        let built = crate::runner::fixture_build(manifest).unwrap();
        assert!(built
            .chain
            .as_ref()
            .unwrap()
            .parent()
            .read_set()
            .contains(owner));
        (
            built.chain.as_ref().unwrap().parent().action(),
            built.artifact().executable().to_path_buf(),
            built.artifact().digest(),
        )
    };
    drop(scope);
    if expect != Expect::Cold {
        // Both the repair and the warm graph reuse the cold runner.
        assert_suffix_hits(recorder, true);
    }
    Suffix {
        exact_input,
        semantic,
        contents,
        actions,
        files,
        #[cfg(feature = "test-fixtures")]
        artifact,
    }
}

fn owned_identity<K: node::Value>(
    owned: &node::staging::Owned<K>,
) -> (node::ActionKey, node::Digest) {
    match owned {
        node::staging::Owned::Current(value) => {
            (value.receipt().action(), value.content().digest())
        }
        node::staging::Owned::Staged(value) => (value.action(), value.content().digest()),
    }
}

fn assert_prefix_hits(recorder: crate::metrics::recorder::Recorder) {
    use crate::metrics::{NodeEvent, NodeKind};
    let evidence = recorder.snapshot().unwrap();
    assert!(evidence.processes.iter().all(|row| row.count == 0));
    for kind in [
        NodeKind::Generic,
        NodeKind::Profile,
        NodeKind::Facade,
        NodeKind::Checked,
    ] {
        assert_eq!(
            evidence
                .nodes
                .iter()
                .find(|row| row.kind == kind && row.event == NodeEvent::ExecutedMiss)
                .unwrap()
                .count,
            0,
            "{kind:?} must reuse content"
        );
        assert!(
            evidence
                .nodes
                .iter()
                .find(|row| row.kind == kind && row.event == NodeEvent::ValidatedHit)
                .unwrap()
                .count
                > 0,
            "{kind:?} must report a hit"
        );
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Expect {
    Cold,
    Repair,
    Warm,
}

// The owner path folds the native build into the resolution bundle; the
// separate cached NativeProduct node is not evaluated here.
fn assert_suffix_hits(recorder: crate::metrics::recorder::Recorder, runner_reused: bool) {
    use crate::metrics::{NodeEvent, NodeKind};
    let evidence = recorder.snapshot().unwrap();
    assert!(evidence.processes.iter().all(|row| row.count == 0));
    assert!(
        evidence
            .nodes
            .iter()
            .filter(|row| row.kind == NodeKind::NativeProduct)
            .all(|row| row.count == 0),
        "the owner path never evaluates the separate cached NativeProduct node"
    );
    for kind in [NodeKind::ProfileProof, NodeKind::Overlay] {
        assert_eq!(
            evidence
                .nodes
                .iter()
                .find(|row| row.kind == kind && row.event == NodeEvent::ExecutedMiss)
                .unwrap()
                .count,
            0,
            "{kind:?} must reuse content"
        );
        assert!(
            evidence
                .nodes
                .iter()
                .find(|row| row.kind == kind && row.event == NodeEvent::ValidatedHit)
                .unwrap()
                .count
                > 0,
            "{kind:?} must report a hit"
        );
    }
    #[cfg(feature = "test-fixtures")]
    {
        let runner = |event| {
            evidence
                .nodes
                .iter()
                .find(|row| row.kind == NodeKind::Runner && row.event == event)
                .unwrap()
                .count
        };
        let (miss, hit, rejected) = if runner_reused { (0, 1, 0) } else { (1, 0, 1) };
        assert_eq!(runner(NodeEvent::ExecutedMiss), miss);
        assert_eq!(runner(NodeEvent::ValidatedHit), hit);
        assert_eq!(runner(NodeEvent::RejectedHit), rejected);
    }
    #[cfg(not(feature = "test-fixtures"))]
    let _ = runner_reused;
}

struct ApiProfile;
impl crate::profile::trusted::Operations for ApiProfile {
    fn classify<P: crate::api::Phase>(
        &self,
        _: &crate::api::Graph<P>,
        _: &mut crate::profile::contract::Builder<'_, P>,
    ) -> std::result::Result<(), crate::profile::Error> {
        Ok(())
    }
    fn demand(
        &self,
        _: crate::profile::trusted::Contract<'_>,
        _: crate::profile::trusted::Checked<'_>,
        _: &mut crate::profile::demand::Builder<'_>,
    ) -> std::result::Result<(), crate::profile::Error> {
        Ok(())
    }
    fn generate(
        &self,
        _: crate::profile::trusted::Demand<'_>,
        _: &mut crate::profile::demand::Generator<'_>,
    ) -> std::result::Result<(), crate::profile::Error> {
        Ok(())
    }
}
struct RawChecked([u8; 32]);
impl crate::profile::Checked for RawChecked {
    fn profile_fingerprint(&self) -> [u8; 32] {
        self.0
    }
    fn profile_payload(&self) -> &[u8] {
        &self.0
    }
}

struct ApiChecked;
impl crate::profile::Checked for ApiChecked {
    fn profile_fingerprint(&self) -> [u8; 32] {
        [9; 32]
    }
    fn profile_payload(&self) -> &[u8] {
        &[9; 32]
    }
}
fn api_adapter() -> crate::profile::trusted::Adapter<ApiProfile> {
    crate::profile::trusted::Adapter::trusted(
        ApiProfile,
        crate::profile::Identity::new("api.continuation.fixture"),
        crate::profile::Version::new(1),
        false,
    )
    .semantic_api()
}
fn api_source(limits: &Limits) -> crate::surface::Snapshot {
    let mut source = crate::surface::Snapshot::builder(limits);
    source.entry("/fixture/Cargo.toml").unwrap();
    source.file("/fixture/main.rs", b"let value = 1;").unwrap();
    source.seal().unwrap()
}

#[test]
fn retained_api_composes_actual_owner_then_reuses_without_children() {
    let Fixture {
        root,
        request,
        context,
        store,
    } = api_continuation_fixture(&Limits::standard());
    let limits = Limits::standard();
    let universe = request.universe.clone();
    let cached = || cargo::CachedToolchain {
        context: context.clone(),
        attestation: context.cached_toolchain().unwrap(),
    };
    let api =
        owner::evaluate_scripted(request, cached(), &limits, &crate::metrics::Current, script)
            .unwrap()
            .retain_api(&limits)
            .unwrap();
    assert!(
        matches!(&api.product, node::staging::Owned::Staged(_)),
        "cold API does not promote before classification"
    );
    let source = api_source(&limits);
    let adapter = api_adapter();
    let completed = crate::composition::evaluate_api_semantic(
        api,
        &adapter,
        &crate::classify::Policy::standard(),
        source,
        &limits,
        |contract, _, _| {
            Ok::<_, ()>((
                contract
                    .compiler((), source, &limits)
                    .unwrap()
                    .seal()
                    .unwrap(),
                ApiChecked,
            ))
        },
    )
    .unwrap();
    let mut request = Fixture::request(&root, context.environment().clone());
    request.universe = universe;
    test_launch::Script::new(
        root.clone(),
        test_launch::FileLimits { count: 0, bytes: 0 },
        Vec::new(),
    )
    .unwrap()
    .run(|| {
        let warm = owner::evaluate_scripted(
            request,
            cached(),
            &limits,
            &crate::metrics::Current,
            no_cold,
        )
        .unwrap()
        .retain_api(&limits)
        .unwrap();
        assert!(matches!(&warm.product, node::staging::Owned::Current(_)));
        let warm = crate::composition::evaluate_api_semantic(
            warm,
            &adapter,
            &crate::classify::Policy::standard(),
            source,
            &limits,
            |_, _, _| -> std::result::Result<(crate::Demand<crate::Sealed, ()>, ApiChecked), ()> {
                panic!("warm source must not run")
            },
        )
        .unwrap();
        assert_eq!(
            warm.contract.fingerprint(),
            completed.contract.fingerprint()
        );
        assert_eq!(
            warm.checked.checked_payload(),
            completed.checked.checked_payload()
        );
    })
    .unwrap();
    drop(completed);
    drop(context);
    store.retire_fixture().unwrap();
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn resolved_warm_launch_equals_cold_without_the_api_graph() {
    let Fixture {
        root,
        request,
        context,
        store,
    } = api_continuation_fixture(&Limits::standard());
    let limits = Limits::standard();
    let universe = request.universe.clone();
    let cached = || cargo::CachedToolchain {
        context: context.clone(),
        attestation: context.cached_toolchain().unwrap(),
    };
    let policy = crate::classify::Policy::standard();
    let adapter = api_adapter();
    let source = api_source(&limits);
    // This owner exposes no demandable item; plan round trips are covered
    // by the composition fixture (`resolved_plans_replay_like_the_contract`).
    let seal = |contract: &crate::Contract<crate::Classified>, source| {
        contract
            .compiler((), source, &limits)
            .unwrap()
            .seal()
            .unwrap()
    };
    let retain = |request, script| {
        owner::evaluate_scripted(request, cached(), &limits, &crate::metrics::Current, script)
            .unwrap()
            .retain_api(&limits)
            .unwrap()
    };
    // The closure passes on resolve_api_semantic's result, whose Err is the unresolved API itself.
    #[allow(clippy::result_large_err)]
    let resolve = |api, source| {
        crate::composition::resolve_api_semantic(api, &adapter, &policy, source, &limits, |_| {
            Ok::<_, ()>(())
        })
        .unwrap()
    };
    // Cold: no resolved product yet; the full path runs and records it.
    let Err(api) = resolve(retain(request, script), source) else {
        panic!("a cold launch has no resolved demand");
    };
    let crate::composition::Completed {
        contract,
        profile,
        facade,
        checked,
    } = crate::composition::evaluate_api_semantic(
        api,
        &adapter,
        &policy,
        source,
        &limits,
        |contract, _, _| Ok::<_, ()>((seal(contract, source), ApiChecked)),
    )
    .unwrap();
    let cold = checked
        .replay_demand((), &contract, source, &limits)
        .unwrap()
        .with_trusted_runner(true, &facade, &contract)
        .unwrap();
    crate::composition::record_resolved(&contract, &cold, &adapter, &policy).unwrap();
    let plans = serde_json::to_vec(cold.adapter_plans()).unwrap();
    let native = contract.into_native_proof();
    let request = || {
        let mut request = Fixture::request(&root, context.environment().clone());
        request.universe = universe.clone();
        request
    };
    test_launch::Script::new(
        root.clone(),
        test_launch::FileLimits { count: 0, bytes: 0 },
        Vec::new(),
    )
    .unwrap()
    .run(|| {
        // Warm: unchanged inputs hit, and the deferred graph rebuild never runs.
        let mut api = retain(request(), no_cold);
        assert!(matches!(api.graph, crate::api::retained::Lazy::Pending(_)));
        api.graph = crate::api::retained::Lazy::Pending(Box::new(|_| {
            panic!("a resolved warm launch must not rebuild the API graph")
        }));
        let started = std::time::Instant::now();
        let Ok(warm) = resolve(api, source) else {
            panic!("unchanged inputs reuse the resolved demand");
        };
        eprintln!("resolved warm hit: {:?}", started.elapsed());
        assert_eq!(warm.demand.canonical_bytes(), cold.canonical_bytes());
        assert_eq!(warm.demand.fingerprint(), cold.fingerprint());
        assert_eq!(
            warm.demand.contract_fingerprint(),
            cold.contract_fingerprint()
        );
        assert_eq!(
            warm.demand.cached_retained_usage(),
            cold.cached_retained_usage()
        );
        assert_eq!(
            serde_json::to_vec(warm.demand.adapter_plans()).unwrap(),
            plans
        );
        assert_eq!(warm.profile.rules_digest(), profile.rules_digest());
        assert_eq!(warm.facade.bytes(), facade.bytes());
        assert_eq!(warm.checked.checked_payload(), checked.checked_payload());
        assert_eq!(warm.native.fingerprint(), native.fingerprint());
        assert_eq!(
            warm.native.native().unwrap().as_bytes(),
            native.native().unwrap().as_bytes()
        );
        assert!(warm.native.watch().eq(native.watch()));
        let (warm, cold) = (warm.demand.into_request(), cold.into_request());
        assert_eq!(
            crate::bridge::generate::semantic_digest(&warm).unwrap(),
            crate::bridge::generate::semantic_digest(&cold).unwrap()
        );
        // A changed Rawr source (a new demand) misses and keeps the full path.
        let mut edit = crate::surface::Snapshot::builder(&limits);
        edit.entry("/fixture/Cargo.toml").unwrap();
        edit.file("/fixture/main.rs", b"let value = 2;").unwrap();
        assert!(resolve(retain(request(), no_cold), edit.seal().unwrap()).is_err());
    })
    .unwrap();
    drop((profile, facade, checked, native));
    drop(context);
    store.retire_fixture().unwrap();
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn retained_api_rejects_owner_drift_before_source_callback() {
    let Fixture {
        root,
        request,
        context,
        store,
    } = api_continuation_fixture(&Limits::standard());
    let limits = Limits::standard();
    let cached = cargo::CachedToolchain {
        context: context.clone(),
        attestation: context.cached_toolchain().unwrap(),
    };
    let api = owner::evaluate_scripted(request, cached, &limits, &crate::metrics::Current, script)
        .unwrap()
        .retain_api(&limits)
        .unwrap();
    fs::write(
        root.join("authority/.cargo/config.toml"),
        b"[net]\noffline = false\n",
    )
    .unwrap();
    assert!(crate::composition::evaluate_api(
        api,
        &api_adapter(),
        &crate::classify::Policy::standard(),
        api_source(&limits),
        &limits,
        |_, _, _| -> std::result::Result<(crate::Demand<crate::Sealed, ()>, ApiChecked), ()> {
            panic!("owner drift must reject before whole source callback")
        }
    )
    .is_err());
    drop(context);
    store.retire_fixture().unwrap();
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn retained_api_rejects_foreign_composition_context() {
    let Fixture {
        root,
        request,
        context,
        store,
    } = api_continuation_fixture(&Limits::standard());
    let other = Fixture::open();
    let limits = Limits::standard();
    let cached = cargo::CachedToolchain {
        context: context.clone(),
        attestation: context.cached_toolchain().unwrap(),
    };
    let mut api =
        owner::evaluate_scripted(request, cached, &limits, &crate::metrics::Current, script)
            .unwrap()
            .retain_api(&limits)
            .unwrap();
    api.publication.context = other.context.clone();
    assert!(crate::composition::evaluate_api(
        api,
        &api_adapter(),
        &crate::classify::Policy::standard(),
        api_source(&limits),
        &limits,
        |_, _, _| -> std::result::Result<(crate::Demand<crate::Sealed, ()>, ApiChecked), ()> {
            panic!("foreign account must reject before callback")
        }
    )
    .is_err());
    drop(context);
    store.retire_fixture().unwrap();
    fs::remove_dir_all(root).unwrap();
    drop(other.context);
    other.store.retire_fixture().unwrap();
    fs::remove_dir_all(other.root).unwrap();
}
