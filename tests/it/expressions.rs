use rawr::{Engine, INT, Scope};

#[test]
fn test_expressions() {
    let engine = Engine::new();
    let mut scope = Scope::new();

    scope.push("x", 10 as INT);

    assert_eq!(engine.eval_expression::<INT>("2 + (10 + 10) * 2").unwrap(), 42);
    assert_eq!(engine.eval_expression_with_scope::<INT>(&mut scope, "2 + (x + 10) * 2").unwrap(), 42);
    assert_eq!(engine.eval_expression_with_scope::<INT>(&mut scope, "if x > 0 { 42 } else { 123 }").unwrap(), 42);
    #[cfg(not(feature = "no_index"))]
    #[cfg(not(feature = "no_function"))]
    {
        assert_eq!(engine.eval_expression_with_scope::<INT>(&mut scope, "[1, 2, 3, 4].iter().map(|x| x * x).fold(0, |a, v| a + v)").unwrap(), 30);
        // A block with statements is still one Rust expression.
        assert_eq!(
            engine
                .eval_expression_with_scope::<INT>(&mut scope, "[1, 2, 3, 4].iter().map(|x| { let r = 2; x * r }).fold(0, |a, v| a + v)")
                .unwrap(),
            20
        );
    }
    assert_eq!(engine.eval_expression_with_scope::<INT>(&mut scope, "if x > 0 { let y = 42; y } else { 123 }").unwrap(), 42);
    assert_eq!(engine.eval_expression_with_scope::<INT>(&mut scope, "if x > 0 { 42 } else { let y = 123; y }").unwrap(), 42);
    let _ = engine.eval_expression_with_scope::<INT>(&mut scope, "if x > 0 { 42 } else {}").unwrap_err();

    assert_eq!(
        engine
            .eval_expression_with_scope::<INT>(
                &mut scope,
                "
                    match x {
                        0 => 1,
                        10 => 42,
                        1..10 => 123,
                        _ => 0,
                    }
                "
            )
            .unwrap(),
        42
    );
    assert_eq!(
        engine
            .eval_expression_with_scope::<INT>(
                &mut scope,
                "
                    match x {
                        0 => 1,
                        10 => 42,
                        _ => {
                            let y = 123;
                            y
                        }
                    }
                ",
            )
            .unwrap(),
        42
    );

    // An expression can't end in a semicolon or be a `let` statement.
    let _ = engine.compile_expression("40 + 2;").unwrap_err();
    let _ = engine.compile_expression("let x = 42").unwrap_err();
    let _ = engine.compile_expression("do { break 42; } while true").unwrap_err();
    engine.compile_expression("40 + { 2 }").unwrap();

    engine.compile("40 + { let x = 2; x }").unwrap();
}
