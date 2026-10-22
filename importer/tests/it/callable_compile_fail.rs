use crate::p4;

use rawr_import::classify::representability::{reject, Representability};

#[test]
fn escaping_hrtb_and_nonstatic_mutation_reject_explicitly() {
    let _turn = rawr_import::store::test_turn();
    let (_project, contract, _limits) = p4::contract();
    let rejection = |name| {
        let outcome = contract
            .outcomes()
            .find(|outcome| outcome.export().path().eq([name]))
            .expect("the rejected callable fixture remains positioned");
        let Representability::Rejected(reason) = outcome.representability() else {
            panic!("the unsupported callable protocol must reject")
        };
        reason.kind()
    };
    assert_eq!(rejection("escaping"), reject::Kind::EscapingBorrow);
    assert_eq!(
        rejection("nonstatic_mutable"),
        reject::Kind::NonStaticCallable
    );
}
