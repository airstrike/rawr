use rawr_import::profile::runtime::Runtime as _;

#[test]
fn rejected_or_stale_candidate_does_not_poison_the_next_prepare() {
    let limits = rawr_import::cargo::Limits::standard();
    let mut snapshot = rawr_import::surface::Snapshot::builder(&limits);
    snapshot.entry("application.rawr").unwrap();
    snapshot.file("application.rawr", b"last good").unwrap();
    let snapshot = snapshot.seal().unwrap();
    let key = rawr_import::supervisor::GenerationKey::fixture([0x11; 32]);
    let route = rawr_import::profile::activation::Route::fixture(key, [0x22; 32]);
    let mut runtime = rawr_iced::profile::Runtime;

    assert!(runtime
        .prepare(rawr_import::profile::runtime::Prepare::from_generation(
            key,
            snapshot,
            &rawr_import::profile::Proof::neutral(),
        ))
        .is_err());

    let definition = rawr_iced::profile::Definition::new(&limits).unwrap();
    let graph = rawr_import::test_fixture::iced_profile();
    let contract = rawr_import::profile::classify(&definition, &graph, &limits).unwrap();
    let checked = rawr_iced::profile::Checked::from_fingerprint([0x33; 32]);
    let demand = rawr_import::profile::demand(&definition, &contract, &checked, &limits).unwrap();
    let proof = rawr_import::profile::Proof::generate(&definition, &demand, &limits).unwrap();
    let prepared = runtime
        .prepare(rawr_import::profile::runtime::Prepare::from_generation(
            key, snapshot, &proof,
        ))
        .unwrap();
    let activated = runtime.activate(prepared, route).unwrap();
    let state = runtime.admit(activated, route).unwrap();
    assert_eq!(state.snapshot(), snapshot);
}
