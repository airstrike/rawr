use crate::p4;

use rawr_import::classify::representability::{convert, Representability};
use rawr_import::surface::instance::Arguments;

#[test]
fn equal_instances_dedupe_but_distinct_types_remain_distinct() {
    let _turn = rawr_import::store::test_turn();
    let (_project, contract, limits) = p4::contract();
    let outcome = contract
        .outcomes()
        .find(|outcome| outcome.export().path().eq(["identity"]))
        .expect("the generic function is exported");
    assert!(matches!(
        outcome.representability(),
        Representability::FunctionItem(_)
    ));
    let selector = outcome
        .identity()
        .selector()
        .try_clone()
        .expect("the exact selector duplicates");
    let base = outcome.identity().clone();
    let mut compiler = contract
        .compiler((), p4::snapshot(&limits), &limits)
        .expect("the P4 compiler starts");
    for position in [0, 1] {
        compiler
            .require_instance(
                &base,
                selector.try_clone().expect("the selector duplicates"),
                arguments(convert::Builtin::I64, &limits),
                "application.rs",
                position,
                position + 1,
            )
            .expect("the repeated concrete instance is demanded");
    }
    compiler
        .require_instance(
            &base,
            selector,
            arguments(convert::Builtin::String, &limits),
            "application.rs",
            2,
            3,
        )
        .expect("the distinct concrete instance is demanded");
    let demand = compiler.seal().expect("the concrete demand seals");
    let identities: Vec<_> = demand.entries().map(|entry| entry.identity()).collect();
    assert_eq!(identities.len(), 3);
    assert_eq!(identities[0].as_bytes(), identities[1].as_bytes());
    assert_ne!(identities[1].as_bytes(), identities[2].as_bytes());
    assert_eq!(
        identities[0].types(),
        &[convert::Type::Builtin(convert::Builtin::I64)]
    );
}

fn arguments(builtin: convert::Builtin, limits: &rawr_import::cargo::Limits) -> Arguments {
    Arguments::new(
        Box::from([convert::Type::Builtin(builtin)]),
        Box::default(),
        Box::default(),
        None,
        limits,
    )
    .expect("the concrete type argument is bounded")
}
