use std::fs::{self, OpenOptions};
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{mpsc, Arc};
use std::time::{Duration, Instant};

use super::{fixture_context, prepare_fixture, temporary, Artifact, Counts};
use crate::cargo::execution;
use crate::node::{Error, Evaluator};

fn locked(path: &Path) -> bool {
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .open(path)
        .expect("the admitted lease exists");
    let mut lease = fd_lock::RwLock::new(file);
    match lease.try_write() {
        Ok(_) => false,
        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => true,
        Err(error) => panic!("lease probe failed: {error}"),
    }
}

fn await_locked(path: &Path) -> bool {
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        if locked(path) {
            return true;
        }
        std::thread::sleep(Duration::from_millis(1));
    }
    false
}

fn value() -> Result<Artifact, Error> {
    Ok(Artifact(b"native-output".to_vec().into_boxed_slice()))
}

#[test]
fn native_waits_for_heavy_before_action_and_releases_publication_during_execution() {
    let root = temporary("native-order");
    let context = prepare_fixture(&root);
    let store = context.open_store().expect("the order store opens");
    let recipe = context
        .direct::<Artifact>(b"native-order", 4_096)
        .expect("recipe seals");
    let heavy = store.root().join("locks/heavy.lock");
    let action = store
        .root()
        .join(format!("locks/actions/{}.lock", recipe.action()));
    let publication = store.root().join("locks/publication.lock");
    let (sent, received) = mpsc::channel();
    let counts = Counts::default();
    std::thread::scope(|threads| {
        store
            .with_action_lease(recipe.action(), || {
                threads.spawn(|| {
                    let result = Evaluator::new(&store, &counts).native(&recipe, || {
                        assert!(locked(&heavy));
                        assert!(locked(&action));
                        assert!(!locked(&publication), "native work cannot hold publication");
                        value()
                    });
                    sent.send(result.is_ok())
                        .expect("the evaluator reports completion");
                });
                assert!(
                    await_locked(&heavy),
                    "heavy must be owned while waiting for action"
                );
                assert!(matches!(
                    received.try_recv(),
                    Err(mpsc::TryRecvError::Empty)
                ));
                Ok(())
            })
            .expect("the blocking action lease releases");
        assert!(received
            .recv_timeout(Duration::from_secs(5))
            .expect("native action completes"));
    });
    assert_eq!(counts.miss.load(Ordering::Relaxed), 1);
    assert_eq!(counts.published.load(Ordering::Relaxed), 1);
    assert!(!locked(&heavy));
    assert!(!locked(&action));
    fs::remove_dir_all(root).expect("the native order fixture retires");
}

