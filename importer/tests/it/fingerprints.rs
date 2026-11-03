use crate::p2;

use rawr_import::classify::Policy;

/// The shared contract's extraction and a second one in another project give
/// the same raw and classified evidence.
#[test]
fn real_extraction_and_contract_fingerprints_repeat_semantically() {
    let limits = rawr_import::cargo::Limits::standard();
    let first = p2::contract();
    let second = p2::graphs(1).pop().expect("the repeated real graph exists");

    assert_eq!(second.fingerprint(), first.raw_fingerprint());
    assert_eq!(
        second.canonical_bytes(),
        first.raw_graph().canonical_bytes()
    );
    let second = second
        .classify(&Policy::standard(), &limits)
        .expect("the repeated real graph classifies");
    assert_eq!(first.raw_fingerprint(), second.raw_fingerprint());
    assert_eq!(first.fingerprint(), second.fingerprint());
    assert_eq!(first.canonical_bytes(), second.canonical_bytes());

    let first_alias = p2::outcome(first, "facade_a", &["RenamedNode"]);
    let second_alias = p2::outcome(&second, "facade_a", &["RenamedNode"]);
    assert_eq!(first_alias.identity(), second_alias.identity());
}
