use crate::p5;

use rawr_import::profile;

#[test]
fn synthetic_profile_contract_retains_only_exact_graph_evidence() {
    let _lane = crate::shared::lane();
    let project = p5::Project::new("profile-contract");
    let (_definition, contract) = p5::profile_contract(&project, p5::Mode::Success, None);
    let evidence = contract.evidence().collect::<Vec<_>>();
    assert_eq!(evidence.len(), 1);
    assert_eq!(evidence[0].kind(), profile::contract::Kind::Definition);
    assert_ne!(contract.rules_digest(), &[0; 32]);
    assert_ne!(contract.graph_fingerprint().as_bytes(), &[0; 32]);
}

/// rawr names and numbers its profiles itself, so an empty name or version
/// 0 is a rawr bug that panics naming the invariant (#538, D664).
#[test]
#[should_panic(expected = "rawr names each profile")]
fn an_unnamed_profile_is_a_rawr_bug() {
    let _ = profile::Identity::new("");
}

#[test]
#[should_panic(expected = "rawr numbers each profile's rules from 1")]
fn a_profile_version_zero_is_a_rawr_bug() {
    let _ = profile::Version::new(0);
}
