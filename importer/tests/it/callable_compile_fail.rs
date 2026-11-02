use crate::p4;

use rawr_import::classify::representability::{reject, Representability};
use rawr_import::surface::callable::Class;

#[test]
fn escaping_hrtb_rejects_and_nonstatic_mutation_is_borrowed() {
    let _turn = rawr_import::store::test_turn();
    let (_project, contract, _limits) = p4::contract();
    let outcome = |name| {
        contract
            .outcomes()
            .find(|outcome| outcome.export().path().eq([name]))
            .expect("the callable fixture remains positioned")
    };
    let Representability::Rejected(reason) = outcome("escaping").representability() else {
        panic!("the unsupported callable protocol must reject")
    };
    assert_eq!(reason.kind(), reject::Kind::EscapingBorrow);
    // An `FnMut` with no `'static` bound is borrowed for the call, as an
    // `Fn` one is (#715).
    let Representability::FunctionItem(item) = outcome("nonstatic_mutable").representability()
    else {
        panic!("a non-'static FnMut keeps its function-item plan")
    };
    assert!(matches!(item.class(), Some(Class::Borrowed(_))));
}
