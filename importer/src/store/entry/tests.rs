use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use crate::node::{self, Error, GraphBudget};

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

fn budget(remaining: usize, paths: usize) -> GraphBudget {
    let budget = GraphBudget::new();
    budget
        .decode(node::MAX_DECODED_BYTES_PER_GRAPH as usize - remaining)
        .unwrap();
    budget
        .paths(node::MAX_PATH_CHECKS_PER_GRAPH - paths)
        .unwrap();
    budget
}

/// The decoded-bytes axis high water once the graph crossed its target.
fn decoded_high_water(budget: &GraphBudget) -> Option<u64> {
    budget
        .exceedances()
        .axis(node::DECODED_RESOURCE)
        .map(|axis| axis.high_water)
}

/// The path-check axis high water once the graph crossed its target.
fn paths_high_water(budget: &GraphBudget) -> Option<u64> {
    budget
        .exceedances()
        .axis(node::PATHS_RESOURCE)
        .map(|axis| axis.high_water)
}

/// `budget` sits exactly at its decoded target: nothing is recorded, and
/// one more byte is still charged while recording the exact cumulative total.
fn assert_exact_decoded(budget: &GraphBudget) {
    assert_eq!(decoded_high_water(budget), None);
    budget.decode(1).expect("an accounting axis never refuses");
    assert_eq!(
        decoded_high_water(budget),
        Some(node::MAX_DECODED_BYTES_PER_GRAPH + 1)
    );
}

/// `budget` sits exactly at its path-check target.
fn assert_exact_paths(budget: &GraphBudget) {
    assert_eq!(paths_high_water(budget), None);
    budget.paths(1).expect("an accounting axis never refuses");
    assert_eq!(
        paths_high_water(budget),
        Some(node::MAX_PATH_CHECKS_PER_GRAPH as u64 + 1)
    );
}

/// One byte less remaining still completes the operation; the exceedance
/// records exactly one byte past the target, proving the exact charge.
fn assert_one_over_decoded(budget: &GraphBudget) {
    assert_eq!(
        decoded_high_water(budget),
        Some(node::MAX_DECODED_BYTES_PER_GRAPH + 1)
    );
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
        let length = path.as_os_str().as_encoded_bytes().len();
        let input = if length >= 384 { length + 1 } else { 0 };
        // Current PathBuf, optional CString, realpath's simultaneous libc and
        // copied Rust outputs, and the returned PathBuf's existing layout charge.
        let required = 2 * std::mem::size_of::<PathBuf>() + length + 1 + input + 1024 + 1023;
        let exact = budget(required, 1);
        assert_eq!(
            super::existing_directory_budgeted(&path, Path::new(""), &exact).unwrap(),
            Some(path.clone())
        );
        assert_exact_decoded(&exact);
        assert_exact_paths(&exact);

        let floor = budget(required - 1, 1);
        assert_eq!(
            super::existing_directory_budgeted(&path, Path::new(""), &floor).unwrap(),
            Some(path.clone())
        );
        assert_one_over_decoded(&floor);
    }
}

#[test]
fn directory_canonicalization_charges_before_observing_a_missing_path() {
    let fixture = Fixture::new();
    let path = fixture.0.join("absent");
    let required = 2 * std::mem::size_of::<PathBuf>()
        + path.as_os_str().as_encoded_bytes().len()
        + 1
        + 1024
        + 1023;
    // The complete canonicalization ownership is charged before the
    // missing path is observed. With one byte less remaining the observation
    // still runs and fails for its own reason, while the exceedance records the
    // exact charge; with the exact remaining nothing is recorded.
    let floor = budget(required - 1, 1);
    assert!(matches!(
        super::existing_directory_budgeted(&path, Path::new(""), &floor),
        Err(Error::Io {
            operation: "canonicalize typed store directory",
            ..
        })
    ));
    assert_one_over_decoded(&floor);
    let exact = budget(required, 1);
    assert!(matches!(
        super::existing_directory_budgeted(&path, Path::new(""), &exact),
        Err(Error::Io {
            operation: "canonicalize typed store directory",
            ..
        })
    ));
    assert_eq!(decoded_high_water(&exact), None);
}