#[cfg(feature = "test-fixtures")]
#[test]
fn retained_bundle_rejects_parent_drift_after_execution_wait() {
    use crate::node::{read_set::Retention, Parent, Value as _};
    use std::sync::atomic::AtomicBool;

    let root = temporary("retained-execution-wait");
    let context = prepare_fixture(&root);
    let store = context.open_store().unwrap();
    let counts = Counts::default();
    let reject = Arc::new(AtomicBool::new(false));
    let refreshes = Arc::new(AtomicUsize::new(0));
    let rejected = Arc::clone(&reject);
    let observed = Arc::clone(&refreshes);
    let parent = Evaluator::new(&store, &counts)
        .discover(
            &context.discover::<Artifact>(b"wait-parent", 4096).unwrap(),
            move |value| {
                observed.fetch_add(1, Ordering::Relaxed);
                if rejected.load(Ordering::Relaxed) {
                    return Err(Error::State("parent changed while waiting"));
                }
                value.encode(4096)
            },
            value,
        )
        .unwrap();
    let retention = Retention::from_validated(&parent, None).unwrap();
    let recipe = context
        .discover_with_parents::<super::BundleValue>(
            b"wait-child",
            4096,
            &[Parent::validated(&parent)],
        )
        .unwrap();
    let validation_store = store.clone();
    let heavy = store.root().join("locks/heavy.lock");
    let before = refreshes.load(Ordering::Relaxed);
    let entered = AtomicBool::new(false);
    let launched = AtomicBool::new(false);
    let pins = store.fixture_pin_counts().unwrap();
    let (sent, received) = mpsc::channel();
    std::thread::scope(|threads| {
        store
            .with_execution_lease(recipe.execution(), || {
                threads.spawn(|| {
                    let result = Evaluator::new(&store, &counts)
                        .discover_bundled_retained_budgeted(
                            &recipe,
                            &retention,
                            move |candidate, _| candidate.validate_published(&validation_store),
                            |attempt| {
                                entered.store(true, Ordering::Relaxed);
                                recipe.revalidate_parents(&store)?;
                                launched.store(true, Ordering::Relaxed);
                                super::bundled_fixture(attempt)
                            },
                        );
                    sent.send(result.is_err()).unwrap();
                });
                assert!(
                    await_locked(&heavy),
                    "evaluator holds heavy while execution is blocked"
                );
                assert_eq!(refreshes.load(Ordering::Relaxed), before);
                assert!(!entered.load(Ordering::Relaxed));
                reject.store(true, Ordering::Relaxed);
                Ok(())
            })
            .unwrap();
        assert!(received.recv_timeout(Duration::from_secs(5)).unwrap());
    });
    assert!(
        entered.load(Ordering::Relaxed),
        "retained storage ownership permits the consumer check"
    );
    assert!(!launched.load(Ordering::Relaxed));
    assert_eq!(refreshes.load(Ordering::Relaxed) - before, 1);
    assert_eq!(store.fixture_pin_counts().unwrap(), pins);
    assert_eq!(
        crate::store::bundle::count_bundles(store.root()).unwrap(),
        0
    );
    store.retire_fixture().unwrap();
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn native_same_key_contenders_execute_once_and_different_heavy_actions_cannot_overlap() {
    let root = temporary("native-contenders");
    let context = prepare_fixture(&root);
    let other = fixture_context(&root);
    let first_store = context.open_store().expect("first store opens");
    let other_store = other.open_store().expect("other store opens");
    let recipe = context
        .direct::<Artifact>(b"same-native", 4_096)
        .expect("first recipe seals");
    let different = other
        .direct::<Artifact>(b"different-native", 4_096)
        .expect("different recipe seals");
    let counts = Counts::default();
    let executed = AtomicUsize::new(0);
    let active = AtomicUsize::new(0);
    let (entered, entry) = mpsc::channel();
    let (release, released) = mpsc::channel();
    let (done, completed) = mpsc::channel();
    std::thread::scope(|threads| {
        let active_first = &active;
        let executed_first = &executed;
        let first_store = &first_store;
        let counts_first = &counts;
        let recipe_first = &recipe;
        threads.spawn(move || {
            Evaluator::new(first_store, counts_first)
                .native(recipe_first, || {
                    assert_eq!(active_first.fetch_add(1, Ordering::SeqCst), 0);
                    executed_first.fetch_add(1, Ordering::Relaxed);
                    entered.send(()).expect("first executor announces entry");
                    released.recv().expect("first executor is released");
                    active_first.fetch_sub(1, Ordering::SeqCst);
                    value()
                })
                .expect("first native action publishes");
        });
        entry
            .recv_timeout(Duration::from_secs(5))
            .expect("first executor holds heavy");
        for request in [&recipe, &different] {
            let done = done.clone();
            let counts = &counts;
            let active = &active;
            let executed = &executed;
            let other_store = &other_store;
            threads.spawn(move || {
                let result = Evaluator::new(other_store, counts).native(request, || {
                    assert_eq!(active.fetch_add(1, Ordering::SeqCst), 0);
                    executed.fetch_add(1, Ordering::Relaxed);
                    active.fetch_sub(1, Ordering::SeqCst);
                    value()
                });
                done.send(result.is_ok())
                    .expect("contender reports completion");
            });
        }
        let early = completed.recv_timeout(Duration::from_millis(100));
        release.send(()).expect("first executor can finish");
        assert!(
            matches!(early, Err(mpsc::RecvTimeoutError::Timeout)),
            "both contenders wait for heavy"
        );
        for _ in 0..2 {
            assert!(completed
                .recv_timeout(Duration::from_secs(5))
                .expect("contender completes"));
        }
    });
    assert_eq!(executed.load(Ordering::Relaxed), 2);
    assert_eq!(counts.miss.load(Ordering::Relaxed), 2);
    assert_eq!(counts.published.load(Ordering::Relaxed), 2);
    assert_eq!(counts.hit.load(Ordering::Relaxed), 1);
    fs::remove_dir_all(root).expect("the native contention fixture retires");
}

#[test]
fn healthy_native_hits_and_unrelated_pure_work_do_not_wait_for_heavy() {
    let root = temporary("native-hit");
    let context = prepare_fixture(&root);
    let store = context.open_store().expect("the hit store opens");
    let warm = context
        .direct::<Artifact>(b"warm-native", 4_096)
        .expect("warm recipe seals");
    Evaluator::new(&store, &Counts::default())
        .native(&warm, value)
        .expect("warm value seeds");
    let pure = context
        .direct::<Artifact>(b"pure-independent", 4_096)
        .expect("pure recipe seals");
    let counts = Counts::default();
    let (sent, received) = mpsc::channel();
    std::thread::scope(|threads| {
        let result = store
            .with_heavy_lease(|| {
                threads.spawn(|| {
                    let evaluator = Evaluator::new(&store, &counts);
                    let hit = evaluator.native(&warm, || panic!("healthy hit cannot execute"));
                    let pure = evaluator.direct(&pure, value);
                    sent.send(hit.is_ok() && pure.is_ok())
                        .expect("independent work reports");
                });
                Ok(received.recv_timeout(Duration::from_secs(5)))
            })
            .expect("the external heavy lease releases");
        assert!(result.expect("hit and pure miss finish while heavy is held"));
    });
    assert_eq!(counts.hit.load(Ordering::Relaxed), 1);
    assert_eq!(counts.miss.load(Ordering::Relaxed), 1);
    fs::remove_dir_all(root).expect("the native hit fixture retires");
}

#[test]
fn uncached_scope_owns_installation_and_exact_execution_and_releases_after_failure() {
    let root = temporary("native-scope");
    let context = Arc::new(prepare_fixture(&root));
    let store = context.open_store().expect("the scope store opens");
    let scope = execution::Scope::bind(&context, execution::Operation::Probe, [17; 32]);
    let failure: crate::Result<()> = scope.run(|| {
        assert!(locked(&store.root().join("locks/heavy.lock")));
        let executions = fs::read_dir(store.root().join("locks/executions"))
            .expect("execution leases enumerate")
            .map(|entry| entry.expect("execution lease reads").path())
            .collect::<Vec<_>>();
        assert_eq!(executions.len(), 1);
        assert!(locked(&executions[0]));
        assert!(!locked(&store.root().join("locks/publication.lock")));
        Err(crate::Error::Owner {
            state: "injected native failure",
        })
    });
    assert!(matches!(
        failure,
        Err(crate::Error::Owner {
            state: "injected native failure"
        })
    ));
    assert!(!locked(&store.root().join("locks/heavy.lock")));
    scope
        .run(|| Ok(()))
        .expect("failure releases both execution leases");
    let other_project = root.join("other-project");
    fs::create_dir(&other_project).expect("a second project shares the installation");
    let other_context = Arc::new(
        crate::context::Context::capture(
            &other_project,
            context.environment().clone(),
            context.target(),
            &context.limits(),
        )
        .expect("the other project context seals"),
    );
    let other_scope =
        execution::Scope::bind(&other_context, execution::Operation::Package, [19; 32]);
    let (sent, received) = mpsc::channel();
    std::thread::scope(|threads| {
        let before_release = store
            .with_heavy_lease(|| {
                threads.spawn(|| {
                    let result = other_scope.run(|| Ok(()));
                    sent.send(result.is_ok())
                        .expect("scope contender reports completion");
                });
                Ok(received.recv_timeout(Duration::from_millis(100)))
            })
            .expect("external ownership releases");
        assert!(matches!(
            before_release,
            Err(mpsc::RecvTimeoutError::Timeout)
        ));
        assert!(received
            .recv_timeout(Duration::from_secs(5))
            .expect("scope resumes after heavy"));
    });
    fs::remove_dir_all(root).expect("the native scope fixture retires");
}

// The per-graph path-check budget is an accounting target. Every native
// recheck spends the one shared account; past the target it keeps going and the
// account records the exceedance instead of refusing before execution.
#[test]
fn native_rechecks_share_the_graph_budget_and_record_exceedance_past_the_floor() {
    for floor in [false, true] {
        let root = temporary("native-budget");
        let context = prepare_fixture(&root);
        let store = context.open_store().expect("native budget store opens");
        let recipe = context
            .direct::<Artifact>(b"native-budget", 4_096)
            .expect("recipe seals");
        let budget = store.graph_budget();
        let counts = Counts::default();
        let result = Evaluator::new(&store, &counts).native(&recipe, || {
            let paths = budget.usage.lock().expect("usage reads").paths;
            assert_eq!(
                paths, 6,
                "initial, heavy, and action rechecks share one budget"
            );
            budget
                .paths(crate::node::MAX_PATH_CHECKS_PER_GRAPH - paths - 26 + usize::from(floor))?;
            value()
        });
        result.expect("the final recheck completes at and past the path target");
        assert_eq!(
            budget.usage.lock().expect("usage reads").paths,
            crate::node::MAX_PATH_CHECKS_PER_GRAPH + usize::from(floor)
        );
        if floor {
            let paths = budget
                .exceedances()
                .axis(crate::node::PATHS_RESOURCE)
                .copied()
                .expect("the one-short recheck records the path exceedance");
            assert_eq!(paths.target, crate::node::MAX_PATH_CHECKS_PER_GRAPH as u64);
            assert_eq!(
                paths.high_water,
                crate::node::MAX_PATH_CHECKS_PER_GRAPH as u64 + 1
            );
            assert!(paths.site.is_some());
            assert_eq!(budget.exceedances().axes().count(), 1);
        } else {
            assert!(!budget.exceedances().is_exceeded());
        }
        fs::remove_dir_all(root).expect("native budget fixture retires");
    }
    let root = temporary("native-before-heavy-budget");
    let context = prepare_fixture(&root);
    let store = context.open_store().expect("native floor store opens");
    let recipe = context
        .direct::<Artifact>(b"native-floor", 4_096)
        .expect("recipe seals");
    store
        .graph_budget()
        .paths(crate::node::MAX_PATH_CHECKS_PER_GRAPH - 3)
        .expect("floor precharges");
    assert!(!store.graph_budget().exceedances().is_exceeded());
    let counts = Counts::default();
    let result = Evaluator::new(&store, &counts).native(&recipe, value);
    result.expect("the heavy recheck proceeds past the path target and executes");
    assert_eq!(counts.miss.load(Ordering::Relaxed), 1);
    assert_eq!(counts.published.load(Ordering::Relaxed), 1);
    let paths = store
        .graph_budget()
        .exceedances()
        .axis(crate::node::PATHS_RESOURCE)
        .copied()
        .expect("the rechecks record the path exceedance");
    assert_eq!(paths.target, crate::node::MAX_PATH_CHECKS_PER_GRAPH as u64);
    assert!(paths.first_requested > crate::node::MAX_PATH_CHECKS_PER_GRAPH as u64);
    assert_eq!(
        paths.high_water,
        store
            .graph_budget()
            .usage
            .lock()
            .expect("usage reads")
            .paths as u64
    );
    assert!(store
        .root()
        .join(format!("locks/actions/{}.lock", recipe.action()))
        .exists());
    fs::remove_dir_all(root).expect("native floor fixture retires");
}
