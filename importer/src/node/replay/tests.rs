use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;

use super::{Staged, Stored};
use crate::node::codec::Envelope;
use crate::node::read_set::{Validator, Witness};
use crate::node::{content_digest, Discoverable, Error, Evaluator, GraphBudget, Kind, Value};
use crate::store::Store;

static NEXT: AtomicU64 = AtomicU64::new(0);

#[test]
fn mixed_branch_carries_hits_without_replay_and_promotes_one_exact_union() {
    use crate::node::staging::Owned as Result;
    use crate::node::{Direct, Lookup, Parent};
    for first_hit in [false, true] {
        for second_hit in [false, true] {
            for drift in [false, true] {
                let root = Root::prepare();
                let store = root.open();
                let calls = Arc::new(AtomicUsize::new(0));
                let owner = stage_discovered(&root, &store, Arc::clone(&calls))
                    .validate_current(&store)
                    .unwrap();
                let retention =
                    crate::node::read_set::Retention::from_validated(&owner, None).unwrap();
                let recipe = |inputs: &[u8]| {
                    Direct::<Owned>::capture([1; 32], inputs, 64, &[Parent::validated(&owner)], 1)
                        .unwrap()
                };
                let first_recipe = recipe(b"profile sibling");
                let second_recipe = recipe(b"generic sibling");
                let evaluator = Evaluator::new(&store, &crate::metrics::Current);
                if first_hit {
                    evaluator.direct(&first_recipe, || Ok(Owned)).unwrap();
                }
                if second_hit {
                    evaluator.direct(&second_recipe, || Ok(Owned)).unwrap();
                }
                calls.store(0, Ordering::SeqCst);
                let first = evaluator.lookup(&first_recipe).unwrap();
                let second = evaluator.lookup(&second_recipe).unwrap();
                let executions = AtomicUsize::new(0);
                let result =
                    store.with_staged_batch(&retention, &crate::metrics::Current, |batch| {
                        let mut obtain = |lookup, recipe| match lookup {
                            Lookup::Hit(hit) => batch.carry(hit),
                            Lookup::Miss(miss) => batch
                                .resume(recipe, miss, || {
                                    executions.fetch_add(1, Ordering::SeqCst);
                                    Ok(Owned)
                                })
                                .map(Result::Staged),
                        };
                        let first = obtain(first, &first_recipe)?;
                        let second = obtain(second, &second_recipe)?;
                        let facade_recipe = Direct::<Owned>::capture_edges(
                            [1; 32],
                            b"facade child",
                            64,
                            std::slice::from_ref(&second),
                            1,
                        )?;
                        let third = Result::Staged(batch.evaluate(&facade_recipe, || Ok(Owned))?);
                        assert_eq!(calls.load(Ordering::SeqCst), 0);
                        if drift {
                            fs::write(root.0.join("source"), b"after").unwrap();
                        }
                        super::promote_three(first, second, third, &store)
                    });
                assert_eq!(
                    executions.load(Ordering::SeqCst),
                    usize::from(!first_hit) + usize::from(!second_hit)
                );
                assert_eq!(calls.load(Ordering::SeqCst), 1);
                if drift {
                    assert!(matches!(result, Err(Error::StaleReadSet { .. })));
                } else {
                    let (first, second, third) = result.unwrap();
                    assert_eq!(first.receipt().read_set().len(), 2);
                    assert_eq!(second.receipt().read_set().len(), 2);
                    assert_eq!(third.receipt().read_set().len(), 3);
                    assert_eq!(first.receipt().action(), first_recipe.action());
                    assert_eq!(second.receipt().action(), second_recipe.action());
                }
            }
        }
    }
}

#[test]
fn resumed_miss_rechecks_a_winner_without_executing_or_replaying_mutable_ancestors() {
    use crate::node::{Direct, Lookup, Parent};
    let root = Root::prepare();
    let store = root.open();
    let calls = Arc::new(AtomicUsize::new(0));
    let owner = stage_discovered(&root, &store, Arc::clone(&calls))
        .validate_current(&store)
        .unwrap();
    let retention = crate::node::read_set::Retention::from_validated(&owner, None).unwrap();
    let recipe =
        Direct::<Owned>::capture([1; 32], b"winner", 64, &[Parent::validated(&owner)], 1).unwrap();
    let evaluator = Evaluator::new(&store, &crate::metrics::Current);
    let Lookup::Miss(miss) = evaluator.lookup(&recipe).unwrap() else {
        panic!("unexpected initial hit")
    };
    let winner = evaluator.direct(&recipe, || Ok(Owned)).unwrap();
    calls.store(0, Ordering::SeqCst);
    let result = store
        .with_staged_batch(&retention, &crate::metrics::Current, |batch| {
            batch.resume(&recipe, miss, || panic!("winner must suppress execution"))
        })
        .unwrap();
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert_eq!(result.action(), winner.receipt().action());
    assert_eq!(result.content().digest(), winner.content().digest());
}

