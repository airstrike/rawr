use super::*;
use std::fs;
use std::os::unix::fs::{symlink, PermissionsExt as _};
use std::sync::atomic::{AtomicUsize, Ordering};

struct Fixture(PathBuf);

impl Fixture {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "rawr-content-batch-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(fs::canonicalize(path).unwrap())
    }

    fn file(&self, name: &str) -> PathBuf {
        let path = self.0.join(name);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, b"first").unwrap();
        path
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}

#[test]
fn shared_parents_reduce_actual_path_work_and_unchanged_members_reuse_their_digest() {
    let fixture = Fixture::new();
    let paths: Vec<_> = (0..32)
        .map(|index| fixture.file(&format!("a/b/c/file{index}")))
        .collect();
    let old = GraphBudget::new();
    for path in &paths {
        super::super::hash(path, &fs::metadata(path).unwrap(), 1024, &old).unwrap();
    }
    assert_eq!(
        old.census_usage().unwrap().2,
        32 * 6,
        "the first consumer reads every member"
    );
    let graph = GraphBudget::new();
    let run = || {
        observe(&graph, |batch| {
            for path in &paths {
                let digest = batch.hash(path, &fs::metadata(path).unwrap(), 1024)?;
                assert_eq!(digest, <[u8; 32]>::from(Sha256::digest(b"first")));
            }
            Ok(())
        })
    };
    run().unwrap();
    let first = graph.census_usage().unwrap();
    assert!(first.1 < old.census_usage().unwrap().1 / 2);
    // Unchanged identities reuse the digest the earlier consumer computed.
    assert_eq!(first.2, 0);
    run().unwrap();
    let second = graph.census_usage().unwrap();
    assert_eq!(second, (first.0 * 2, first.1 * 2, 0));
    // A changed identity (here the status-change time) is read again.
    for path in &paths {
        fs::set_permissions(path, fs::metadata(path).unwrap().permissions()).unwrap();
    }
    run().unwrap();
    assert_eq!(graph.census_usage().unwrap().2, 32 * 6);
}

#[test]
fn renamed_parent_cannot_return_a_successful_batch() {
    let fixture = Fixture::new();
    let path = fixture.file("parent/file");
    let metadata = fs::metadata(&path).unwrap();
    let result = observe(&GraphBudget::new(), |batch| {
        let digest = batch.hash(&path, &metadata, 1024)?;
        fs::rename(fixture.0.join("parent"), fixture.0.join("moved")).unwrap();
        fixture.file("parent/file");
        Ok(digest)
    });
    assert!(result.is_err());
}

#[test]
fn stable_handle_does_not_follow_a_temporary_parent_redirection() {
    let fixture = Fixture::new();
    let first = fixture.file("parent/first");
    let second = fixture.file("parent/second");
    let second_metadata = fs::metadata(&second).unwrap();
    let outside = fixture.file("other/second");
    fs::write(outside, b"wrong").unwrap();
    observe(&GraphBudget::new(), |batch| {
        batch.hash(&first, &fs::metadata(&first).unwrap(), 1024)?;
        fs::rename(fixture.0.join("parent"), fixture.0.join("moved")).unwrap();
        symlink(fixture.0.join("other"), fixture.0.join("parent")).unwrap();
        let digest = batch.hash(&second, &second_metadata, 1024)?;
        assert_eq!(digest, <[u8; 32]>::from(Sha256::digest(b"first")));
        fs::remove_file(fixture.0.join("parent")).unwrap();
        fs::rename(fixture.0.join("moved"), fixture.0.join("parent")).unwrap();
        Ok(())
    })
    .unwrap();
}

