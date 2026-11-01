//! Actual entry projection parity through the legacy and current adapters.

use crate::cargo::{self, Limits};
use crate::input::build::{Evidence, Package};
use crate::input::proof::{self, Entry, Role};
use crate::input::stored::Budget;
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
        &mut Budget::borrow(&limits, &mut allocation, &graph),
    )
    .expect("current projection");
    assert_eq!(actual.bytes(), expected.bytes());
    assert_eq!(actual.digest(), expected.digest());
}
