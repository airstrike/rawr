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
                create_bootstrap(&root.0, &path).expect("accept contender creation");
                with_bootstrap(&root.0, false, || {
                    assert_eq!(active.fetch_add(1, Ordering::SeqCst), 0);
                    completed.fetch_add(1, Ordering::SeqCst);
                    assert_eq!(active.fetch_sub(1, Ordering::SeqCst), 1);
                    Ok(())
                })
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
        create_bootstrap(&root.0, &path,),
        Err(Error::Shape(_))
    ));
    fs::remove_file(&path).expect("remove symlink");
    fs::hard_link(&target, &path).expect("create hard-linked contender");
    assert!(matches!(
        create_bootstrap(&root.0, &path,),
        Err(Error::Shape(_))
    ));
    fs::remove_file(&path).expect("remove hard link");
    fs::create_dir(&path).expect("create directory contender");
    assert!(matches!(
        create_bootstrap(&root.0, &path,),
        Err(Error::Shape(_))
    ));
}