#[test]
fn symlink_ancestor_and_final_symlink_reject() {
    let fixture = Fixture::new();
    let path = fixture.file("actual/file");
    let metadata = fs::metadata(&path).unwrap();
    symlink(fixture.0.join("actual"), fixture.0.join("alias")).unwrap();
    assert!(observe(&GraphBudget::new(), |batch| batch.hash(
        &fixture.0.join("alias/file"),
        &metadata,
        1024
    ))
    .is_err());
    symlink(&path, fixture.0.join("link")).unwrap();
    assert!(observe(&GraphBudget::new(), |batch| batch.hash(
        &fixture.0.join("link"),
        &metadata,
        1024
    ))
    .is_err());
}

#[test]
fn same_length_replacement_and_hard_links_reject() {
    let fixture = Fixture::new();
    let path = fixture.file("file");
    let metadata = fs::metadata(&path).unwrap();
    fs::rename(&path, fixture.0.join("old")).unwrap();
    fs::write(&path, b"first").unwrap();
    assert!(observe(&GraphBudget::new(), |batch| batch
        .hash(&path, &metadata, 1024))
    .is_err());
    fs::hard_link(&path, fixture.0.join("linked")).unwrap();
    let current = fs::metadata(&path).unwrap();
    assert!(observe(&GraphBudget::new(), |batch| batch
        .hash(&path, &current, 1024))
    .is_err());
}

#[test]
fn directory_mode_change_after_hash_rejects_at_finish() {
    let fixture = Fixture::new();
    let path = fixture.file("parent/file");
    let parent = path.parent().unwrap();
    let initial = fs::metadata(parent).unwrap().permissions();
    let result = observe(&GraphBudget::new(), |batch| {
        batch.hash(&path, &fs::metadata(&path).unwrap(), 1024)?;
        fs::set_permissions(parent, fs::Permissions::from_mode(initial.mode() ^ 0o100)).unwrap();
        Ok(())
    });
    fs::set_permissions(parent, initial).unwrap();
    assert!(result.is_err());
}

#[test]
fn original_budget_refusal_and_operation_failure_do_not_return_results() {
    let fixture = Fixture::new();
    let path = fixture.file("file");
    let metadata = fs::metadata(&path).unwrap();
    // A graph at its path target still returns the digest and records
    // the path axis at exactly target + the batch's own path checks.
    let measured = GraphBudget::new();
    observe(&measured, |batch| batch.hash(&path, &metadata, 1024)).unwrap();
    let batch_paths = measured.census_usage().unwrap().1;
    assert!(batch_paths > 0);
    assert!(!measured.exceedances().is_exceeded());
    // The digest is now memoized for this identity, so a repeat skips
    // the read and its final reinspection; a fresh identity repeats the work.
    fs::set_permissions(&path, metadata.permissions()).unwrap();
    let metadata = fs::metadata(&path).unwrap();
    let graph = GraphBudget::new();
    graph.paths(crate::node::MAX_PATH_CHECKS_PER_GRAPH).unwrap();
    assert_eq!(
        observe(&graph, |batch| batch.hash(&path, &metadata, 1024)).unwrap(),
        <[u8; 32]>::from(Sha256::digest(b"first"))
    );
    let axis = graph
        .exceedances()
        .axis(crate::node::PATHS_RESOURCE)
        .copied()
        .unwrap();
    assert_eq!(
        axis.high_water,
        (crate::node::MAX_PATH_CHECKS_PER_GRAPH + batch_paths) as u64
    );
    // A physical file-byte limit still refuses before any path is bound.
    let graph = GraphBudget::new();
    assert!(matches!(
        observe(&graph, |batch| batch.hash(&path, &metadata, 4)),
        Err(Error::Limit {
            resource: "runner artifact file bytes",
            ..
        })
    ));
    assert_eq!(graph.census_usage().unwrap(), (0, 0, 0));
    let graph = GraphBudget::new();
    assert!(observe(&graph, |batch| {
        batch.hash(&path, &metadata, 1024)?;
        Err::<(), _>(Error::State("injected operation failure"))
    })
    .is_err());
    assert!(graph.census_usage().unwrap().1 > 0);
    observe(&graph, |batch| batch.hash(&path, &metadata, 1024)).unwrap();
}