#[test]
fn staged_pair_validates_its_complete_union_once_and_rejects_interstage_drift() {
    for drift in [false, true] {
        let root = Root::prepare();
        let store = root.open();
        let calls = Arc::new(AtomicUsize::new(0));
        let owner = stage_discovered(&root, &store, Arc::clone(&calls))
            .validate_current(&store)
            .unwrap();
        calls.store(0, Ordering::SeqCst);
        let retention = crate::node::read_set::Retention::from_validated(&owner, None).unwrap();
        let recipe = crate::node::Direct::<Owned>::capture(
            [1; 32],
            b"first pure",
            64,
            &[crate::node::Parent::validated(&owner)],
            1,
        )
        .unwrap();
        let result = store.with_staged_batch(&retention, &crate::metrics::Current, |batch| {
            let first = batch.evaluate(&recipe, || Ok(Owned))?;
            assert_eq!(calls.load(Ordering::SeqCst), 0);
            if drift {
                fs::write(root.0.join("source"), b"after").unwrap();
            }
            let recipe = crate::node::Direct::<Owned>::capture_edges(
                [1; 32],
                b"second pure",
                64,
                std::slice::from_ref(&first),
                1,
            )?;
            let second = batch.evaluate(&recipe, || Ok(Owned))?;
            assert_eq!(calls.load(Ordering::SeqCst), 0);
            first.validate_pair(second, &store)
        });
        if drift {
            assert!(matches!(result, Err(Error::StaleReadSet { .. })));
        } else {
            let (first, second) = result.unwrap();
            assert_eq!(first.receipt().read_set().len(), 2);
            assert_eq!(second.receipt().read_set().len(), 3);
        }
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }
}

struct Root(PathBuf);

impl Root {
    fn prepare() -> Self {
        let path = std::env::temp_dir().join(format!(
            "rawr-staged-replay-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).expect("create isolated staged replay root");
        fs::create_dir(path.join("store")).expect("create isolated store root");
        Self(path)
    }

    fn open(&self) -> Store {
        Store::open(&self.0.join("store"), [2; 32]).expect("open fixture store")
    }
}

impl Drop for Root {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

struct Owned;

impl Value for Owned {
    const KIND: Kind = Kind::Api;
    const SCHEMA: u32 = 1;

    fn encode(&self, _: usize) -> Result<Box<[u8]>, Error> {
        Ok(Box::from(&b"owned"[..]))
    }

    fn decode(bytes: &[u8], _: usize) -> Result<Self, Error> {
        if bytes != b"owned" {
            return Err(Error::Codec("fixture owned value changed"));
        }
        Ok(Self)
    }
}

fn stage_discovered(root: &Root, store: &Store, calls: Arc<AtomicUsize>) -> Staged<Owned> {
    let source = root.0.join("source");
    fs::write(&source, b"before").expect("write initial source");
    let recipe = Discoverable::<Owned>::capture([1; 32], b"fixed", 64, &[], 0)
        .expect("bind discoverable fixture");
    let current = recipe.complete(b"before").expect("bind initial source");
    Evaluator::new(store, &crate::metrics::Current)
        .direct(&current, || Ok(Owned))
        .expect("publish fixture output");
    let replayed = store
        .inspect_candidate::<Owned>(current.action(), 64, store.graph_budget())
        .expect("authenticate owned output")
        .expect("find published output");
    let validator: Validator<Owned> = Arc::new(move |_, _| {
        calls.fetch_add(1, Ordering::SeqCst);
        fs::read(&source)
            .map(Vec::into_boxed_slice)
            .map_err(|source_error| Error::Io {
                operation: "read staged fixture source",
                path: source.clone(),
                source: source_error,
            })
    });
    let witness = Witness::discovered(
        &recipe,
        replayed.value_arc(),
        validator,
        &current,
        replayed.content(),
        replayed.bundled(),
    );
    replayed
        .stage(&current, witness, store.graph_budget())
        .expect("stage authenticated output")
}

#[test]
fn unchanged_staged_output_promotes_only_after_current_union_validation() {
    let root = Root::prepare();
    let store = root.open();
    let calls = Arc::new(AtomicUsize::new(0));
    let staged = stage_discovered(&root, &store, Arc::clone(&calls));
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    let bytes = staged.value().encode(64).expect("pure owned transform");
    assert_eq!(&*bytes, b"owned");
    let expected = staged.content().digest();
    let current = staged
        .validate_current(&store)
        .expect("promote unchanged output");
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(current.receipt().content_digest(), expected);
    assert_eq!(current.receipt().read_set().len(), 1);
}

#[test]
fn source_drift_during_staged_pure_work_refuses_current_promotion() {
    let root = Root::prepare();
    let store = root.open();
    let calls = Arc::new(AtomicUsize::new(0));
    let staged = stage_discovered(&root, &store, Arc::clone(&calls));
    assert_eq!(
        &*staged.value().encode(64).expect("pure owned transform"),
        b"owned"
    );
    fs::write(root.0.join("source"), b"after").expect("change source during pure work");
    assert!(matches!(
        staged.validate_current(&store),
        Err(Error::StaleReadSet { .. })
    ));
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

#[test]
fn staged_output_cannot_promote_on_a_fresh_graph() {
    let root = Root::prepare();
    let store = root.open();
    let calls = Arc::new(AtomicUsize::new(0));
    let staged = stage_discovered(&root, &store, Arc::clone(&calls));
    let foreign = root.open();
    assert!(matches!(
        staged.validate_current(&foreign),
        Err(Error::State("node read-set belongs to another graph"))
    ));
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}

#[test]
fn replayed_output_cannot_stage_with_a_replacement_graph() {
    let graph = GraphBudget::new();
    let current = crate::node::Direct::<Owned>::capture([1; 32], b"fixed", 64, &[], 0)
        .expect("bind direct fixture");
    let bytes = Owned.encode(64).expect("encode fixture");
    let envelope = Envelope::direct(&current, content_digest::<Owned>(&bytes), bytes.len())
        .expect("bind fixture envelope");
    let replayed = Stored::new(envelope, bytes, false)
        .inspect(64, &graph)
        .expect("decode fixture");
    let witness = Witness::direct(&current, replayed.content(), false);
    assert!(matches!(
        replayed.stage(&current, witness, &GraphBudget::new()),
        Err(Error::State("replayed node belongs to another graph"))
    ));
}
