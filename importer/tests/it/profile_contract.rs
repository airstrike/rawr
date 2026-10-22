use crate::p5;

use rawr_import::profile;

#[test]
fn synthetic_profile_contract_retains_only_exact_graph_evidence() {
    let _turn = rawr_import::store::test_turn();
    let project = p5::Project::new("profile-contract");
    let (_definition, contract, limits) = p5::profile_contract(&project, p5::Mode::Success, None);
    let evidence = contract.evidence().collect::<Vec<_>>();
    assert_eq!(evidence.len(), 1);
    assert_eq!(evidence[0].kind(), profile::contract::Kind::Definition);
    assert_ne!(contract.rules_digest(), &[0; 32]);
    assert_ne!(contract.graph_fingerprint().as_bytes(), &[0; 32]);
    assert!(profile::Identity::new("", &limits).is_err());
    assert!(profile::Version::new(0).is_err());
}
