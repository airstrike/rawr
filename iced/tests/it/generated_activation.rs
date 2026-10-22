use rawr_import::profile::runtime::Runtime as _;

#[test]
fn candidate_is_inert_until_the_exact_route_is_admitted() {
    let (proof, snapshot) = proof_and_snapshot();
    let key = rawr_import::supervisor::GenerationKey::fixture([1; 32]);
    let foreign_key = rawr_import::supervisor::GenerationKey::fixture([2; 32]);
    let route = rawr_import::profile::activation::Route::fixture(key, [3; 32]);
    let foreign = rawr_import::profile::activation::Route::fixture(foreign_key, [4; 32]);
    let mut runtime = rawr_iced::profile::Runtime;

    let prepared = runtime
        .prepare(rawr_import::profile::runtime::Prepare::from_generation(
            key, snapshot, &proof,
        ))
        .unwrap();
    assert!(runtime.activate(prepared, foreign).is_err());

    let prepared = runtime
        .prepare(rawr_import::profile::runtime::Prepare::from_generation(
            key, snapshot, &proof,
        ))
        .unwrap();
    let activated = runtime.activate(prepared, route).unwrap();
    assert!(runtime.admit(activated, foreign).is_err());

    let prepared = runtime
        .prepare(rawr_import::profile::runtime::Prepare::from_generation(
            key, snapshot, &proof,
        ))
        .unwrap();
    let activated = runtime.activate(prepared, route).unwrap();
    let state = runtime.admit(activated, route).unwrap();
    assert_eq!(state.route(), route);
    assert_eq!(state.snapshot(), snapshot);
    runtime.retire(state);
}

fn proof_and_snapshot() -> (rawr_import::profile::Proof, rawr_import::surface::Snapshot) {
    let limits = rawr_import::cargo::Limits::standard();
    let definition = rawr_iced::profile::Definition::new(&limits).unwrap();
    let graph = rawr_import::test_fixture::iced_profile();
    let contract = rawr_import::profile::classify(&definition, &graph, &limits).unwrap();
    let checked = rawr_iced::profile::Checked::from_fingerprint([5; 32]);
    let demand = rawr_import::profile::demand(&definition, &contract, &checked, &limits).unwrap();
    let proof = rawr_import::profile::Proof::generate(&definition, &demand, &limits).unwrap();
    let mut snapshot = rawr_import::surface::Snapshot::builder(&limits);
    snapshot.entry("application.rawr").unwrap();
    snapshot.file("application.rawr", b"view").unwrap();
    (proof, snapshot.seal().unwrap())
}
