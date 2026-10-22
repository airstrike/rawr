use rawr_import::profile::Proof;

#[test]
fn neutral_profile_is_the_default_and_emits_nothing() {
    let explicit = Proof::neutral();
    let default = Proof::default();
    assert_eq!(default.fingerprint(), explicit.fingerprint());
    assert!(!explicit.has_generated_projections());
    assert!(!default.has_generated_projections());
}
