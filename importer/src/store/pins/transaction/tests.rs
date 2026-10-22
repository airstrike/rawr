use std::collections::BTreeMap;
use std::fs::{self, File};
use std::os::fd::AsRawFd as _;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;

use super::*;
use crate::node::{Direct, Discoverable, Evaluator, Kind, Value};
use crate::store::accounting::Charge;
use crate::store::{lease, Store};

struct Artifact(u8);

impl Value for Artifact {
    const KIND: Kind = Kind::NativeProduct;
    const SCHEMA: u32 = 1;

    fn encode(&self, _: usize) -> Result<Box<[u8]>, Error> {
        Ok(Box::from([self.0]))
    }

    fn decode(bytes: &[u8], _: usize) -> Result<Self, Error> {
        match bytes {
            [value] => Ok(Self(*value)),
            _ => Err(Error::Codec("pin transaction fixture changed")),
        }
    }
}

fn store(label: &str) -> Store {
    let root = std::env::temp_dir().join(format!(
        "rawr-pin-transaction-{label}-{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir(&root).unwrap();
    let store = Store::open(&root, [0x97; 32]).unwrap();
    lease::with_project(store.root(), store.project, || Ok(())).unwrap();
    store
}

fn proof(store: &Store, value: u8) -> ReadSetProof {
    let recipe = Direct::<Artifact>::capture([0x28; 32], &[value], 4_096, &[], 1).unwrap();
    let evaluator = Evaluator::new(store, &crate::metrics::Noop);
    let value = evaluator.direct(&recipe, || Ok(Artifact(value))).unwrap();
    evaluator
        .seal_read_set([value.receipt().read_set().clone()])
        .unwrap()
}

#[derive(Debug, Eq, PartialEq)]
struct Snapshot {
    ledger: Option<Vec<u8>>,
    installation: Vec<u8>,
    records: BTreeMap<PathBuf, Vec<u8>>,
    physical: (Charge, Charge),
    project: (u64, u64, u64, usize, usize),
    pins: (usize, usize),
}

fn snapshot(store: &Store) -> Snapshot {
    let ledger = match fs::read(super::super::ledger_path(store.root(), store.project)) {
        Ok(bytes) => Some(bytes),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => panic!("ledger read failed: {error}"),
    };
    let mut records = BTreeMap::new();
    for role in [Role::Live, Role::LastGood, Role::Prepared, Role::InFlight] {
        for record in fs::read_dir(store.root().join("pins").join(role.directory())).unwrap() {
            let path = record.unwrap().path().join("root");
            records.insert(
                path.strip_prefix(store.root()).unwrap().to_path_buf(),
                fs::read(path).unwrap(),
            );
        }
    }
    Snapshot {
        ledger,
        installation: fs::read(store.root().join("ledgers/installation")).unwrap(),
        records,
        physical: crate::store::accounting::physical_usage(store.root()).unwrap(),
        project: store.fixture_project_usage().unwrap(),
        pins: store.fixture_pin_counts().unwrap(),
    }
}

fn publication_is_locked(store: &Store) {
    let file = File::open(store.root().join("locks/publication.lock")).unwrap();
    assert!(matches!(
        nix::fcntl::flock(
            file.as_raw_fd(),
            nix::fcntl::FlockArg::LockExclusiveNonblock
        ),
        Err(nix::errno::Errno::EWOULDBLOCK)
    ));
}

#[test]
fn prepared_failures_restore_absent_empty_and_existing_ledgers_exactly() {
    for previous in 0..3 {
        for (index, stage) in [
            Stage::Published(Role::Prepared),
            Stage::Acquired(1),
            Stage::Ledger,
            Stage::PostBind,
        ]
        .into_iter()
        .enumerate()
        {
            let store = store(&format!("prepared-{previous}-{index}"));
            let old = proof(&store, 1);
            let next = proof(&store, 2);
            let retained = match previous {
                0 => None,
                1 => {
                    lease::with_publication(store.root(), store.graph_budget(), |publication| {
                        super::super::save(
                            publication,
                            store.root(),
                            &Ledger::empty(store.project),
                            #[cfg(feature = "import-baseline")]
                            crate::metrics::publication::Replacement::ProjectRecovery,
                            #[cfg(feature = "import-baseline")]
                            crate::metrics::principal::ledger::Cause::Recovery(None),
                        )
                    })
                    .unwrap();
                    None
                }
                2 => Some(store.acquire_pin(Role::Prepared, &old).unwrap()),
                _ => unreachable!(),
            };
            let before = snapshot(&store);
            FAILURE.with(|failure| failure.set(Some(stage)));
            assert!(matches!(
                store.acquire_pin(Role::Prepared, &next),
                Err(Error::State("injected pin transaction failure"))
            ));
            assert_eq!(snapshot(&store), before);
            drop(retained);
            store.retire_fixture().unwrap();
        }
    }
}

#[test]
fn live_last_good_failures_restore_both_roles_and_their_exact_old_closure() {
    for (index, stage) in [
        Stage::Published(Role::Live),
        Stage::Acquired(1),
        Stage::Published(Role::LastGood),
        Stage::Acquired(2),
        Stage::Ledger,
        Stage::PostBind,
    ]
    .into_iter()
    .enumerate()
    {
        let store = store(&format!("promotion-{index}"));
        let old = proof(&store, 3);
        let next = proof(&store, 4);
        let prepared = store.acquire_pin(Role::Prepared, &next).unwrap();
        let retained = store.pin_live_last_good(&old).unwrap();
        let before = snapshot(&store);
        FAILURE.with(|failure| failure.set(Some(stage)));
        assert!(matches!(
            store.pin_live_last_good(&next),
            Err(Error::State("injected pin transaction failure"))
        ));
        assert_eq!(snapshot(&store), before);
        drop((retained, prepared));
        store.retire_fixture().unwrap();
    }
}

#[test]
fn rollback_preserves_a_preexisting_record_reused_by_the_failed_candidate() {
    let store = store("existing-record");
    let old = proof(&store, 5);
    let next = proof(&store, 6);
    drop(store.acquire_pin(Role::Prepared, &next).unwrap());
    let retained = store.acquire_pin(Role::Prepared, &old).unwrap();
    let before = snapshot(&store);
    assert_eq!(before.pins, (2, 1));
    FAILURE.with(|failure| failure.set(Some(Stage::PostBind)));
    assert!(store.acquire_pin(Role::Prepared, &next).is_err());
    assert_eq!(snapshot(&store), before);
    drop(retained);
    store.retire_fixture().unwrap();
}

#[test]
fn admission_error_rolls_back_and_success_only_transfers_the_locked_roots() {
    let store = store("callback");
    let old = proof(&store, 7);
    let next = proof(&store, 8);
    let retained = store.pin_live_last_good(&old).unwrap();
    let before = snapshot(&store);
    #[cfg(feature = "import-baseline")]
    let recorder = crate::metrics::recorder::Recorder::transform(10_000).unwrap();
    #[cfg(feature = "import-baseline")]
    let scope = recorder.scope().unwrap();
    let rejected = store
        .with_live_last_good(&next, || {
            publication_is_locked(&store);
            assert_eq!(store.fixture_pin_counts().unwrap(), (4, 4));
            Err::<(), _>("profile rejected")
        })
        .unwrap();
    assert!(matches!(
        rejected,
        Err(Rejected::Operation("profile rejected"))
    ));
    assert_eq!(snapshot(&store), before);
    #[cfg(feature = "import-baseline")]
    {
        use crate::metrics::principal::{self, ledger};
        drop(scope);
        let evidence = recorder.snapshot().unwrap().principal.unwrap();
        assert!(evidence.fault.is_none());
        let prior = principal::Commitment::produced(before.ledger.as_deref().unwrap());
        assert!(evidence
            .events
            .as_slice()
            .iter()
            .any(|event| matches!(event,
                principal::Event::Ledger { change: ledger::Event::Started {
                    cause: ledger::Cause::PinBind, before: ledger::Prior::Bytes(value), ..
                } } if *value == prior
            )));
        assert!(evidence
            .events
            .as_slice()
            .iter()
            .any(|event| matches!(event,
                principal::Event::Ledger { change: ledger::Event::Started {
                    cause: ledger::Cause::PinRestore, after, ..
                } } if *after == prior
            )));
    }

    let committed = store
        .with_live_last_good(&next, || {
            publication_is_locked(&store);
            Ok::<_, &'static str>(19)
        })
        .unwrap()
        .unwrap();
    assert_eq!(committed.0, 19);
    assert_eq!(store.fixture_pin_counts().unwrap(), (4, 4));
    let ledger = Ledger::read(store.root(), store.project).unwrap();
    assert!(ledger.roots.iter().all(|root| root.proof == next.digest()));
    drop(retained);
    assert_eq!(store.fixture_pin_counts().unwrap(), (4, 2));
    drop(committed);
    store.retire_fixture().unwrap();
}

#[test]
fn post_bind_witness_drift_uses_the_same_open_store_and_never_calls_admission() {
    let store = store("witness");
    let old = proof(&store, 9);
    let retained = store.pin_live_last_good(&old).unwrap();
    let armed = Arc::new(AtomicBool::new(false));
    let visits = Arc::new(AtomicUsize::new(0));
    let validation = store.clone();
    let validating = Arc::clone(&armed);
    let counting = Arc::clone(&visits);
    let recipe = Discoverable::<Artifact>::capture([0x28; 32], b"witness", 4_096, &[], 1).unwrap();
    let evaluator = Evaluator::new(&store, &crate::metrics::Noop);
    let value = evaluator
        .discover_budgeted(
            &recipe,
            move |value, budget| {
                budget.paths(1)?;
                let _ = validation.fixture_project_usage()?;
                if validating.load(Ordering::SeqCst) && counting.fetch_add(1, Ordering::SeqCst) == 1
                {
                    publication_is_locked(&validation);
                    return Err(Error::Codec("external witness drift"));
                }
                Ok(Box::from([value.0]))
            },
            || Ok(Artifact(10)),
        )
        .unwrap();
    let next = evaluator
        .seal_read_set([value.receipt().read_set().clone()])
        .unwrap();
    let before = snapshot(&store);
    armed.store(true, Ordering::SeqCst);
    let result = store.with_live_last_good(&next, || -> Result<(), &'static str> {
        panic!("stale union must reject before admission");
    });
    assert!(matches!(
        result,
        Err(Error::Codec("external witness drift"))
    ));
    assert_eq!(visits.load(Ordering::SeqCst), 2);
    assert_eq!(snapshot(&store), before);
    drop(retained);
    store.retire_fixture().unwrap();
}

#[test]
fn rollback_error_retains_the_original_admission_failure() {
    let store = store("cleanup-error");
    let old = proof(&store, 11);
    let next = proof(&store, 12);
    let retained = store.pin_live_last_good(&old).unwrap();
    let before = snapshot(&store);
    #[cfg(feature = "import-baseline")]
    let recorder = crate::metrics::recorder::Recorder::transform(10_000).unwrap();
    #[cfg(feature = "import-baseline")]
    let scope = recorder.scope().unwrap();
    let result = store
        .with_live_last_good(&next, || {
            FAILURE.with(|failure| failure.set(Some(Stage::Rollback)));
            Err::<(), _>("profile rejected")
        })
        .unwrap();
    assert!(matches!(
        result,
        Err(Rejected::Cleanup {
            failure: "profile rejected",
            cleanup: Error::State("injected pin transaction failure"),
        })
    ));
    assert_eq!(
        snapshot(&store),
        before,
        "the armed owner retries cleanup while publication remains held"
    );
    #[cfg(feature = "import-baseline")]
    {
        use crate::metrics::principal::{self, pin};
        drop(scope);
        let evidence = recorder.snapshot().unwrap().principal.unwrap();
        assert!(evidence.fault.is_none());
        let attempts: Vec<_> = evidence
            .events
            .as_slice()
            .iter()
            .filter_map(|event| match event {
                principal::Event::Pin {
                    change:
                        pin::Event::TransactionStep {
                            step: pin::TransactionStep::Restore { attempt },
                            ..
                        },
                } => Some(*attempt),
                _ => None,
            })
            .collect();
        let results: Vec<_> = evidence
            .events
            .as_slice()
            .iter()
            .filter_map(|event| match event {
                principal::Event::Pin {
                    change:
                        pin::Event::TransactionStep {
                            step: pin::TransactionStep::Restored { attempt, succeeded },
                            ..
                        },
                } => Some((*attempt, *succeeded)),
                _ => None,
            })
            .collect();
        assert_eq!(
            attempts.len(),
            2,
            "explicit failure and Drop retry are separate occurrences"
        );
        assert_ne!(attempts[0], attempts[1]);
        assert_eq!(results, [(attempts[0], false), (attempts[1], true)]);
        let acquired: Vec<_> = evidence
            .events
            .as_slice()
            .iter()
            .filter_map(|event| match event {
                principal::Event::Pin {
                    change:
                        pin::Event::TransactionStep {
                            step: pin::TransactionStep::Acquired { owner, .. },
                            ..
                        },
                } => Some(*owner),
                _ => None,
            })
            .collect();
        let previous: Vec<_> = evidence
            .events
            .as_slice()
            .iter()
            .filter_map(|event| match event {
                principal::Event::Pin {
                    change:
                        pin::Event::TransactionStep {
                            step: pin::TransactionStep::Previous { owner },
                            ..
                        },
                } => Some(*owner),
                _ => None,
            })
            .collect();
        assert!(!acquired.is_empty() && !previous.is_empty());
        let mut owners = std::collections::BTreeSet::new();
        for event in evidence.events.as_slice() {
            match event {
                principal::Event::Pin {
                    change: pin::Event::Owner { owner, .. },
                } => assert!(owners.insert(*owner)),
                principal::Event::Pin {
                    change: pin::Event::Retired { owner },
                } => assert!(owners.remove(owner)),
                principal::Event::Pin {
                    change:
                        pin::Event::TransactionStep {
                            step: pin::TransactionStep::RestoreApplied { .. },
                            ..
                        },
                } => {
                    assert!(acquired.iter().all(|owner| !owners.contains(owner)));
                    assert!(
                        previous.iter().all(|owner| owners.contains(owner)),
                        "old guards still cover completed restoration mutations"
                    );
                }
                principal::Event::Pin {
                    change:
                        pin::Event::TransactionStep {
                            step:
                                pin::TransactionStep::Restored {
                                    succeeded: true, ..
                                },
                            ..
                        },
                } => {
                    assert!(
                        previous.iter().all(|owner| !owners.contains(owner)),
                        "old guards retire before the success marker"
                    );
                }
                _ => {}
            }
        }
        assert!(
            owners.is_empty(),
            "every reached validated guard retired inside this scope"
        );
        let relocated = evidence
            .events
            .as_slice()
            .iter()
            .position(|event| {
                matches!(
                    event,
                    principal::Event::Pin {
                        change: pin::Event::Relocated { .. }
                    }
                )
            })
            .unwrap();
        let removed = evidence
            .events
            .as_slice()
            .iter()
            .position(|event| {
                matches!(
                    event,
                    principal::Event::Pin {
                        change: pin::Event::Step {
                            step: pin::Step::Removed,
                            ..
                        }
                    }
                )
            })
            .unwrap();
        assert!(relocated < removed);
    }
    drop(retained);
    store.retire_fixture().unwrap();
}

#[test]
fn discarded_and_unwinding_transactions_release_new_guards_before_restoration() {
    let store = store("unwinding");
    let old = proof(&store, 13);
    let next = proof(&store, 14);
    let retained = store.pin_live_last_good(&old).unwrap();
    let before = snapshot(&store);
    lease::with_publication(store.root(), store.graph_budget(), |publication| {
        drop(Transaction::bind(
            publication,
            store.root(),
            store.project,
            [Role::Live, Role::LastGood],
            &next,
        )?);
        Ok(())
    })
    .unwrap();
    assert_eq!(snapshot(&store), before);
    let panicked = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _ = store.with_live_last_good(&next, || -> Result<(), &'static str> {
            panic!("injected admission panic");
        });
    }));
    assert!(panicked.is_err());
    assert_eq!(snapshot(&store), before);
    drop(retained);
    store.retire_fixture().unwrap();
}

