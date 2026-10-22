//! Real migrated/new-writer companion and bounded migration regression tests.
use std::fs;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use crate::node::{Error, Evaluator, Fragment, Kind, Value};
use crate::store::{lease, pins::Role};

#[derive(Debug)]
struct Artifact;
impl Value for Artifact {
    const KIND: Kind = Kind::NativeProduct;
    const SCHEMA: u32 = 1;
    fn encode(&self, limit: usize) -> Result<Box<[u8]>, Error> {
        if limit < 1 {
            return Err(Error::Allocation);
        }
        Ok(Box::from([42]))
    }
    fn decode(bytes: &[u8], _limit: usize) -> Result<Self, Error> {
        if bytes != [42] {
            return Err(Error::State("compatibility fixture changed"));
        }
        Ok(Self)
    }
}

#[test]
#[ignore = "external coordinator owns the migration barrier"]
fn new_pin_worker() {
    let path = |name| PathBuf::from(std::env::var_os(name).expect("worker path supplied"));
    let fixture = path("RAWR_NEW_PIN_FIXTURE");
    let ready = path("RAWR_NEW_PIN_READY");
    let resume = path("RAWR_NEW_PIN_RESUME");
    let report = path("RAWR_NEW_PIN_REPORT");
    let mode = std::env::var("RAWR_NEW_PIN_MODE").expect("empty or nonempty supplied");
    assert!(matches!(mode.as_str(), "empty" | "nonempty"));
    fs::create_dir_all(fixture.join("project")).unwrap();
    fs::create_dir_all(fixture.join("cargo-home")).unwrap();
    let cargo_home = fs::canonicalize(fixture.join("cargo-home")).unwrap();
    let environment = crate::cargo::config::Environment::from_pairs([(
        "CARGO_HOME",
        cargo_home.to_str().unwrap(),
    )])
    .unwrap();
    let context = crate::context::Context::capture(
        &fixture.join("project"),
        environment,
        "test-target",
        &crate::cargo::Limits::standard(),
    )
    .unwrap();
    let store = context.open_store().unwrap();
    let evaluator = Evaluator::new(&store, &crate::metrics::Noop);
    let fragments = if mode == "nonempty" {
        let recipe = context
            .direct::<Artifact>(b"old-pin-exact", 1024)
            .unwrap();
        let value = evaluator
            .direct(&recipe, || {
                Err(Error::State("migration discarded reusable fixture output"))
            })
            .unwrap();
        vec![value.receipt().read_set().clone()]
    } else {
        Vec::<Fragment>::new()
    };
    let proof = evaluator.seal_read_set(fragments).unwrap();
    let bound = std::cell::Cell::new(false);
    let callback = std::cell::Cell::new(false);
    let result = store.with_ownership(|_| {
        let guards = lease::with_project(store.root(), store.project, || {
            lease::with_publication(store.root(), store.graph_budget(), |publication| {
                crate::store::pins::release_in_flight(
                    publication,
                    store.root(),
                    store.project,
                    proof.digest(),
                )?;
                let roots = store.root().join("pins/in-flight");
                assert_eq!(
                    fs::read_dir(&roots).unwrap().count(),
                    0,
                    "old exact root retired"
                );
                let transaction = crate::store::pins::transaction::Transaction::bind(
                    publication,
                    store.root(),
                    store.project,
                    [Role::InFlight],
                    &proof,
                )?;
                bound.set(true);
                assert_eq!(
                    fs::read_dir(&roots).unwrap().count(),
                    1,
                    "new root published after fence"
                );
                let (_, guards) = transaction
                    .finish(
                        || Ok(()),
                        || {
                            callback.set(true);
                            Ok::<_, Error>(())
                        },
                    )?
                    .map_err(|_| Error::State("fixture callback failed"))?;
                Ok(guards)
            })
        })?;
        // Hold the new ownership/InFlight scope, not publication or project.
        // The old worker must reach its mandatory ledger read without blocking.
        fs::write(
            &ready,
            format!(
                "root={}\nproject={}\nproof={}\nmode={}\n",
                store.root().display(),
                store.project.directory(),
                proof.digest().directory(),
                mode
            ),
        )
        .unwrap();
        let until = Instant::now() + Duration::from_secs(120);
        while !resume.exists() {
            assert!(
                Instant::now() < until,
                "coordinator did not release new guard"
            );
            std::thread::sleep(Duration::from_millis(20));
        }
        drop(guards);
        Ok(())
    });
    fs::write(
        report,
        format!(
            "bind_returned={}\ncallback_entered={}\nresult={:?}\n",
            bound.get(),
            callback.get(),
            result
        ),
    )
    .unwrap();
}

