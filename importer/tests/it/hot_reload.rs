use crate::p3;

#[test]
fn same_surface_hot_admits_and_changed_surface_returns_one_cold_continuation() {
    let _turn = rawr_import::store::test_turn();
    let project = rawr_import::store::TestProject::new("rawr-p4-hot");
    let root = project.path().to_path_buf();
    let cache = rawr_import::cache::Writer::new(&root).expect("the cache binds");
    // Hot admission runs under the shared node context the product always has.
    let chain = p3::dag_chain(&root);
    let (initial, initial_snapshot) =
        p3::dag_request_at(&root, &chain, b"initial body", p3::Surface::Complete);
    let generated = initial
        .generate(&rawr_import::cargo::Limits::standard())
        .expect("the initial surface generates");
    let cargo = rawr_import::cargo::Owner::acquire().expect("the Cargo lane is free");
    let built = generated
        .build(&cargo, &cache)
        .expect("the initial surface builds exactly once");
    drop(cargo);

    let process = rawr_import::process::Owner::new(&root).expect("the process owner binds");
    let mut supervisor = rawr_import::supervisor::Owner::new();
    let mut current = supervisor
        .bootstrap(built, initial_snapshot, &process)
        .expect("the initial runner admits");
    let initial = current.key();

    let (same, same_snapshot) = p3::dag_request_at(
        &root,
        &chain,
        b"body changed without changing native demand",
        p3::Surface::Complete,
    );
    let hot = current
        .compile(same, same_snapshot)
        .expect("the same surface hot-admits");
    let rawr_import::process::hot::Outcome::Hit(hot_key) = hot else {
        panic!("the identical bridge surface must not invoke Cargo")
    };
    assert_ne!(initial, hot_key);
    assert_eq!(current.key(), hot_key);

    let (changed, changed_snapshot) = p3::dag_request_at(
        &root,
        &chain,
        b"new demand requires a cold surface",
        p3::Surface::First,
    );
    let miss = current
        .compile(changed, changed_snapshot)
        .expect("surface comparison itself is side-effect free");
    let rawr_import::process::hot::Outcome::Miss(miss) = miss else {
        panic!("a changed adapter table must close the hot gate")
    };
    assert_eq!(current.key(), hot_key);
    let (changed, changed_snapshot) = miss.into_parts();
    let generated = changed
        .generate(&rawr_import::cargo::Limits::standard())
        .expect("the sole cold continuation generates");
    let cargo = rawr_import::cargo::Owner::acquire().expect("the Cargo lane is serialized");
    let changed = generated
        .build(&cargo, &cache)
        .expect("the coalesced miss builds once");
    drop(cargo);
    let replacement = supervisor
        .bootstrap(changed, changed_snapshot, &process)
        .expect("the miss admits only through a fresh epoch");
    assert_ne!(replacement.key(), current.key());
    current.shutdown().expect("the hot epoch retires");
    replacement.shutdown().expect("the replacement retires");
}
