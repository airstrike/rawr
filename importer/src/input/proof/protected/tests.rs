//! Actual entry projection parity through the legacy and current adapters.

use crate::cargo::{self, Limits};
use crate::input::build::{Evidence, Package};
use crate::input::proof::{self, Entry, Role};
use crate::input::stored::{Budget, Error};
use crate::input::{Allocation, Usage};
use crate::node;

fn fixture() -> (Box<[Entry]>, Evidence, cargo::universe::MetadataProof) {
    let root = std::path::Path::new("/fixture/importer");
    let id = "path+file:///fixture/importer#rawr-import@0.1.0";
    let entries = Box::from([
        Entry::File {
            role: Role::DependencySource,
            package_id: Some(id.into()),
            path: "/fixture/importer/src/lib.rs".into(),
            bytes: 3,
            digest: [7; 32],
        },
        Entry::Fact {
            role: Role::DependencySource,
            package_id: Some(id.into()),
            name: format!("package:{id}").into(),
            value: "/fixture/importer".into(),
        },
    ]);
    let evidence = Evidence {
        usage: Usage { items: 0, bytes: 0 },
        packages: [(
            id.into(),
            Package {
                manifest: root.join("Cargo.toml"),
                root: root.into(),
                build_script: None,
            },
        )]
        .into(),
        scripts: Box::default(),
        artifacts: Box::default(),
        environment: Box::default(),
    };
    (
        entries,
        evidence,
        cargo::universe::MetadataProof::fixture_package(
            id,
            "rawr-import",
            "0.1.0",
            "",
            root.into(),
        ),
    )
}

#[test]
fn current_projector_matches_the_complete_legacy_entry_projection() {
    let (entries, evidence, protected) = fixture();
    let limits = Limits::standard();
    let mut legacy = Allocation::new(&limits);
    let expected = proof::protected_entries(
        &entries,
        &evidence,
        &protected,
        "aarch64-apple-darwin",
        &limits,
        &mut legacy,
    )
    .expect("legacy projection");
    let mut allocation = Allocation::for_proof(
        Usage {
            items: 7,
            bytes: 31,
        },
        &limits,
    );
    let graph = node::GraphBudget::new();
    let actual = super::project::current(
        &entries,
        &evidence,
        &protected,
        "aarch64-apple-darwin",
        &limits,
        &mut Budget::borrow(&limits, &mut allocation, &graph),
    )
    .expect("current projection");
    assert_eq!(actual.bytes(), expected.bytes());
    assert_eq!(actual.digest(), expected.digest());
}

#[test]
fn current_projection_keeps_prior_usage_and_records_exceedances_at_exact_floors() {
    let (entries, evidence, protected) = fixture();
    let limits = Limits::standard();
    let prior = Usage {
        items: 7,
        bytes: 31,
    };
    let mut measured = Allocation::for_proof(prior, &limits);
    super::project::current(
        &entries,
        &evidence,
        &protected,
        "aarch64-apple-darwin",
        &limits,
        &mut Budget::borrow(&limits, &mut measured, &node::GraphBudget::new()),
    )
    .expect("measure current admitted work");
    let used = measured.usage();
    for (items, bytes) in [
        (used.items, used.bytes),
        (used.items - 1, used.bytes),
        (used.items, used.bytes - 1),
    ] {
        let mut limited = limits;
        limited.items = items;
        limited.input_proof_bytes = bytes;
        let mut allocation = Allocation::for_proof(prior, &limited);
        let graph = node::GraphBudget::new();
        let mut budget = Budget::borrow(&limits, &mut allocation, &graph);
        let result = super::project::current(
            &entries,
            &evidence,
            &protected,
            "aarch64-apple-darwin",
            &limits,
            &mut budget,
        );
        // Every floor still projects; prior usage is kept and the exact
        // measured total is charged again.
        assert!(
            result.is_ok(),
            "accounting targets are recorded, never refused"
        );
        let record = budget.exceedances();
        budget
            .check()
            .expect("an accounting exceedance never latches the account");
        assert_eq!(allocation.usage(), used);
        if (items, bytes) == (used.items, used.bytes) {
            assert!(!record.is_exceeded());
        } else {
            // One under records that axis alone with a high water equal to
            // the exact measured total.
            let (exceeded, untouched, total) = if items < used.items {
                (
                    crate::input::INPUT_ITEMS_RESOURCE,
                    crate::input::INPUT_BYTES_RESOURCE,
                    used.items,
                )
            } else {
                (
                    crate::input::INPUT_BYTES_RESOURCE,
                    crate::input::INPUT_ITEMS_RESOURCE,
                    used.bytes,
                )
            };
            let axis = *record
                .axis(exceeded)
                .expect("the one-under axis records the exact total");
            assert_eq!(
                (axis.target, axis.high_water),
                ((total - 1) as u64, total as u64)
            );
            assert!(record.axis(untouched).is_none());
            // A genuine failure latches the account; a latched account does
            // no further projection work.
            allocation.refuse();
            assert!(matches!(
                super::project::current(
                    &[],
                    &evidence,
                    &protected,
                    "aarch64-apple-darwin",
                    &limits,
                    &mut Budget::borrow(&limits, &mut allocation, &graph)
                ),
                Err(Error::Refused)
            ));
        }
    }
}
