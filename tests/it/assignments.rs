use rawr::{Engine, INT, ParseErrorType, resolve::Error};

#[test]
fn test_assignments() {
    let engine = Engine::new();

    assert_eq!(engine.eval::<INT>("let mut x = 42; x = 123; x").unwrap(), 123);
    assert_eq!(engine.eval::<INT>("let mut x = 42; x += 123; x").unwrap(), 165);

    #[cfg(not(feature = "no_index"))]
    assert_eq!(engine.eval::<INT>("let mut x = vec![42]; x[0] += 123; x[0]").unwrap(), 165);

    #[cfg(not(feature = "no_object"))]
    assert_eq!(engine.eval::<INT>("struct Point { a: i64 } let mut x = Point { a: 42 }; x.a += 123; x.a").unwrap(), 165);
}

#[test]
fn test_assignments_bad_lhs() {
    let engine = Engine::new();

    let bad_target = |script: &str| {
        let err = engine.compile(script).expect_err(script);
        matches!(err.err_type(), ParseErrorType::Resolution(Error::UnsupportedNominalSyntax { construct, .. }) if construct == "assignment target")
    };

    assert!(bad_target("let x = 1; let y = 2; (x + y) = 42;"));
    assert!(bad_target("fn foo(x: i64) -> i64 { x } let x = 1; foo(x) = 42;"));
    assert!(bad_target("true = 42;"));
    assert!(bad_target("123 = 42;"));

    #[cfg(not(feature = "no_object"))]
    assert!(bad_target("struct S { y: i64 } impl S { fn foo(&self) -> i64 { self.y } } let x = S { y: 1 }; x.foo() = 42;"));

    // Assigning to a binding that isn't `mut` is an error too.
    assert!(matches!(
        engine.compile("let x = 1; x = 2;").unwrap_err().err_type(),
        ParseErrorType::Resolution(Error::ImmutableBinding { binding, .. }) if binding == "x"
    ));
}
