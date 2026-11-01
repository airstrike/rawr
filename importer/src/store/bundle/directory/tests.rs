use std::cell::Cell;
use std::fs;
use std::sync::atomic::{AtomicU64, Ordering};

use rawr_directory::{Admission as _, Storage, Work};

use super::{caller, names, observe, output, Host};
use crate::node::{Error, GraphBudget, MAX_DECODED_BYTES_PER_GRAPH, MAX_PATH_CHECKS_PER_GRAPH};

static NEXT: AtomicU64 = AtomicU64::new(0);

fn decode_cap() -> usize {
    usize::try_from(MAX_DECODED_BYTES_PER_GRAPH).expect("the graph cap is addressable")
}

/// The decoded bytes charged, once past the standard decoded-byte target.
fn decoded_high_water(budget: &GraphBudget) -> Option<u64> {
    let decoded = budget.census_usage().unwrap().0;
    (decoded > MAX_DECODED_BYTES_PER_GRAPH).then_some(decoded)
}

/// The path checks charged, once past the standard path-check target.
fn paths_high_water(budget: &GraphBudget) -> Option<u64> {
    let paths = budget.census_usage().unwrap().1;
    (paths > MAX_PATH_CHECKS_PER_GRAPH).then_some(paths as u64)
}

/// Prove `budget` sits exactly at its decoded target: one more byte is
/// charged (never refused) and lands one past it.
fn assert_exact_decoded(budget: &GraphBudget) {
    budget.decode(1).expect("graph work never refuses");
    assert_eq!(
        decoded_high_water(budget),
        Some(MAX_DECODED_BYTES_PER_GRAPH + 1)
    );
}

#[test]
fn filesystem_observation_floor_uses_original_nonzero_epoch() {
    let budget = GraphBudget::new();
    budget
        .paths(MAX_PATH_CHECKS_PER_GRAPH - 1)
        .expect("prior work is admitted");
    let mut host = Host {
        budget: &budget,
        visible_limit: 1,
    };
    host.admit(Work::Open { path_bytes: 2 })
        .expect("the exact last observation is admitted");
    assert_eq!(paths_high_water(&budget), None);
    // Each further probe is still charged to the original account at
    // exactly one path check, never refused.
    for (excess, event) in [Work::Mount, Work::Read { buffer_bytes: 8192 }, Work::Record]
        .into_iter()
        .enumerate()
    {
        host.admit(event).expect("graph work never refuses");
        assert_eq!(
            paths_high_water(&budget),
            Some(MAX_PATH_CHECKS_PER_GRAPH as u64 + 1 + excess as u64)
        );
    }
}

#[test]
fn allocation_floor_charges_the_complete_new_layout_with_prior_ownership() {
    let budget = GraphBudget::new();
    budget
        .decode(decode_cap() - 80)
        .expect("prior ownership is admitted");
    let mut host = Host {
        budget: &budget,
        visible_limit: 1,
    };
    host.admit(Work::Allocate {
        storage: Storage::Entries,
        bytes: 64,
        alignment: 8,
    })
    .expect("complete replacement layout is charged");
    host.admit(Work::Allocate {
        storage: Storage::Name,
        bytes: 16,
        alignment: 1,
    })
    .expect("the exact remaining name layout is charged");
    assert_eq!(decoded_high_water(&budget), None);
    // One more byte is still charged to the original account, never refused.
    host.admit(Work::Allocate {
        storage: Storage::Name,
        bytes: 1,
        alignment: 1,
    })
    .expect("graph work never refuses");
    assert_eq!(
        decoded_high_water(&budget),
        Some(MAX_DECODED_BYTES_PER_GRAPH + 1)
    );
}

#[test]
fn pure_sort_and_temporary_union_retention_have_no_fictitious_probe_cost() {
    let budget = GraphBudget::new();
    budget
        .decode(17)
        .expect("nonzero prior ownership is admitted");
    let before = budget.census_vector().unwrap();
    let mut host = Host {
        budget: &budget,
        visible_limit: 1,
    };
    for entries in 1..=61 {
        host.admit(Work::Retain {
            entries,
            name_bytes: 4,
        })
        .expect("raw temporary count is not the visible-member limit");
    }
    for event in [
        Work::Name { bytes: 5 },
        Work::Copy { bytes: 8 },
        Work::Order,
        Work::Compare { bytes: 4 },
    ] {
        host.admit(event)
            .expect("bounded pure work spends no filesystem or candidate comparison count");
    }
    let after = budget.census_vector().unwrap();
    assert_eq!((after[1], after[4]), (before[1], before[4]));
    host.admit(Work::Visible { entries: 1 })
        .expect("61 raw entries can collapse to one visible member");
    assert!(matches!(
        host.admit(Work::Visible { entries: 2 }),
        Err(Error::Limit {
            resource: "runner bundle directory members",
            limit: 1,
            actual: 2
        })
    ));
}

