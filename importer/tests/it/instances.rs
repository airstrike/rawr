use rawr_import::cargo::Limits;
use rawr_import::classify::representability::convert::{Builtin, Type};
use rawr_import::surface::callable::{once, shared, Class, Error as CallableError};
use rawr_import::surface::instance::{Arguments, Binder};

#[test]
fn instance_rejects_an_unconstrained_type() {
    let result = Arguments::new(
        Box::from([Type::Parameter(0)]),
        Box::default(),
        Box::default(),
        None,
        &Limits::standard(),
    );
    assert!(matches!(
        result,
        Err(rawr_import::surface::Error::UnconstrainedGeneric)
    ));
}

#[test]
fn concrete_instance_accepts_owned_builtin_types() {
    let result = Arguments::new(
        Box::from([Type::Builtin(Builtin::I64)]),
        Box::default(),
        Box::default(),
        None,
        &Limits::standard(),
    );
    assert!(result.is_ok());
}

#[test]
fn once_callable_reports_consumption() {
    let mut function = once::Ready::new(|value: i64| value + 1);
    assert_eq!(function.call(1), Ok(2));
    assert_eq!(function.call(1), Err(CallableError::Consumed));
}

#[test]
fn callable_arguments_seal_all_four_closed_ownership_classes() {
    let limits = Limits::standard();
    let binder = Binder::normalized(0, 0);
    let borrowed = Arguments::borrowed(
        Box::from([Type::Builtin(Builtin::I64)]),
        Box::default(),
        Box::from([binder]),
        &limits,
    )
    .expect("the input-only borrowed callable seals");
    let shared = Arguments::shared(
        Box::from([Type::Builtin(Builtin::I64)]),
        Box::default(),
        Box::default(),
        shared::Scope::Epoch,
        &limits,
    )
    .expect("the shared callable seals");
    let mutable = Arguments::mutable(
        Box::from([Type::Builtin(Builtin::I64)]),
        Box::default(),
        Box::default(),
        &limits,
    )
    .expect("the mutable callable seals");
    let once = Arguments::once(
        Box::from([Type::Builtin(Builtin::I64)]),
        Box::default(),
        Box::default(),
        &limits,
    )
    .expect("the one-shot callable seals");

    assert!(matches!(
        borrowed.callable_class(),
        Some(Class::Borrowed(_))
    ));
    assert!(matches!(shared.callable_class(), Some(Class::Shared(_))));
    assert!(matches!(mutable.callable_class(), Some(Class::Mutable(_))));
    assert!(matches!(once.callable_class(), Some(Class::Once(_))));
    assert!(Arguments::shared(
        Box::default(),
        Box::default(),
        Box::default(),
        shared::Scope::Region,
        &limits,
    )
    .is_ok());
}