#[cfg(feature = "import-baseline")]
#[test]
fn recovery_observes_the_removed_reservation_instead_of_the_current_key() {
    use crate::metrics::{principal, recorder};
    use crate::store::accounting::LeaseKey;

    let store = store("recovery-key-observation");
    let proof = proof(&store, 27);
    let retained = store.pin_live_last_good(&proof).unwrap();
    let removed = LeaseKey::Action(crate::node::ActionKey::from_bytes([0x71; 32]));
    let current = LeaseKey::Action(crate::node::ActionKey::from_bytes([0x72; 32]));
    lease::with_publication(store.root(), store.graph_budget(), |publication| {
        super::super::reserve(
            publication,
            store.root(),
            store.project,
            removed,
            Charge::zero(),
        )
    })
    .unwrap();
    let recorder = recorder::Recorder::transform(10_000).unwrap();
    let scope = recorder.scope().unwrap();
    lease::with_publication(store.root(), store.graph_budget(), |publication| {
        super::super::recover(
            publication,
            store.root(),
            store.project,
            current,
            &mut crate::store::recovery::Budget::new(),
        )
    })
    .unwrap();
    drop(scope);
    let evidence = recorder.snapshot().unwrap().principal.unwrap();
    assert!(evidence.fault.is_none());
    let recovered: Vec<_> = evidence
        .events
        .as_slice()
        .iter()
        .filter_map(|event| match event {
            principal::Event::Ledger {
                change:
                    principal::ledger::Event::Started {
                        cause: principal::ledger::Cause::Recovery(key),
                        ..
                    },
            } => Some(*key),
            _ => None,
        })
        .collect();
    assert_eq!(recovered, [Some(removed.observed())]);
    assert!(Ledger::read(store.root(), store.project)
        .unwrap()
        .reservations
        .is_empty());
    drop(retained);
    store.retire_fixture().unwrap();
}
