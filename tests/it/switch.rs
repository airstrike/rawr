#[cfg(feature = "internals")]
use rawr::{ASTNode, Expr};
use rawr::{Engine, INT, ParseErrorType, Scope};

#[test]
fn test_match_literals() {
    let engine = Engine::new();
    let mut scope = Scope::new();
    scope.push("x", 42 as INT);

    assert_eq!(engine.eval::<char>("match 2 { 1 => 'z', 2 => 'a', _ => 'q' }").unwrap(), 'a');
    engine.run("match 3 { 1 => (), 2 => (), _ => () }").unwrap();
    assert_eq!(engine.eval::<INT>("match 3 { 1 => 1, 2 => 2, 42 => 3, _ => 123 }").unwrap(), 123);
    assert_eq!(engine.eval_with_scope::<INT>(&mut scope, "match 2 { 1 => 1, 2 if x < 40 => 2, 42 => 3, _ => 123 }").unwrap(), 123);
    assert_eq!(engine.eval_with_scope::<INT>(&mut scope, "match 2 { 1 => 1, 2 if x > 40 => 2, 42 => 3, _ => 123 }").unwrap(), 2);
    assert!(engine.eval_with_scope::<bool>(&mut scope, "match x { 1 => false, 2 => false, 42 => true, _ => false }").unwrap());
    assert!(engine.eval_with_scope::<bool>(&mut scope, "match x { 1 => false, 2 => false, _ => true }").unwrap());

    assert_eq!(engine.eval_with_scope::<INT>(&mut scope, "match x { 1 | 2 | 3 | 5..50 => 123, _ => 0 }").unwrap(), 123);
    assert_eq!(engine.eval_with_scope::<INT>(&mut scope, "match x { 424242 => 123, _ => 42 }").unwrap(), 42);
    assert_eq!(engine.eval_with_scope::<INT>(&mut scope, "match x { 1 => 123, 42 => { x / 2 }, _ => 999 }").unwrap(), 21);
    assert_eq!(engine.eval_with_scope::<INT>(&mut scope, "match x { 1 => 123, other => other + 100 }").unwrap(), 142);
    #[cfg(not(feature = "no_index"))]
    assert_eq!(
        engine
            .eval_with_scope::<INT>(
                &mut scope,
                "
                    let y = [1, 2, 3];

                    match y {
                        [4, 5, 6] => 1,
                        [1, 2, 3] => 3,
                        _ => 9
                    }
                "
            )
            .unwrap(),
        3
    );
    assert_eq!(engine.eval::<INT>("match 'x' { 'a'..='m' => 1, 'x' | 'y' => 2, _ => 3 }").unwrap(), 2);
    assert_eq!(engine.eval::<INT>(r#"match "hello" { "world" => 1, "hello" => 2, _ => 3 }"#).unwrap(), 2);
    assert_eq!(engine.eval::<INT>("match true { true => 1, false => 2 }").unwrap(), 1);

    // The first matching arm wins.
    assert_eq!(engine.eval_with_scope::<INT>(&mut scope, "match 42 { 42 => 123, 42 => 999, _ => 0 }").unwrap(), 123);

    assert_eq!(engine.eval_with_scope::<INT>(&mut scope, "match x { 42 => 123, 42 => 999, _ => 0 }").unwrap(), 123);
}

#[test]
fn test_match_errors() {
    let engine = Engine::new();

    // As in Rust, a match on a number must cover every value, and a guarded
    // catch-all arm doesn't count.
    for script in ["let x = 1; match x { 1 => 123 }", "let x = 1; match x { 1 => 123, _ if true => 42 }"] {
        assert!(matches!(
            engine.compile(script).unwrap_err().err_type(),
            ParseErrorType::Resolution(rawr::resolve::Error::InvalidMatch { reason, .. }) if reason.contains("non-exhaustive")
        ));
    }
}

#[test]
fn test_match_condition() {
    let engine = Engine::new();
    let mut scope = Scope::new();
    scope.push("x", 42 as INT);

    assert_eq!(
        engine
            .eval_with_scope::<INT>(
                &mut scope,
                "
                    match x / 2 {
                        21 if x > 40 => 1,
                        0 if x < 100 => 2,
                        1 => 3,
                        _ => 9
                    }
                "
            )
            .unwrap(),
        1
    );

    assert_eq!(
        engine
            .eval_with_scope::<INT>(
                &mut scope,
                "
                    match x / 2 {
                        21 if x < 40 => 1,
                        0 if x < 100 => 2,
                        1 => 3,
                        _ => 9
                    }
                "
            )
            .unwrap(),
        9
    );

    assert_eq!(
        engine
            .eval_with_scope::<INT>(
                &mut scope,
                "
                    match x {
                        42 if x < 40 => 1,
                        42 if x > 40 => 7,
                        0 if x < 100 => 2,
                        1 => 3,
                        42 if x == 10 => 10,
                        _ => 9
                    }
                "
            )
            .unwrap(),
        7
    );
}

#[test]
fn test_match_ranges() {
    let engine = Engine::new();
    let mut scope = Scope::new();
    scope.push("x", 42 as INT);

    assert_eq!(
        engine
            .eval_with_scope::<char>(&mut scope, "match x { 10..20 => 'q', 20..=42 => 'a', 25..45 => 'z', 30..100 => 'w', _ => '-' }")
            .unwrap(),
        'a'
    );
    assert_eq!(
        engine
            .eval_with_scope::<char>(&mut scope, "match x { 10..20 => 'q', 20..=42 if x < 40 => 'a', 25..45 => 'z', 30..100 => 'w', _ => '-' }")
            .unwrap(),
        'z'
    );
    assert_eq!(
        engine
            .eval_with_scope::<char>(&mut scope, "match x { 42 => 'x', 10..20 => 'q', 20..=42 => 'a', 25..45 => 'z', 30..100 => 'w', _ => '-' }")
            .unwrap(),
        'x'
    );
    // A literal after a range that covers it is never reached.
    assert_eq!(
        engine
            .eval_with_scope::<char>(&mut scope, "match x { 10..20 => 'q', 20..=42 => 'a', 25..45 => 'z', 42 => 'x', 30..100 => 'w', _ => '-' }")
            .unwrap(),
        'a'
    );
    #[cfg(not(feature = "no_float"))]
    assert!(matches!(
        engine.compile_with_scope(&scope, "match x { 10..20 => 'q', 42.0 => 'x', _ => '-' }").unwrap_err().err_type(),
        ParseErrorType::Resolution(rawr::resolve::Error::TypeMismatch { .. })
    ));
    assert_eq!(
        engine
            .eval_with_scope::<char>(
                &mut scope,
                "
                    match 5 {
                        0..10 if x + 2 == 1 + 2 => 'a',
                        _ => 'x'
                    }
                "
            )
            .unwrap(),
        'x'
    );
    assert_eq!(
        engine
            .eval_with_scope::<INT>(
                &mut scope,
                "
                    match 5 {
                        0..10 => 123,
                        2..12 => 7,
                        _ => 0
                    }
                "
            )
            .unwrap(),
        123
    );
    assert_eq!(
        engine
            .eval_with_scope::<INT>(
                &mut scope,
                "
                    match 5 {
                        4 | 5 | 6 => 42,
                        0..10 => 123,
                        2..12 => 7,
                        _ => 0
                    }
                "
            )
            .unwrap(),
        42
    );
    assert_eq!(
        engine
            .eval_with_scope::<char>(
                &mut scope,
                "
                    match 5 {
                        2..12 => 'z',
                        0..10 if x + 2 == 1 + 2 => 'a',
                        _ => 'x'
                    }
                "
            )
            .unwrap(),
        'z'
    );
    assert_eq!(
        engine
            .eval_with_scope::<char>(
                &mut scope,
                "
                    match 5 {
                        0..10 if x + 2 == 1 + 2 => 'a',
                        2..12 => 'z',
                        _ => 'x'
                    }
                "
            )
            .unwrap(),
        'z'
    );
}

/// AST walk tests check that `walk` visits the body of every `match` arm.

#[test]
#[cfg(feature = "internals")]
fn test_match_walk_visits_default_case_body() {
    let mut engine = Engine::new();
    // Without optimization the variables stay in the tree.
    engine.set_optimization_level(rawr::OptimizationLevel::None);
    // `extra` is inside the catch-all arm's body and must be visited.
    let ast = engine
        .compile(
            r#"
                fn foo() -> i64 { 1 }
                fn bar(value: i64) -> i64 { value }
                let action = 1;
                let extra = 2;
                match action {
                    1 => foo(),
                    _ => bar(extra)
                }
            "#,
        )
        .unwrap();

    let mut vars: Vec<String> = Vec::new();
    ast.walk(&mut |nodes: &[ASTNode]| {
        if let Some(ASTNode::Expr(Expr::Variable(info, _, _))) = nodes.last() {
            vars.push(info.1.to_string());
        }
        true
    });

    assert!(vars.contains(&"extra".to_string()), "walk should visit `extra` in the catch-all arm's body");
}

#[test]
#[cfg(feature = "internals")]
fn test_match_walk_visits_all_case_bodies_and_default() {
    let mut engine = Engine::new();
    // Without optimization the variables stay in the tree.
    engine.set_optimization_level(rawr::OptimizationLevel::None);
    let ast = engine
        .compile(
            r#"
                fn arm(value: i64) -> i64 { value }
                let (x, a, b, c) = (1, 2, 3, 4);
                match x {
                    1 => arm(a),
                    2 => arm(b),
                    _ => arm(c)
                }
            "#,
        )
        .unwrap();

    let mut vars: Vec<String> = Vec::new();
    ast.walk(&mut |nodes: &[ASTNode]| {
        if let Some(ASTNode::Expr(Expr::Variable(info, _, _))) = nodes.last() {
            vars.push(info.1.to_string());
        }
        true
    });

    for name in &["x", "a", "b", "c"] {
        assert!(vars.contains(&name.to_string()), "walk should visit `{name}`", name = name);
    }
}

#[test]
#[cfg(feature = "internals")]
fn test_match_walk_default_only() {
    let mut engine = Engine::new();
    // Without optimization the variables stay in the tree.
    engine.set_optimization_level(rawr::OptimizationLevel::None);
    // A match with only a catch-all arm: the body must still be visited.
    let ast = engine.compile("fn fallback(value: i64) -> i64 { value } let x = 1; let y = 2; match x { _ => fallback(y) }").unwrap();

    let mut vars: Vec<String> = Vec::new();
    ast.walk(&mut |nodes: &[ASTNode]| {
        if let Some(ASTNode::Expr(Expr::Variable(info, _, _))) = nodes.last() {
            vars.push(info.1.to_string());
        }
        true
    });

    assert!(vars.contains(&"y".to_string()), "walk should visit `y` inside the catch-all arm's body");
}
