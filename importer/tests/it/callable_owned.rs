use crate::p4;

use rawr_import::classify::representability::Representability;
use rawr_import::surface::callable::{shared, Class};

#[test]
fn real_static_fn_and_fn_mut_use_closed_epoch_owned_classes() {
    let _turn = rawr_import::store::test_turn();
    let (_project, contract, _limits) = p4::contract();
    let class = |name| {
        let outcome = contract
            .outcomes()
            .find(|outcome| outcome.export().path().eq([name]))
            .expect("the callable fixture is exported");
        let Representability::FunctionItem(item) = outcome.representability() else {
            panic!("the callable fixture must retain a function-item plan")
        };
        item.class().expect("the callable class is explicit")
    };
    assert!(matches!(
        class("shared"),
        Class::Shared(proof) if proof.scope() == shared::Scope::Epoch
    ));
    assert!(matches!(class("mutable"), Class::Mutable(_)));
}
