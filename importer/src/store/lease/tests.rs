use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::Barrier;

use super::{create_bootstrap, with_bootstrap};
use crate::node::Error;

static NEXT: AtomicU64 = AtomicU64::new(0);

struct Root(PathBuf);

impl Root {
    fn prepare() -> Self {
        let path = std::env::temp_dir().join(format!(
            "rawr-bootstrap-lease-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).expect("create isolated bootstrap root");
        Self(path)
    }
}

impl Drop for Root {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn project_enrollment_precedes_existing_only_ownership_and_does_not_repair_damage() {
    let root = Root::prepare();
    let first = crate::store::Store::open(&root.0, [1; 32]).expect("first project");
    first
        .with_existing_ownership(|_| Ok(()))
        .expect("first project enrolled");
    let second = crate::store::Store::open(&root.0, [2; 32]).expect("shared-store project");
    second
        .with_existing_ownership(|_| Ok(()))
        .expect("no local miss required");
    let name = format!(
        "{}.lock",
        crate::node::Digest::from_bytes([2; 32]).directory()
    );
    let owner = root.0.join("locks/owners").join(name);
    let before = fs::metadata(&owner).expect("enrolled owner");
    let ledger = fs::read(root.0.join("ledgers/installation")).expect("ledger");
    crate::store::Store::open(&root.0, [2; 32]).expect("healthy reopen");
    assert_eq!(
        ledger,
        fs::read(root.0.join("ledgers/installation")).expect("same ledger")
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt as _;
        assert_eq!(
            before.ino(),
            fs::metadata(&owner).expect("same owner").ino()
        );
    }
    fs::remove_file(&owner).expect("inject missing enrolled owner");
    assert!(matches!(
        crate::store::Store::open(&root.0, [2; 32]),
        Err(Error::CleanupRequired(
            "enrolled project ownership control is missing"
        ))
    ));
    assert!(
        !owner.exists(),
        "healthy reopen must not recreate damaged ownership"
    );
}

#[test]
fn concurrent_missing_observations_accept_one_bootstrap_identity() {
    let root = Root::prepare();
    let barrier = Barrier::new(2);
    let active = AtomicUsize::new(0);
    let completed = AtomicUsize::new(0);
    std::thread::scope(|scope| {
        for _ in 0..2 {
            scope.spawn(|| {
                let path = root.0.join("bootstrap.lock");
                assert_eq!(
                    fs::symlink_metadata(&path)
                        .expect_err("both observe absence")
                        .kind(),
                    std::io::ErrorKind::NotFound
                );
                barrier.wait();
                // Both enter the creation branch: exactly one sees AlreadyExists.
                create_bootstrap(
                    &root.0,
                    &path,
                    #[cfg(feature = "import-baseline")]
                    None,
                )
                .expect("accept contender creation");
                with_bootstrap(
                    &root.0,
                    false,
                    #[cfg(feature = "import-baseline")]
                    &crate::node::GraphBudget::new(),
                    || {
                        assert_eq!(active.fetch_add(1, Ordering::SeqCst), 0);
                        completed.fetch_add(1, Ordering::SeqCst);
                        assert_eq!(active.fetch_sub(1, Ordering::SeqCst), 1);
                        Ok(())
                    },
                )
                .expect("acquire the same bootstrap lease");
            });
        }
    });
    assert_eq!(completed.load(Ordering::SeqCst), 2);
}

#[cfg(unix)]
#[test]
fn contender_created_bootstrap_rejects_nonexclusive_file_shapes() {
    let root = Root::prepare();
    let path = root.0.join("bootstrap.lock");
    let target = root.0.join("target");
    fs::write(&target, []).expect("create target");
    std::os::unix::fs::symlink(&target, &path).expect("create symlink contender");
    assert!(matches!(
        create_bootstrap(
            &root.0,
            &path,
            #[cfg(feature = "import-baseline")]
            None,
        ),
        Err(Error::Shape(_))
    ));
    fs::remove_file(&path).expect("remove symlink");
    fs::hard_link(&target, &path).expect("create hard-linked contender");
    assert!(matches!(
        create_bootstrap(
            &root.0,
            &path,
            #[cfg(feature = "import-baseline")]
            None,
        ),
        Err(Error::Shape(_))
    ));
    fs::remove_file(&path).expect("remove hard link");
    fs::create_dir(&path).expect("create directory contender");
    assert!(matches!(
        create_bootstrap(
            &root.0,
            &path,
            #[cfg(feature = "import-baseline")]
            None,
        ),
        Err(Error::Shape(_))
    ));
}

#[cfg(feature = "import-baseline")]
#[test]
fn adopted_bootstrap_metadata_does_not_claim_empty_content() {
    use crate::metrics::{cell::project, principal, recorder};
    use principal::creation;

    let root = Root::prepare();
    let path = root.0.join("bootstrap.lock");
    fs::write(&path, b"existing control bytes").unwrap();
    let before = project::Manifest::read(&root.0).unwrap();
    let graph = crate::node::GraphBudget::new();
    let recorder = recorder::Recorder::transform(10_000).unwrap();
    let scope = recorder.scope().unwrap();
    {
        let observation = creation::Context::unleased(&root.0, &graph, creation::Source::Bootstrap)
            .start(&path, false);
        let result = create_bootstrap(&root.0, &path, Some(&observation));
        observation.completed(result.is_ok());
        result.unwrap();
    }
    drop(scope);
    let mut snapshot = recorder.snapshot().unwrap();
    let evidence = snapshot.principal.as_mut().unwrap();
    evidence
        .prepare_cell_transport(&mut snapshot.accounting, 10_000)
        .unwrap();
    let history: principal::Evidence =
        serde_json::from_slice(&serde_json::to_vec(evidence).unwrap()).unwrap();
    let empty = principal::Evidence {
        cell_transport: true,
        ..Default::default()
    };
    let after = project::Manifest::read(&root.0).unwrap();
    assert_eq!(before, after);
    let retirements =
        principal::retirement::join::pair([(1, &history), (2, &empty)], &root.0, &before, &after)
            .unwrap();
    let joined = creation::join::pair(
        [(1, &history), (2, &empty)],
        &root.0,
        &before,
        &after,
        &retirements,
    )
    .unwrap();
    let attempt = &joined.attempts[0];
    assert_eq!(attempt.creation, Some(creation::Outcome::Contended));
    assert_eq!(
        attempt.adopted.as_ref().unwrap().bytes,
        Some(b"existing control bytes".len() as u64)
    );
    let endpoint = joined.endpoints.iter().find(|row| !row.directory).unwrap();
    assert!(endpoint.creations.is_empty());
    assert_eq!(endpoint.adoptions.len(), 1);
    assert!(endpoint.initial_contents);
    let mut omitted = before;
    omitted.members = Box::default();
    assert!(creation::join::pair(
        [(1, &history), (2, &empty)],
        &root.0,
        &omitted,
        &after,
        &retirements
    )
    .is_err());
}