#[test]
fn caller_byte_and_error_path_ownership_is_admitted_before_the_operation() {
    let path = std::path::Path::new("/directory-observation-must-not-run");
    let cost = std::mem::size_of::<std::path::PathBuf>()
        + 2 * path.as_os_str().as_encoded_bytes().len()
        + 1;
    let budget = GraphBudget::new();
    budget
        .decode(decode_cap() - cost + 1)
        .expect("nonzero prior ownership is admitted");
    // The caller ownership is still charged before the operation runs;
    // the operation then proceeds and the exact total is charged, never refused.
    let observed = Cell::new(None);
    let result = observe(path, 1, &budget, |_, _, host| {
        observed.set(decoded_high_water(host.budget));
        Err(rawr_directory::Error::Terminal)
    });
    assert!(matches!(
        result,
        Err(Error::State("native directory reader is terminal"))
    ));
    assert_eq!(observed.get(), Some(MAX_DECODED_BYTES_PER_GRAPH + 1));
    assert_eq!(
        decoded_high_water(&budget),
        Some(MAX_DECODED_BYTES_PER_GRAPH + 1)
    );

    let exact = GraphBudget::new();
    exact
        .decode(decode_cap() - cost)
        .expect("exact prior ownership is admitted");
    let (bytes, owned_path) =
        caller(path, &exact).expect("both exact caller allocations are admitted");
    assert_eq!(owned_path, path);
    assert_eq!(bytes.last(), Some(&0));
    assert_eq!(
        &bytes[..bytes.len() - 1],
        path.as_os_str().as_encoded_bytes()
    );
    assert_exact_decoded(&exact);
}

#[test]
fn exhausted_original_observation_account_records_the_native_open() {
    let budget = GraphBudget::new();
    budget
        .decode(23)
        .expect("prior caller ownership is admitted");
    budget
        .paths(MAX_PATH_CHECKS_PER_GRAPH)
        .expect("prior observations exhaust the original epoch");
    assert_eq!(paths_high_water(&budget), None);
    let missing = std::path::Path::new("/rawr-directory-nonexistent-open-fixture");
    // The exhausted target never refuses the open. The failed native open
    // is charged exactly one path check, and the operation reports its own
    // filesystem error.
    let result = names(missing, 1, &budget);
    assert!(
        matches!(result, Err(Error::Io { path, source, .. }) if path == missing && source.kind() == std::io::ErrorKind::NotFound)
    );
    assert_eq!(
        paths_high_water(&budget),
        Some(MAX_PATH_CHECKS_PER_GRAPH as u64 + 1)
    );
}

#[test]
fn output_container_and_joined_paths_are_precharged_at_their_exact_floor() {
    let output_bytes = 2 * std::mem::size_of::<std::ffi::OsString>();
    let budget = GraphBudget::new();
    budget
        .decode(decode_cap() - output_bytes + 1)
        .expect("prior ownership is admitted");
    assert!(
        output(2, &budget)
            .expect("graph work never refuses")
            .capacity()
            >= 2
    );
    assert_eq!(
        decoded_high_water(&budget),
        Some(MAX_DECODED_BYTES_PER_GRAPH + 1)
    );
    let exact = GraphBudget::new();
    exact
        .decode(decode_cap() - output_bytes)
        .expect("exact prior ownership is admitted");
    assert!(
        output(2, &exact)
            .expect("exact output container is admitted")
            .capacity()
            >= 2
    );
    assert_exact_decoded(&exact);

    let parent = std::path::Path::new("/exact-parent");
    let child = std::path::Path::new("native-child");
    let cost = std::mem::size_of::<std::path::PathBuf>()
        + parent.as_os_str().as_encoded_bytes().len()
        + child.as_os_str().as_encoded_bytes().len()
        + 1;
    let budget = GraphBudget::new();
    budget
        .decode(decode_cap() - cost + 1)
        .expect("prior joined-path ownership is admitted");
    assert_eq!(
        super::super::path(parent, child, &budget).expect("graph work never refuses"),
        parent.join(child)
    );
    assert_eq!(
        decoded_high_water(&budget),
        Some(MAX_DECODED_BYTES_PER_GRAPH + 1)
    );
    let exact = GraphBudget::new();
    exact
        .decode(decode_cap() - cost)
        .expect("exact prior path ownership is admitted");
    assert_eq!(
        super::super::path(parent, child, &exact).expect("exact path is admitted"),
        parent.join(child)
    );
    assert_exact_decoded(&exact);
}

