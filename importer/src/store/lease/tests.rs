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

const ROOT_ENV: &str = "RAWR_IMPORT_LEASE_TEST_ROOT";
const PROJECT: [u8; 32] = [7; 32];
const WAITING: &str = "Blocking waiting for file lock on the store at";

/// Take the project's owner lease in the store at `ROOT_ENV`, waiting for
/// its holder, and say so on stdout.
#[test]
#[ignore = "subprocess entry point"]
fn owner_lease_contender_process_helper() {
    let Some(root) = std::env::var_os(ROOT_ENV).map(PathBuf::from) else {
        return;
    };
    crate::store::Store::open(&root, PROJECT)
        .expect("the contender opens the store")
        .with_heavy_lease(|| Ok(()))
        .expect("the contender holds the lease once its holder lets go");
    println!("held");
}

/// The test binary taking the owner lease of the store at `root`, with its
/// stderr piped.
fn contender(root: &std::path::Path) -> std::process::Child {
    std::process::Command::new(std::env::current_exe().expect("the test executable is known"))
        .args([
            "--exact",
            "store::lease::tests::owner_lease_contender_process_helper",
            "--ignored",
            "--nocapture",
        ])
        .env(ROOT_ENV, root)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("the contender spawns")
}

/// What the contender said on stderr, and whether it held the lease.
fn finished(contender: std::process::Child) -> (String, bool) {
    let output = contender
        .wait_with_output()
        .expect("the contender is reaped");
    let held = output.status.success()
        && String::from_utf8_lossy(&output.stdout)
            .lines()
            .any(|line| line == "held");
    (String::from_utf8_lossy(&output.stderr).into_owned(), held)
}

/// A second process on one project finds the owner lease held, says once
/// that it waits, naming the store, and holds the lease once this lets go,
/// as a second Cargo does on its build directory.
#[test]
fn a_second_process_waits_for_the_owner_lease_and_says_so_once() {
    use std::io::{BufRead as _, Read as _};
    let root = Root::prepare();
    let store = crate::store::Store::open(&root.0, PROJECT).expect("the store opens");
    let (mut waiting, mut said) = store
        .with_heavy_lease(|| {
            let mut waiting = contender(&root.0);
            let mut said = std::io::BufReader::new(waiting.stderr.take().expect("stderr is piped"));
            // The contender says it waits before it blocks, so this returns
            // with the contender blocked on the lease or about to be.
            let line = (&mut said)
                .lines()
                .map(|line| line.expect("stderr reads"))
                .find(|line| line.contains(WAITING))
                .expect("the contender says it waits");
            assert!(line.contains(&root.0.display().to_string()), "{line}");
            Ok((waiting, said))
        })
        .expect("the lease is free");
    let mut rest = String::new();
    said.read_to_string(&mut rest).expect("stderr reads");
    let mut output = String::new();
    waiting
        .stdout
        .take()
        .expect("stdout is piped")
        .read_to_string(&mut output)
        .expect("stdout reads");
    assert!(waiting.wait().expect("the contender is reaped").success());
    assert!(output.lines().any(|line| line == "held"), "{output}");
    assert!(!rest.contains(WAITING), "the wait is said once: {rest}");
}

/// A round that fails inside the owner and execution leases leaves neither
/// held: the next process takes the lease at once and says no wait.
#[test]
fn a_failed_round_leaves_the_next_process_nothing_to_wait_for() {
    let root = Root::prepare();
    let store = crate::store::Store::open(&root.0, PROJECT).expect("the store opens");
    let execution = crate::node::ExecutionKey::from_bytes([9; 32]);
    let failed = store.with_heavy_lease(|| {
        store.with_execution_lease(execution, || {
            Err::<(), _>(Error::State("the round's compile failed"))
        })
    });
    assert!(matches!(failed, Err(Error::State(_))));
    let (said, held) = finished(contender(&root.0));
    assert!(held, "{said}");
    assert!(!said.contains(WAITING), "nothing held the lease: {said}");
}
