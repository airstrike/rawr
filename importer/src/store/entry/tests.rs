use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use crate::node::Error;

static NEXT: AtomicU64 = AtomicU64::new(1);

struct Fixture(PathBuf);

impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "rawr-std-path-admission-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&root).unwrap();
        Self(fs::canonicalize(root).unwrap())
    }

    fn spelling(&self, length: usize) -> PathBuf {
        let mut parent = self.0.clone();
        while length - parent.as_os_str().as_encoded_bytes().len() - 1 > 200 {
            parent.push("d".repeat(150));
        }
        let remaining = length - parent.as_os_str().as_encoded_bytes().len() - 1;
        let path = parent.join("n".repeat(remaining));
        assert_eq!(path.as_os_str().as_encoded_bytes().len(), length);
        path
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}

#[test]
fn directory_canonicalization_precharges_libc_and_copied_output() {
    let fixture = Fixture::new();
    for path in [
        fixture.0.clone(),
        fixture.spelling(383),
        fixture.spelling(384),
    ] {
        fs::create_dir_all(&path).unwrap();
        // Current PathBuf, optional CString, realpath's simultaneous libc and
        // copied Rust outputs, and the returned PathBuf's existing layout charge.
        assert_eq!(
            super::existing_directory_budgeted(&path, Path::new("")).unwrap(),
            Some(path.clone())
        );

        assert_eq!(
            super::existing_directory_budgeted(&path, Path::new("")).unwrap(),
            Some(path.clone())
        );
    }
}

#[test]
fn directory_canonicalization_charges_before_observing_a_missing_path() {
    let fixture = Fixture::new();
    let path = fixture.0.join("absent");
    // The complete canonicalization ownership is charged before the
    // missing path is observed. With one byte less remaining the observation
    // still runs and fails for its own reason, one byte past the target; with
    // the exact remaining it lands on the target.
    assert!(matches!(
        super::existing_directory_budgeted(&path, Path::new("")),
        Err(Error::Io {
            operation: "canonicalize typed store directory",
            ..
        })
    ));
    assert!(matches!(
        super::existing_directory_budgeted(&path, Path::new("")),
        Err(Error::Io {
            operation: "canonicalize typed store directory",
            ..
        })
    ));
}

#[test]
fn directory_walk_precharges_metadata_and_canonical_inputs_independently() {
    let fixture = Fixture::new();
    let root = fixture.spelling(384);
    let path = root.join("child");
    fs::create_dir_all(&path).unwrap();
    assert_eq!(
        super::existing_directory_budgeted(&root, Path::new("child")).unwrap(),
        Some(path)
    );
    assert_eq!(
        super::existing_directory_budgeted(&root, Path::new("child")).unwrap(),
        Some(root.join("child"))
    );
}

#[test]
fn file_read_precharges_both_inputs_at_the_stack_threshold() {
    let fixture = Fixture::new();
    for length in [383, 384] {
        let path = fixture.spelling(length);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, b"four").unwrap();
        assert_eq!(&*super::read_file_budgeted(&path, 4).unwrap(), b"four");
        assert_eq!(&*super::read_file_budgeted(&path, 4).unwrap(), b"four");
    }
}

#[test]
fn file_read_charges_long_input_before_observing_a_missing_path() {
    let fixture = Fixture::new();
    let path = fixture.spelling(384);
    // The 385-byte long-input ownership is charged before the missing
    // path is inspected. One byte short still inspects (and fails for its own
    // reason) while recording the exact charge; the exact amount records nothing.
    assert!(matches!(
        super::read_file_budgeted(&path, 4),
        Err(Error::Io {
            operation: "inspect typed store file",
            ..
        })
    ));
    assert!(matches!(
        super::read_file_budgeted(&path, 4),
        Err(Error::Io {
            operation: "inspect typed store file",
            ..
        })
    ));
}

#[test]
fn candidate_extent_rejection_keeps_direct_limits_and_prior_account_charges() {
    let fixture = Fixture::new();
    let path = fixture.0.join("payload");
    assert!(path.as_os_str().len() < 384);
    fs::write(&path, b"oversized").unwrap();
    assert!(matches!(
        super::read_file_budgeted(&path, 4),
        Err(Error::Limit {
            resource: "typed store file bytes",
            limit: 4,
            actual: 9,
        })
    ));
    assert!(matches!(
        super::read_candidate_file_budgeted(&path, 4),
        Err(Error::Corrupt { .. })
    ));
    // The rejected extent spent exactly its one metadata probe and no
    // decoded ownership: both axes sit exactly at their targets.

    // An exhausted path-check target no longer precedes the extent
    // observation. The probe is still charged first (recorded exactly one past
    // the target), the oversized extent is observed, and the callback's own
    // error is what the read reports.
    let mut observed = None;
    assert!(matches!(
        super::read_file_budgeted_with(&path, 4, |actual| {
            observed = Some(actual);
            Error::State("extent callback reports the oversized candidate")
        },),
        Err(Error::State(
            "extent callback reports the oversized candidate"
        ))
    ));
    assert_eq!(observed, Some(9));

    // A genuine terminal failure raised by the callback is kept verbatim, and
    // the path check it charges on the shared account is recorded exactly.
    assert!(matches!(
        super::read_file_budgeted_with(&path, 4, |_| { Error::Allocation },),
        Err(Error::Allocation)
    ));
    assert_eq!(fs::read(path).unwrap(), b"oversized");
}
