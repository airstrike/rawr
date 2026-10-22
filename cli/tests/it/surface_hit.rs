use crate::p3;

#[test]
fn unchanged_surface_hot_admits_with_zero_cargo_path() {
    let _turn = rawr_import::store::test_turn();
    let project = rawr_import::store::TestProject::new("rawr-cli-surface-hit");
    let root = project.path();
    let chain = p3::dag_chain(root);
    let (initial, snapshot) =
        p3::dag_request_at(root, &chain, b"initial Rawr body", p3::Surface::Complete);
    let generated = initial
        .generate(&rawr_import::cargo::Limits::standard())
        .expect("the initial runner generates");
    let mut lane = rawr_cli::watch::native::Lane::new(root).expect("the native lane binds");
    let initial = lane
        .replace(generated, snapshot)
        .expect("the initial runner admits");
    let (same, snapshot) = p3::dag_request_at(
        root,
        &chain,
        b"Rawr body changed while native demand stayed exact",
        p3::Surface::Complete,
    );
    let rawr_import::process::hot::Outcome::Hit(hot) = lane
        .compile(same, snapshot)
        .expect("the existing runner accepts the snapshot")
    else {
        panic!("the same BuildRequestKey must bypass Cargo")
    };
    assert_ne!(initial, hot);
    assert_eq!(lane.current_key(), Some(hot));
    lane.shutdown().expect("the hot runner retires");
}