#[test]
fn fence_preserves_legacy_ledger_and_resumes_missing_controls() {
    let root = std::env::temp_dir().join(format!("rawr-migration-ledger-{}", std::process::id()));
    fs::create_dir(&root).unwrap();
    let store = crate::store::Store::open(&root, [177; 32]).unwrap();
    let legacy = super::super::Ledger::read(&root)
        .unwrap()
        .encode_schema(4)
        .unwrap();
    fs::remove_file(root.join("locks/ownership.ready")).unwrap();
    fs::remove_dir_all(root.join("locks/owners")).unwrap();
    fs::write(super::super::ledger_path(&root), &legacy).unwrap();
    drop(store);
    let migrated = crate::store::Store::open(&root, [177; 32]).unwrap();
    assert_eq!(
        super::super::Ledger::read(&root)
            .unwrap()
            .encode_schema(4)
            .unwrap(),
        legacy
    );
    assert!(root.join("locks/ownership.ready").is_file());
    assert!(root
        .join("locks/owners")
        .join(format!("{}.lock", migrated.project.directory()))
        .is_file());
    // A crash after the fence but before control publication stays fenced and
    // resumes without rerunning the legacy transition or losing prior controls.
    fs::remove_file(root.join("locks/ownership.ready")).unwrap();
    fs::remove_dir_all(root.join("locks/owners")).unwrap();
    drop(migrated);
    let resumed = crate::store::Store::open(&root, [177; 32]).unwrap();
    assert_eq!(
        super::super::Ledger::read(&root)
            .unwrap()
            .encode_schema(4)
            .unwrap(),
        legacy
    );
    resumed.retire_fixture().unwrap();
}

#[test]
fn enrollment_rejects_inverse_damage_and_resumes_only_with_pending_intent() {
    let root = std::env::temp_dir().join(format!("rawr-enrollment-inverse-{}", std::process::id()));
    fs::create_dir(&root).unwrap();
    let store = crate::store::Store::open(&root, [178; 32]).unwrap();
    let project = store.project.directory();
    let member = root.join("locks/projects").join(format!("{project}.lock"));
    fs::remove_file(&member).unwrap();
    assert!(matches!(
        crate::store::Store::open(&root, [178; 32]),
        Err(Error::CleanupRequired(
            "enrolled project lock control is missing"
        ))
    ));
    assert!(!member.exists(), "damage refusal does not repair silently");
    // Durable preexisting intent distinguishes a crashed enrollment from damage.
    let pending = root
        .join("locks/projects")
        .join(format!("{project}.pending"));
    fs::write(&pending, []).unwrap();
    let resumed = crate::store::Store::open(&root, [178; 32]).unwrap();
    assert!(member.is_file());
    assert!(!pending.exists());
    resumed.retire_fixture().unwrap();
}

#[test]
fn migration_controls_share_the_original_cached_ledger_account() {
    let root = std::env::temp_dir().join(format!("rawr-migration-account-{}", std::process::id()));
    fs::create_dir(&root).unwrap();
    let store = crate::store::Store::open(&root, [179; 32]).unwrap();
    let root = store.root().to_path_buf();
    let mut budget = crate::store::recovery::Budget::new();
    lease::with_publication(&root, store.graph_budget(), |publication| {
        super::advance(publication, &root, &mut budget)?;
        let first = budget.metadata_bytes();
        budget.metadata(8 * 1024 * 1024 - first as usize)?;
        let exhausted = budget.metadata_bytes();
        let path = root.join("locks/owners").join(format!(
            "{}.lock",
            crate::node::Digest::from_bytes([180; 32]).directory()
        ));
        crate::store::lease::ensure_migration_file(publication, &root, &path, &mut budget)?;
        assert_eq!(
            budget.metadata_bytes(),
            exhausted,
            "unchanged ledger is not decoded again for control admission"
        );
        // An actual intervening publication invalidates the cached identity.
        // The same exhausted account must refuse before creating another file.
        let ledger = super::super::Ledger::read(&root)?;
        super::super::save(
            publication,
            &root,
            &ledger,
            #[cfg(feature = "import-baseline")]
            crate::metrics::principal::ledger::Cause::Recovery(None),
        )?;
        let refused = root.join("locks/owners").join(format!(
            "{}.lock",
            crate::node::Digest::from_bytes([181; 32]).directory()
        ));
        assert!(matches!(
            crate::store::lease::ensure_migration_file(publication, &root, &refused, &mut budget),
            Err(Error::Limit {
                resource: "typed store recovery metadata bytes",
                ..
            })
        ));
        assert!(
            !refused.exists(),
            "budget refusal must precede control creation"
        );
        Ok(())
    })
    .unwrap();
    store.retire_fixture().unwrap();
}
