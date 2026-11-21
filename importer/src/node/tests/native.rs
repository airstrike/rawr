use std::fs::{self, OpenOptions};
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{mpsc, Arc};
use std::time::Duration;

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
    while !locked(path) {
        std::thread::sleep(Duration::from_millis(1));
    }
    true
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
        .direct::<Artifact>(b"native-order")
        .expect("recipe seals");
    let heavy = store.fixture_owner_lock();
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
                    let context = fixture_context(&root);
                    let store = context.open_store().expect("the evaluator store opens");
                    let recipe = context
                        .direct::<Artifact>(b"native-order")
                        .expect("recipe seals");
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
        assert!(received.recv().expect("native action completes"));
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
            &context.discover::<Artifact>(b"wait-parent").unwrap(),
            move |value| {
                observed.fetch_add(1, Ordering::Relaxed);
                if rejected.load(Ordering::Relaxed) {
                    return Err(Error::State("parent changed while waiting"));
                }
                value.encode()
            },
            value,
        )
        .unwrap();
    let retention = Retention::from_validated(&parent, None).unwrap();
    let recipe = context
        .discover_with_parents::<super::BundleValue>(b"wait-child", &[Parent::validated(&parent)])
        .unwrap();
    let validation_store = store.clone();
    let heavy = store.fixture_owner_lock();
    let before = refreshes.load(Ordering::Relaxed);
    let entered = AtomicBool::new(false);
    let launched = AtomicBool::new(false);
    let pins = store.fixture_pin_counts().unwrap();
    let execution = recipe.execution();
    let (held, holding) = mpsc::channel();
    std::thread::scope(|threads| {
        // The holder opens its own store on its thread: a store's graph
        // budget stays on its thread.
        let holder = threads.spawn(|| {
            fixture_context(&root)
                .open_store()
                .unwrap()
                .with_execution_lease(execution, || {
                    held.send(()).unwrap();
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
        });
        holding.recv().unwrap();
        let result = Evaluator::new(&store, &counts).discover_bundled_retained_budgeted(
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
        assert!(result.is_err());
        holder.join().unwrap();
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
    prepare_fixture(&root);
    let counts = Counts::default();
    let executed = AtomicUsize::new(0);
    let active = AtomicUsize::new(0);
    let (entered, entry) = mpsc::channel();
    let (release, released) = mpsc::channel();
    let (done, completed) = mpsc::channel();
    std::thread::scope(|threads| {
        let active_first = &active;
        let executed_first = &executed;
        let counts_first = &counts;
        let root = &root;
        threads.spawn(move || {
            let context = fixture_context(root);
            let store = context.open_store().expect("first store opens");
            let recipe = context
                .direct::<Artifact>(b"same-native")
                .expect("first recipe seals");
            Evaluator::new(&store, counts_first)
                .native(&recipe, || {
                    assert_eq!(active_first.fetch_add(1, Ordering::SeqCst), 0);
                    executed_first.fetch_add(1, Ordering::Relaxed);
                    entered.send(()).expect("first executor announces entry");
                    released.recv().expect("first executor is released");
                    active_first.fetch_sub(1, Ordering::SeqCst);
                    value()
                })
                .expect("first native action publishes");
        });
        entry.recv().expect("first executor holds heavy");
        for request in [&b"same-native"[..], b"different-native"] {
            let done = done.clone();
            let counts = &counts;
            let active = &active;
            let executed = &executed;
            threads.spawn(move || {
                let context = fixture_context(root);
                let store = context.open_store().expect("other store opens");
                let request = context
                    .direct::<Artifact>(request)
                    .expect("contender recipe seals");
                let result = Evaluator::new(&store, counts).native(&request, || {
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
            assert!(completed.recv().expect("contender completes"));
        }
    });
    assert_eq!(executed.load(Ordering::Relaxed), 2);
    assert_eq!(counts.miss.load(Ordering::Relaxed), 2);
    assert_eq!(counts.published.load(Ordering::Relaxed), 2);
    assert_eq!(counts.hit.load(Ordering::Relaxed), 1);
    fs::remove_dir_all(root).expect("the native contention fixture retires");
}

#[test]
fn healthy_native_hits_do_not_wait_for_heavy() {
    let root = temporary("native-hit");
    let context = prepare_fixture(&root);
    let store = context.open_store().expect("the hit store opens");
    let warm = context
        .direct::<Artifact>(b"warm-native")
        .expect("warm recipe seals");
    Evaluator::new(&store, &Counts::default())
        .native(&warm, value)
        .expect("warm value seeds");
    let counts = Counts::default();
    let (sent, received) = mpsc::channel();
    std::thread::scope(|threads| {
        let result = store
            .with_heavy_lease(|| {
                threads.spawn(|| {
                    let context = fixture_context(&root);
                    let store = context.open_store().expect("the evaluator store opens");
                    let warm = context
                        .direct::<Artifact>(b"warm-native")
                        .expect("warm recipe seals");
                    let hit = Evaluator::new(&store, &counts)
                        .native(&warm, || panic!("healthy hit cannot execute"));
                    sent.send(hit.is_ok()).expect("the hit reports");
                });
                Ok(received.recv())
            })
            .expect("the external heavy lease releases");
        assert!(result.expect("a hit finishes while heavy is held"));
    });
    assert_eq!(counts.hit.load(Ordering::Relaxed), 1);
    fs::remove_dir_all(root).expect("the native hit fixture retires");
}

#[test]
fn uncached_scope_owns_the_project_and_exact_execution_and_releases_after_failure() {
    let root = temporary("native-scope");
    let context = std::rc::Rc::new(prepare_fixture(&root));
    let store = context.open_store().expect("the scope store opens");
    let scope = execution::Scope::bind(&context, execution::Operation::Probe, [17; 32]);
    let failure: crate::Result<()> = scope.run(|| {
        assert!(locked(&store.fixture_owner_lock()));
        assert!(!locked(&store.root().join("locks/heavy.lock")));
        let executions = fs::read_dir(store.root().join("locks/executions"))
            .expect("execution leases enumerate")
            .map(|entry| entry.expect("execution lease reads").path())
            .collect::<Vec<_>>();
        assert_eq!(executions.len(), 1);
        assert!(locked(&executions[0]));
        assert!(!locked(&store.root().join("locks/publication.lock")));
        Err(crate::Error::InputAccountRefused)
    });
    assert!(matches!(failure, Err(crate::Error::InputAccountRefused)));
    assert!(!locked(&store.fixture_owner_lock()));
    scope
        .run(|| Ok(()))
        .expect("failure releases both execution leases");
    fs::remove_dir_all(root).expect("the native scope fixture retires");
}

/// A second open of the owner lease on one thread used to wait on the first
/// forever. Native work now finds the lease its thread holds and runs under
/// it, and a node inside a node is refused at once.
#[test]
fn nested_heavy_work_runs_under_the_held_owner_lease() {
    let root = temporary("native-nested");
    let context = std::rc::Rc::new(prepare_fixture(&root));
    let store = context.open_store().expect("the nested store opens");
    let outer = context
        .direct::<Artifact>(b"nested-outer")
        .expect("outer recipe seals");
    let inner = context
        .direct::<Artifact>(b"nested-inner")
        .expect("inner recipe seals");
    let scoped = context
        .direct::<Artifact>(b"nested-scoped")
        .expect("scoped recipe seals");
    let probe = execution::Scope::bind(&context, execution::Operation::Probe, [23; 32]);
    let build = execution::Scope::bind(&context, execution::Operation::Build, [29; 32]);
    let counts = Counts::default();
    let evaluator = Evaluator::new(&store, &counts);
    let mut nested = None;
    evaluator
        .native(&outer, || {
            probe
                .run(|| build.run(|| Ok(())))
                .expect("a scope inside a scope inside a node runs");
            nested = Some(evaluator.direct(&inner, value));
            value()
        })
        .expect("the outer node publishes");
    assert!(matches!(
        nested,
        Some(Err(Error::State(
            "a node evaluation began inside another node's ownership of its project"
        )))
    ));
    let mut scoped_node = None;
    probe
        .run(|| {
            scoped_node = Some(evaluator.direct(&scoped, value));
            Ok(())
        })
        .expect("a node inside a scope runs");
    assert!(matches!(scoped_node, Some(Ok(_))));
    assert!(!locked(&store.fixture_owner_lock()));
    fs::remove_dir_all(root).expect("the nested fixture retires");
}

/// Two projects sharing one store run their heavy work at once: each holds
/// its own owner lease and execution lease, and both executors meet inside.
#[test]
fn two_projects_execute_heavy_work_at_once_in_one_store() {
    use crate::node::Value as _;

    let root = temporary("native-projects");
    prepare_fixture(&root);
    let shared = root.join("shared-store");
    fs::create_dir(&shared).expect("the shared store root creates");
    let both = std::sync::Barrier::new(2);
    let counts = Counts::default();
    std::thread::scope(|threads| {
        for (project, input) in [
            ([0x31; 32], &b"first-project"[..]),
            ([0x32; 32], b"second-project"),
        ] {
            let (root, shared, both, counts) = (&root, &shared, &both, &counts);
            threads.spawn(move || {
                let context = fixture_context(root);
                let store = crate::store::Store::open(shared, project).expect("the store opens");
                let recipe = context
                    .discover::<Artifact>(input)
                    .expect("the recipe seals");
                Evaluator::new(&store, counts)
                    .discover(
                        &recipe,
                        |value: &Artifact| value.encode(),
                        || {
                            assert!(locked(&store.fixture_owner_lock()));
                            both.wait();
                            value()
                        },
                    )
                    .expect("each project's execution publishes");
            });
        }
    });
    assert_eq!(counts.miss.load(Ordering::Relaxed), 2);
    fs::remove_dir_all(root).expect("the shared-store fixture retires");
}
