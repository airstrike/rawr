use crate::p2;

use rawr_import::classify::Policy;

#[test]
fn real_extraction_and_contract_fingerprints_repeat_semantically() {
    let _turn = rawr_import::store::test_turn();
    let limits = rawr_import::cargo::Limits::standard();
    let mut graphs = p2::graphs(2);
    let second = graphs.pop().expect("the repeated real graph exists");
    let first = graphs.pop().expect("the first real graph exists");
    let first_raw = first.fingerprint();
    let first_bytes = first.canonical_bytes().to_owned();
    let first = first
        .classify(&Policy::standard(), &limits)
        .expect("the first real graph classifies");

    assert_eq!(second.fingerprint(), first_raw);
    assert_eq!(second.canonical_bytes(), first_bytes);
    let second = second
        .classify(&Policy::standard(), &limits)
        .expect("the repeated real graph classifies");
    assert_eq!(first.raw_fingerprint(), second.raw_fingerprint());
    assert_eq!(first.fingerprint(), second.fingerprint());
    assert_eq!(first.canonical_bytes(), second.canonical_bytes());

    let first_alias = p2::outcome(&first, "facade_a", &["RenamedNode"]);
    let second_alias = p2::outcome(&second, "facade_a", &["RenamedNode"]);
    assert_eq!(first_alias.identity(), second_alias.identity());
}
