use rawr::{Engine, INT, Scope};

#[test]
fn test_options_allow() {
    let mut engine = Engine::new();

    engine.compile("let y = true; let x = if y { 1 } else { 2 };").unwrap();

    engine.set_allow_if_expression(false);

    let _ = engine.compile("let y = true; let x = if y { 1 } else { 2 };").unwrap_err();

    engine.compile("let x = { let z = 0; z + 1 };").unwrap();

    engine.set_allow_statement_expression(false);

    let _ = engine.compile("let x = { let z = 0; z + 1 };").unwrap_err();

    #[cfg(not(feature = "no_function"))]
    {
        engine.compile("let x = || 42;").unwrap();

        engine.set_allow_anonymous_fn(false);

        let _ = engine.compile("let x = || 42;").unwrap_err();
    }

    let ast = engine.compile("let mut x = 0; while x < 10 { x += 1; }").unwrap();

    engine.set_allow_looping(false);

    engine.run_ast(&ast).unwrap();

    let _ = engine.compile("let mut x = 0; while x < 10 { x += 1; }").unwrap_err();

    engine.compile("let x = 42; let x = 123;").unwrap();

    engine.set_allow_shadowing(false);

    let _ = engine.compile("let x = 42; let x = 123;").unwrap_err();

    let mut scope = Scope::new();
    scope.push("x", 42 as INT);

    let _ = engine.run_with_scope(&mut scope, "let x = 42;").unwrap_err();
}

#[test]
fn test_options_strict_var() {
    let mut engine = Engine::new();

    // Every variable must be declared, with or without strict variables.
    let _ = engine.compile("let x = if y { z } else { w };").unwrap_err();

    let mut scope = Scope::new();
    scope.push("x", 42 as INT);
    scope.push_constant("y", 0 as INT);

    engine.set_strict_variables(true);

    let _ = engine.compile("let x = if y { z } else { w };").unwrap_err();

    engine.compile("let y = 42; let x = y;").unwrap();

    assert_eq!(engine.eval_with_scope::<INT>(&mut scope, "{ let y = 42; x * y }").unwrap(), 42 * 42);

    #[cfg(not(feature = "no_function"))]
    {
        // A function doesn't see the scope's variables.
        let _ = engine.compile_with_scope(&scope, "fn foo(x: i64) -> i64 { x + y }").unwrap_err();

        assert_eq!(engine.eval_with_scope::<INT>(&mut scope, "fn foo(z: i64) -> i64 { z } let f = foo; f(x)").unwrap(), 42);
        let _ = engine.compile("let f = |y: i64| x * y;").unwrap_err();
        #[cfg(not(feature = "no_closure"))]
        {
            engine.compile("let x = 42; let f = |y: i64| x * y;").unwrap();
            engine.compile("let x = 42; let f = |y: i64| move || x + y;").unwrap();
            let _ = engine.compile("fn foo() -> i64 { let f = |y: i64| x + y; f(1) }").unwrap_err();
        }
    }
}