#[test]
fn directory_walk_precharges_metadata_and_canonical_inputs_independently() {
    let fixture = Fixture::new();
    let root = fixture.spelling(384);
    let path = root.join("child");
    fs::create_dir_all(&path).unwrap();
    let length = path.as_os_str().as_encoded_bytes().len();
    let required = 2 * std::mem::size_of::<PathBuf>() + length + 2 * (length + 1) + 1024 + 1023;
    let exact = budget(required, 2);
    assert_eq!(
        super::existing_directory_budgeted(&root, Path::new("child"), &exact).unwrap(),
        Some(path)
    );
    assert_exact_decoded(&exact);
    assert_exact_paths(&exact);
    let floor = budget(required - 1, 2);
    assert_eq!(
        super::existing_directory_budgeted(&root, Path::new("child"), &floor).unwrap(),
        Some(root.join("child"))
    );
    assert_one_over_decoded(&floor);
}

#[test]
fn file_read_precharges_both_inputs_at_the_stack_threshold() {
    let fixture = Fixture::new();
    for length in [383, 384] {
        let path = fixture.spelling(length);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, b"four").unwrap();
        let input = if length >= 384 { length + 1 } else { 0 };
        let required = 4 + 2 * input;
        let exact = budget(required, 2);
        assert_eq!(
            &*super::read_file_budgeted(&path, 4, &exact).unwrap(),
            b"four"
        );
        assert_exact_decoded(&exact);
        assert_exact_paths(&exact);
        let floor = budget(required - 1, 2);
        assert_eq!(
            &*super::read_file_budgeted(&path, 4, &floor).unwrap(),
            b"four"
        );
        assert_one_over_decoded(&floor);
    }
}

#[test]
fn file_read_charges_long_input_before_observing_a_missing_path() {
    let fixture = Fixture::new();
    let path = fixture.spelling(384);
    // The 385-byte long-input ownership is charged before the missing
    // path is inspected. One byte short still inspects (and fails for its own
    // reason) while recording the exact charge; the exact amount records nothing.
    let floor = budget(384, 1);
    assert!(matches!(
        super::read_file_budgeted(&path, 4, &floor),
        Err(Error::Io {
            operation: "inspect typed store file",
            ..
        })
    ));
    assert_one_over_decoded(&floor);
    let exact = budget(385, 1);
    assert!(matches!(
        super::read_file_budgeted(&path, 4, &exact),
        Err(Error::Io {
            operation: "inspect typed store file",
            ..
        })
    ));
    assert_eq!(decoded_high_water(&exact), None);
}

#[test]
fn candidate_extent_rejection_keeps_direct_limits_and_prior_account_charges() {
    let fixture = Fixture::new();
    let path = fixture.0.join("payload");
    assert!(path.as_os_str().len() < 384);
    fs::write(&path, b"oversized").unwrap();
    assert!(matches!(
        super::read_file_budgeted(&path, 4, &GraphBudget::new()),
        Err(Error::Limit {
            resource: "typed store file bytes",
            limit: 4,
            actual: 9,
        })
    ));
    let graph = budget(0, 1);
    assert!(matches!(
        super::read_candidate_file_budgeted(&path, 4, &graph),
        Err(Error::Corrupt { .. })
    ));
    // The rejected extent spent exactly its one metadata probe and no
    // decoded ownership: both axes sit exactly at their targets.
    assert_exact_paths(&graph);
    assert_exact_decoded(&graph);

    // An exhausted path-check target no longer precedes the extent
    // observation. The probe is still charged first (recorded exactly one past
    // the target), the oversized extent is observed, and the callback's own
    // error is what the read reports.
    let graph = budget(0, 0);
    let mut observed = None;
    assert!(matches!(
        super::read_file_budgeted_with(&path, 4, &graph, |actual| {
            observed = Some((actual, paths_high_water(&graph)));
            Error::State("extent callback reports the oversized candidate")
        }),
        Err(Error::State(
            "extent callback reports the oversized candidate"
        ))
    ));
    assert_eq!(
        observed,
        Some((9, Some(node::MAX_PATH_CHECKS_PER_GRAPH as u64 + 1)))
    );
    assert_eq!(decoded_high_water(&graph), None);

    // A genuine terminal failure raised by the callback is kept verbatim, and
    // the path check it charges on the shared account is recorded exactly.
    let graph = budget(0, 1);
    assert!(matches!(
        super::read_file_budgeted_with(&path, 4, &graph, |_| {
            graph.paths(1).expect("an accounting axis never refuses");
            Error::Allocation
        }),
        Err(Error::Allocation)
    ));
    assert_eq!(
        paths_high_water(&graph),
        Some(node::MAX_PATH_CHECKS_PER_GRAPH as u64 + 1)
    );
    assert_eq!(fs::read(path).unwrap(), b"oversized");
}
