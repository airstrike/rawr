use crate::p2;

use rawr_import::classify::representability::Representability;
use rawr_import::classify::Policy;

#[test]
fn real_symbolic_selectors_are_exact_and_never_create_authority() {
    let _turn = rawr_import::store::test_turn();
    let contract = p2::graph()
        .classify(&Policy::standard(), &rawr_import::cargo::Limits::standard())
        .expect("the real extracted graph classifies");

    let function = p2::outcome(&contract, "facade_a", &["integer_only"]);
    assert!(matches!(
        function.representability(),
        Representability::Function(_)
    ));
    assert_eq!(
        contract
            .select(function.identity(), function.identity().selector().clone())
            .expect("the exact function selector is valid")
            .as_ref(),
        Some(function.identity())
    );

    let record = p2::outcome(&contract, "facade_a", &["OpaqueFields"]);
    assert!(matches!(
        record.representability(),
        Representability::Rejected(_)
    ));
    assert!(contract
        .select(record.identity(), record.identity().selector().clone())
        .expect("rejected selection remains a closed lookup")
        .is_none());

    for outcome in contract.outcomes() {
        for associated in outcome
            .implementations()
            .flat_map(rawr_import::classify::ImplementationOutcome::associated)
            .filter(|associated| associated.representability().is_demandable())
        {
            assert_eq!(
                contract
                    .select(
                        associated.identity(),
                        associated.identity().selector().clone(),
                    )
                    .expect("an admitted associated selector is valid")
                    .as_ref(),
                Some(associated.identity())
            );
        }
    }
}
