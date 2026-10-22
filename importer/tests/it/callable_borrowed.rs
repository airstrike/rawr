use crate::p4;

use rawr_import::classify::representability::convert::{Builtin, Type};
use rawr_import::classify::representability::Representability;
use rawr_import::surface::callable::Class;
use rawr_import::surface::instance::{Arguments, Binder};

#[test]
fn real_input_only_late_bound_fn_is_classified_as_borrowed() {
    let _turn = rawr_import::store::test_turn();
    let (_project, contract, _limits) = p4::contract();
    let outcome = contract
        .outcomes()
        .find(|outcome| outcome.export().path().eq(["borrowed"]))
        .expect("the borrowed callable fixture is exported");
    let Representability::FunctionItem(item) = outcome.representability() else {
        panic!(
            "borrowed must retain a demanded function-item plan: {:?}",
            outcome.representability()
        )
    };
    let Some(Class::Borrowed(proof)) = item.class() else {
        panic!("the nonescaping Fn input must be borrowed")
    };
    assert!(proof.is_late_bound());
    assert_eq!(item.binders().len(), 1);
    assert_eq!(item.binders()[0].depth(), 0);
    assert_eq!(item.binders()[0].index(), 0);
    let mut compiler = contract
        .compiler((), p4::snapshot(&_limits), &_limits)
        .expect("the callable demand compiler starts");
    compiler
        .require_instance(
            outcome.identity(),
            outcome
                .identity()
                .selector()
                .try_clone()
                .expect("the callable selector duplicates"),
            Arguments::borrowed(
                Box::from([Type::Builtin(Builtin::I64)]),
                Box::default(),
                Box::from([Binder::normalized(0, 0)]),
                &_limits,
            )
            .expect("the borrowed instance seals"),
            "application.rawr",
            0,
            1,
        )
        .expect("the exact callable class and binder match the declaration");
    assert_eq!(
        compiler
            .seal()
            .expect("the callable demand seals")
            .entries()
            .len(),
        1
    );
}
