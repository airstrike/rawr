use crate::p4;

use rawr_import::classify::representability::Representability;
use rawr_import::surface::callable::{once, Class, Error};

#[test]
fn real_fn_once_uses_private_consumed_state() {
    let _turn = rawr_import::store::test_turn();
    let (_project, contract, _limits) = p4::contract();
    let outcome = contract
        .outcomes()
        .find(|outcome| outcome.export().path().eq(["once"]))
        .expect("the FnOnce fixture is exported");
    let Representability::FunctionItem(item) = outcome.representability() else {
        panic!("the FnOnce fixture must retain a function-item plan")
    };
    assert!(matches!(item.class(), Some(Class::Once(_))));
    let mut ready = once::Ready::new(|value: i64| value + 1);
    assert_eq!(ready.call(1), Ok(2));
    assert_eq!(ready.call(1), Err(Error::Consumed));
}
