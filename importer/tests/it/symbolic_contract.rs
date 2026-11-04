use crate::p2;

use rawr_import::classify::representability::Representability;

#[test]
fn real_symbolic_selectors_are_exact_and_never_create_authority() {
    let contract = p2::contract();

    let function = p2::outcome(contract, "facade_a", &["integer_only"]);
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

    // A struct with a private field stays, readable through its public
    // field, and nothing can build it (D464.6).
    let opaque = p2::outcome(contract, "facade_a", &["OpaqueFields"]);
    assert!(matches!(
        opaque.representability(),
        Representability::Struct(plan) if plan.constructor().is_none() && !plan.constructible()
    ));
    let record = contract
        .outcomes()
        .find(|outcome| matches!(outcome.representability(), Representability::Rejected(_)))
        .expect("the facade has a rejected item");
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
