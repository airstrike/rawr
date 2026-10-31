use std::path::{Path, PathBuf};

use rawr_import::applicability::{inventory::Acquired, Catalog};
use rawr_import::cargo::config::{Dependency, Environment};
use rawr_import::cargo::{Limits, Request};
use rawr_import::extract::Policy;
use rawr_import::process::Native;

#[test]
fn independent_intrinsic_domains_keep_full_predicates_and_associated_links() {
    let _turn = rawr_import::store::test_turn();
    rawr_import::store::isolate_for_tests();
    let limits = Limits::standard();
    let native = Native::pinned().expect("the pinned driver toolchain is installed");
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../target/tmp/independent-inventories");
    std::fs::create_dir_all(&root).unwrap();
    let first = capture(&root.join("ab"), &["a", "b"], &native, &limits);
    let second = capture(&root.join("cb"), &["c", "b"], &native, &limits);
    assert_eq!(first.partitions(), 2);
    assert_eq!(second.partitions(), 2);
    assert!(first
        .definitions()
        .all(|definition| definition.id().unit().crate_name() != "facts_c"));
    assert!(second
        .definitions()
        .all(|definition| definition.id().unit().crate_name() != "facts_a"));
    assert!(first.definitions().any(|definition| definition
        .display_path()
        .any(|segment| segment == "Carrier")));
    assert!(first.implementations().any(|implementation| implementation
        .generics()
        .predicates()
        .len()
        != 0));
    assert!(second.implementations().any(|implementation| implementation
        .generics()
        .predicates()
        .len()
        != 0));
    let common = |inventory: &Acquired| {
        inventory
            .definitions()
            .filter(|definition| definition.id().unit().crate_name() == "facts_b")
            .map(|definition| (definition.id().clone(), definition.clone()))
            .collect::<std::collections::BTreeMap<_, _>>()
    };
    assert_eq!(
        common(&first),
        common(&second),
        "the independently obtained B facts have exact stable identity and contents"
    );
    eprintln!("INDEPENDENT_INVENTORIES ab_definitions={} ab_implementations={} cb_definitions={} cb_implementations={}",
        first.definitions().count(), first.implementations().count(),
        second.definitions().count(), second.implementations().count());
    let inventories = [first, second];
    let catalog = Catalog::join(&inventories).expect("independent exact facts compose");
    assert_eq!(catalog.work().2, 3);
    assert!(catalog.export("facts_a", &["Alias"]).is_some());
    assert!(catalog.export("facts_c", &["Left"]).is_some());
}

fn capture(root: &Path, selected: &[&str], native: &Native, limits: &Limits) -> Acquired {
    std::fs::create_dir_all(root).unwrap();
    let root = std::fs::canonicalize(root).unwrap();
    let environment = Environment::capture()
        .unwrap()
        .setting("CARGO_NET_OFFLINE", "true")
        .unwrap();
    let mut builder = Request::builder(&root, &root.join("target/rawr/cache"), limits)
        .unwrap()
        .inventory()
        .environment(environment)
        .policy(Policy::manifest(limits).seal())
        .unwrap();
    for name in selected {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures")
            .join(format!("applicability-{name}"));
        builder = builder
            .dependency(
                Dependency::path(&format!("facts_{name}"), &path, limits)
                    .unwrap()
                    .package(&format!("facts-{name}"))
                    .unwrap(),
            )
            .unwrap();
    }
    Acquired::capture(builder.build().unwrap(), native, limits)
        .expect("the independent intrinsic domain authenticates")
}
