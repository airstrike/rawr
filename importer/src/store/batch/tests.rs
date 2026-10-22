use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{mpsc, Barrier};
use std::time::Duration;

use crate::node::{Direct, Error, Evaluator, Kind, Parent, Value};
use crate::store::Store;

static NEXT: AtomicU64 = AtomicU64::new(0);

struct Root(PathBuf);
impl Root {
    fn prepare() -> Self {
        let root = std::env::temp_dir().join(format!(
            "rawr-project-ownership-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&root).unwrap();
        Self(root)
    }
}
impl Drop for Root {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

struct Owned;
impl Value for Owned {
    const KIND: Kind = Kind::Facade;
    const SCHEMA: u32 = 1;
    fn encode(&self, _: usize) -> Result<Box<[u8]>, Error> {
        Ok(Box::from(&b"owned"[..]))
    }
    fn decode(bytes: &[u8], _: usize) -> Result<Self, Error> {
        if bytes == b"owned" {
            Ok(Self)
        } else {
            Err(Error::Codec("ownership fixture bytes changed"))
        }
    }
}

#[test]
fn batch_and_two_legacy_contenders_serialize_while_another_project_progresses() {
    let root = Root::prepare();
    let store = Store::open(&root.0, [1; 32]).unwrap();
    let other = Store::open(&root.0, [2; 32]).unwrap();
    let seed = Direct::<Owned>::capture([3; 32], b"seed", 64, &[], 0).unwrap();
    let owner = Evaluator::new(&store, &crate::metrics::Current)
        .direct(&seed, || Ok(Owned))
        .unwrap();
    let retained = crate::node::read_set::Retention::from_validated(&owner, None).unwrap();
    let recipe = Direct::<Owned>::capture(
        [3; 32],
        b"shared-child",
        64,
        &[Parent::validated(&owner)],
        1,
    )
    .unwrap();
    let independent = Direct::<Owned>::capture([4; 32], b"independent", 64, &[], 0).unwrap();
    let start = Barrier::new(4);
    let calls = AtomicUsize::new(0);
    let (done, completed) = mpsc::channel();
    let (progress, progressed) = mpsc::channel();
    let mut predecessor = None;
    std::thread::scope(|scope| {
        for _ in 0..2 {
            let done = done.clone();
            let store = &store;
            let recipe = &recipe;
            let start = &start;
            let calls = &calls;
            scope.spawn(move || {
                start.wait();
                let result = Evaluator::new(store, &crate::metrics::Current).direct(recipe, || {
                    calls.fetch_add(1, Ordering::SeqCst);
                    Ok(Owned)
                });
                done.send(result.is_ok()).unwrap();
            });
        }
        scope.spawn(|| {
            start.wait();
            let result =
                Evaluator::new(&other, &crate::metrics::Current).direct(&independent, || Ok(Owned));
            progress.send(result.is_ok()).unwrap();
        });
        store
            .with_staged_batch(&retained, &crate::metrics::Current, |batch| {
                let records = fs::read_dir(store.root().join("pins/in-flight"))
                    .unwrap()
                    .map(|entry| entry.unwrap().path().join("root"))
                    .collect::<Vec<_>>();
                assert_eq!(records.len(), 1, "initial batch pin has one exact root");
                predecessor = Some((records[0].clone(), fs::read(&records[0]).unwrap()));
                start.wait();
                assert!(progressed
                    .recv_timeout(Duration::from_secs(5))
                    .expect("other project must not wait for this ownership"));
                assert!(matches!(
                    completed.try_recv(),
                    Err(mpsc::TryRecvError::Empty)
                ));
                let staged = batch.evaluate(&recipe, || {
                    calls.fetch_add(1, Ordering::SeqCst);
                    Ok(Owned)
                })?;
                staged.validate_current(&store)?;
                Ok(())
            })
            .unwrap();
        for _ in 0..2 {
            assert!(completed
                .recv_timeout(Duration::from_secs(5))
                .expect("contender resumes after batch ownership releases"));
        }
    });
    assert_eq!(
        calls.load(Ordering::SeqCst),
        1,
        "both exact-action contenders reuse the batch winner"
    );
    let (predecessor, bytes) =
        predecessor.expect("the initial root was observed before replacement");
    assert_eq!(
        fs::read(&predecessor).unwrap(),
        bytes,
        "replacement preserves the exact historical predecessor record"
    );
    assert_eq!(
        store.fixture_pin_counts().unwrap(),
        (1, 0),
        "only the inactive predecessor awaits bounded miss recovery"
    );
    assert_eq!(
        store.fixture_project_usage().unwrap().4,
        0,
        "the final InFlight logical role was released"
    );
    let cleanup = Direct::<Owned>::capture([3; 32], b"cleanup-miss", 64, &[], 0).unwrap();
    Evaluator::new(&store, &crate::metrics::Current)
        .direct(&cleanup, || Ok(Owned))
        .unwrap();
    assert!(
        !predecessor.exists(),
        "the next legitimate miss retires the displaced predecessor"
    );
    assert_eq!(store.fixture_pin_counts().unwrap(), (0, 0));
}

#[test]
fn existing_ownership_does_not_create_a_missing_control() {
    let root = Root::prepare();
    let store = Store::open(&root.0, [1; 32]).unwrap();
    store.with_ownership(|_| Ok(())).unwrap();
    let owner = store
        .root()
        .join("locks/owners")
        .join(format!("{}.lock", store.project.directory()));
    let before = fs::metadata(&owner).unwrap().modified().unwrap();
    store.with_existing_ownership(|_| Ok(())).unwrap();
    assert_eq!(fs::metadata(&owner).unwrap().modified().unwrap(), before);
    fs::remove_file(&owner).unwrap();
    assert!(store.with_existing_ownership(|_| Ok(())).is_err());
    assert!(store.with_ownership(|_| Ok(())).is_err());
    assert!(
        !owner.exists(),
        "healthy ownership acquisition cannot initialize controls"
    );
    let project = store
        .root()
        .join("locks/projects")
        .join(format!("{}.lock", store.project.directory()));
    fs::remove_file(&project).unwrap();
    assert!(super::super::lease::with_project(store.root(), store.project, || Ok(())).is_err());
    assert!(
        !project.exists(),
        "project acquisition cannot recreate an enrolled lease"
    );
}

#[test]
fn batch_retains_sibling_results_and_their_join_with_exact_edges() {
    let root = Root::prepare();
    let store = Store::open(&root.0, [1; 32]).unwrap();
    let seed = Direct::<Owned>::capture([3; 32], b"branch-seed", 64, &[], 0).unwrap();
    let owner = Evaluator::new(&store, &crate::metrics::Current)
        .direct(&seed, || Ok(Owned))
        .unwrap();
    let retained = crate::node::read_set::Retention::from_validated(&owner, None).unwrap();
    let left =
        Direct::<Owned>::capture([3; 32], b"branch-left", 64, &[Parent::validated(&owner)], 1)
            .unwrap();
    let right = Direct::<Owned>::capture(
        [3; 32],
        b"branch-right",
        64,
        &[Parent::validated(&owner)],
        1,
    )
    .unwrap();
    store
        .with_staged_batch(&retained, &crate::metrics::Current, |batch| {
            let left = batch.evaluate(&left, || Ok(Owned))?;
            let right = batch.evaluate(&right, || Ok(Owned))?;
            assert_eq!(left.fragment().len(), 2);
            assert_eq!(right.fragment().len(), 2);
            assert_eq!(batch.retained.get().entries().len(), 3);
            let left = left.validate_current(&store)?;
            let right = right.validate_current(&store)?;
            let join = Direct::<Owned>::capture(
                [3; 32],
                b"branch-join",
                64,
                &[Parent::validated(&left), Parent::validated(&right)],
                2,
            )?;
            let joined = batch.evaluate(&join, || Ok(Owned))?;
            assert_eq!(joined.fragment().len(), 4);
            assert_eq!(batch.retained.get().entries().len(), 4);
            joined.validate_current(&store)?;
            Ok(())
        })
        .unwrap();
    assert_eq!(store.fixture_project_usage().unwrap().4, 0);
}

#[cfg(feature = "import-baseline")]
#[test]
fn staged_publication_reports_actual_payload_and_hits_do_not_encode_again() {
    use crate::metrics::{accounting::Scalar, recorder::Recorder, NodeEvent, NodeKind};

    let root = Root::prepare();
    let store = Store::open(&root.0, [1; 32]).unwrap();
    let seed = Direct::<Owned>::capture([3; 32], b"payload-seed", 64, &[], 0).unwrap();
    let owner = Evaluator::new(&store, &crate::metrics::Current)
        .direct(&seed, || Ok(Owned))
        .unwrap();
    let retained = crate::node::read_set::Retention::from_validated(&owner, None).unwrap();
    let recipe = Direct::<Owned>::capture(
        [3; 32],
        b"payload-child",
        64,
        &[Parent::validated(&owner)],
        1,
    )
    .unwrap();

    for published in [true, false] {
        let recorder = Recorder::transform(u64::MAX).unwrap();
        {
            let _scope = recorder.scope().unwrap();
            store
                .with_staged_batch(&retained, &crate::metrics::Current, |batch| {
                    batch.evaluate(&recipe, || {
                        assert!(published, "a staged hit must not repeat its encoder");
                        Ok(Owned)
                    })?;
                    Ok(())
                })
                .unwrap();
        }
        let observed = recorder.snapshot().unwrap();
        let payload = observed
            .accounting
            .scalar(Scalar::NodePayloadBytes(NodeKind::Facade));
        if published {
            let payload = payload.expect("the actual batch encoder reports its output");
            assert_eq!(
                (payload.count, payload.min, payload.max, payload.total),
                (1, 5, 5, 5)
            );
        } else {
            assert!(payload.is_none(), "a hit has no new encoding observation");
        }
        let event = if published {
            NodeEvent::Published
        } else {
            NodeEvent::ValidatedHit
        };
        assert!(observed
            .nodes
            .iter()
            .any(|row| row.kind == NodeKind::Facade && row.event == event && row.count == 1));
    }
}
