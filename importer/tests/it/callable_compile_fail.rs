use crate::p4;

use rawr_import::classify::representability::Representability;
use rawr_import::surface::callable::Class;

#[test]
fn input_borrowing_hrtb_and_nonstatic_mutation_are_admitted() {
    let _turn = rawr_import::store::test_turn();
    let (_project, contract, _limits) = p4::contract();
    let outcome = |name| {
        contract
            .outcomes()
            .find(|outcome| outcome.export().path().eq([name]))
            .expect("the callable fixture remains positioned")
    };
    // `for<'a> Fn(&'a i64) -> &'a i64` returns a borrow of its own input,
    // which is valid Rust, so it is admitted as rustc admits it (D664,
    // D683.3); the early rejection of borrow returns is superseded.
    assert!(matches!(
        outcome("escaping").representability(),
        Representability::FunctionItem(_)
    ));
    // An `FnMut` with no `'static` bound is borrowed for the call, as an
    // `Fn` one is (#715).
    let Representability::FunctionItem(item) = outcome("nonstatic_mutable").representability()
    else {
        panic!("a non-'static FnMut keeps its function-item plan")
    };
    assert!(matches!(item.class(), Some(Class::Borrowed(_))));
}
