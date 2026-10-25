use rawr_import::profile;

#[test]
fn checked_application_seals_the_isolated_generated_profile() {
    let checked = rawr_iced::application::Compiler::new()
        .unwrap()
        .check_source(include_str!(
            "../../examples/scripts/application_counter.rs"
        ))
        .unwrap();
    let checked_profile = rawr_iced::profile::Checked::from_application(&checked);
    let limits = rawr_import::cargo::Limits::standard();
    let definition = rawr_iced::profile::Definition::new(&limits).unwrap();
    let graph = rawr_import::test_fixture::iced_profile();
    let contract = profile::classify(&definition, &graph, &limits).unwrap();
    let demand = profile::demand(&definition, &contract, &checked_profile, &limits).unwrap();
    let proof = profile::Proof::generate(&definition, &demand, &limits).unwrap();

    assert!(proof.has_generated_projections());
    assert!(proof.owns_product_lifecycle());
    assert_ne!(proof.fingerprint(), profile::Proof::neutral().fingerprint());
}