#[test]
fn real_adapter_observation_preserves_names_and_rejects_visible_excess() {
    let fixture = std::env::temp_dir().join(format!(
        "rawr-directory-budget-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir(&fixture).expect("the inert fixture directory exists");
    fs::write(fixture.join("z"), []).expect("one inert member exists");
    fs::create_dir(fixture.join("a")).expect("an empty directory exists");
    let budget = GraphBudget::new();
    budget
        .paths(7)
        .expect("the original epoch has prior observations");
    budget
        .decode(19)
        .expect("the original epoch has prior ownership");
    assert_eq!(
        names(&fixture, 2, &budget).expect("the complete directory is observed"),
        [std::ffi::OsString::from("a"), std::ffi::OsString::from("z")]
    );
    assert!(matches!(
        names(&fixture, 1, &budget),
        Err(Error::Limit {
            resource: "runner bundle directory members",
            limit: 1,
            actual: 2
        })
    ));
    fs::remove_dir_all(&fixture).expect("the inert fixture retires");
}

#[test]
fn production_bundle_names_and_paths_preserve_complete_sorted_members() {
    let fixture = std::env::temp_dir().join(format!(
        "rawr-directory-entry-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir(&fixture).unwrap();
    fs::write(fixture.join("z"), []).unwrap();
    fs::create_dir(fixture.join(".fingerprint")).unwrap();
    fs::create_dir(fixture.join("empty")).unwrap();
    let budget = GraphBudget::new();
    budget.paths(7).unwrap();
    budget.decode(19).unwrap();
    let expected = [".fingerprint", "empty", "z"];
    assert_eq!(
        super::super::names(&fixture, 3, &budget).unwrap(),
        expected.map(std::ffi::OsString::from)
    );
    assert_eq!(
        super::super::paths(&fixture, 3, &budget).unwrap(),
        expected.map(|name| fixture.join(name))
    );
    for result in [
        super::super::names(&fixture, 2, &budget).map(|_| ()),
        super::super::paths(&fixture, 2, &budget).map(|_| ()),
    ] {
        assert!(matches!(
            result,
            Err(Error::Limit {
                resource: "runner bundle directory members",
                limit: 2,
                actual: 3
            })
        ));
    }
    fs::remove_dir_all(fixture).unwrap();
}

#[test]
fn production_bundle_paths_precharge_output_and_path_bytes_after_names() {
    let fixture = std::env::temp_dir().join(format!(
        "rawr-directory-path-floor-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir(&fixture).unwrap();
    fs::write(fixture.join("a"), []).unwrap();
    let observed = GraphBudget::new();
    observed.decode(17).unwrap();
    super::super::names(&fixture, 1, &observed).unwrap();
    // The census reveals current usage without mutating it. This measures
    // actual names ownership rather than copying the foundation's growth rule.
    let (decoded, _, _) = observed.census_usage().unwrap();
    let names_bytes = usize::try_from(decoded).unwrap() - 17;
    assert_eq!(decoded_high_water(&observed), None);
    let output_and_path = std::mem::size_of::<std::path::PathBuf>()
        + fixture.as_os_str().as_encoded_bytes().len()
        + 2;
    for excess in [0, 1] {
        let budget = GraphBudget::new();
        budget.paths(11).unwrap();
        budget
            .decode(decode_cap() - names_bytes - output_and_path + excess)
            .unwrap();
        let result = super::super::paths(&fixture, 1, &budget);
        assert_eq!(result.unwrap(), [fixture.join("a")]);
        if excess == 0 {
            assert_exact_decoded(&budget);
        } else {
            // One byte less remaining still completes the operation and the
            // charged total lands one past the target.
            assert_eq!(
                decoded_high_water(&budget),
                Some(MAX_DECODED_BYTES_PER_GRAPH + 1)
            );
        }
    }
    fs::remove_dir_all(fixture).unwrap();
}

#[test]
fn production_bundle_entry_points_preserve_original_budget_and_io_errors() {
    let missing = std::path::Path::new("/rawr-directory-missing-production-entry");
    let budget = GraphBudget::new();
    budget.decode(23).unwrap();
    budget.paths(MAX_PATH_CHECKS_PER_GRAPH).unwrap();
    // Both entry points keep charging the original account past its
    // target (one path check per failed open) and report the filesystem error.
    for result in [
        super::super::names(missing, 1, &budget).map(|_| ()),
        super::super::paths(missing, 1, &budget).map(|_| ()),
    ] {
        assert!(
            matches!(result, Err(Error::Io { path, source, .. }) if path == missing && source.kind() == std::io::ErrorKind::NotFound)
        );
    }
    assert_eq!(
        paths_high_water(&budget),
        Some(MAX_PATH_CHECKS_PER_GRAPH as u64 + 2)
    );
    let budget = GraphBudget::new();
    budget.paths(5).unwrap();
    budget.decode(29).unwrap();
    for result in [
        super::super::names(missing, 1, &budget).map(|_| ()),
        super::super::paths(missing, 1, &budget).map(|_| ()),
    ] {
        assert!(
            matches!(result, Err(Error::Io { path, source, .. }) if path == missing && source.kind() == std::io::ErrorKind::NotFound)
        );
    }
}
