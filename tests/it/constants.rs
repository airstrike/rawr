use rawr::{Engine, EvalAltResult, INT, ParseErrorType, Scope};

#[test]
fn test_constant() {
    let engine = Engine::new();

    assert_eq!(engine.eval::<INT>("const X: i64 = 123; X").unwrap(), 123);

    // Rust refuses to assign to a constant, so the script doesn't compile.
    let _ = engine.compile("const X: i64 = 123; X = 42;").expect_err("expects error");

    #[cfg(not(feature = "no_index"))]
    let _ = engine.compile("const X: [i64; 5] = [1, 2, 3, 4, 5]; X[2] = 42;").expect_err("expects error");
}

#[test]
fn test_constant_scope() {
    let engine = Engine::new();

    let mut scope = Scope::new();
    scope.push_constant("x", 42 as INT);

    assert!(matches!(
        *engine.run_with_scope(&mut scope, "x = 1").expect_err("expects error"),
        EvalAltResult::ErrorParsing(ParseErrorType::Resolution(rawr::resolve::Error::ImmutableBinding { binding, .. }), ..) if binding == "x"
    ));

    // A value pushed with `push` is a mutable local.
    scope.push("y", 42 as INT);
    assert_eq!(engine.eval_with_scope::<INT>(&mut scope, "y = 1; y").unwrap(), 1);
    assert_eq!(scope.get_value::<INT>("y").unwrap(), 1);
}
