use rawr::{Engine, FuncRegistration, INT, OptimizationLevel, Scope};

#[test]
fn test_optimizer() {
    let mut engine = Engine::new();
    engine.set_optimization_level(OptimizationLevel::Simple);

    assert_eq!(
        engine
            .eval::<INT>(
                "
                    const X: i64 = 40 + 2 - 1 + 1;
                    X
                "
            )
            .unwrap(),
        42
    );
}

#[test]
fn test_optimizer_run() {
    fn run_test(engine: &mut Engine) {
        assert_eq!(engine.eval::<INT>("if true { 42 } else { 123 }").unwrap(), 42);
        assert_eq!(engine.eval::<INT>("if 1 == 1 || 2 > 3 { 42 } else { 123 }").unwrap(), 42);
        assert_eq!(engine.eval::<INT>(r#"const ABC: &str = "hello"; if ABC < "foo" { 42 } else { 123 }"#).unwrap(), 123);
    }

    let mut engine = Engine::new();

    engine.set_optimization_level(OptimizationLevel::None);
    run_test(&mut engine);

    engine.set_optimization_level(OptimizationLevel::Simple);
    run_test(&mut engine);

    engine.set_optimization_level(OptimizationLevel::Full);
    run_test(&mut engine);

    // Override == operator
    engine.register_fn("==", |_x: INT, _y: INT| false);

    engine.set_optimization_level(OptimizationLevel::Simple);

    assert_eq!(engine.eval::<INT>("if 1 == 1 || 2 > 3 { 42 } else { 123 }").unwrap(), 42);

    engine.set_fast_operators(false);

    assert_eq!(engine.eval::<INT>("if 1 == 1 || 2 > 3 { 42 } else { 123 }").unwrap(), 123);

    engine.set_optimization_level(OptimizationLevel::Full);

    assert_eq!(engine.eval::<INT>("if 1 == 1 || 2 > 3 { 42 } else { 123 }").unwrap(), 123);
}

#[cfg(feature = "metadata")]
#[test]
fn test_optimizer_parse() {
    let mut engine = Engine::new();

    engine.set_optimization_level(OptimizationLevel::Simple);

    // The branch on a constant folds to the value of the branch taken.
    let ast = engine.compile("{ const DECISION: bool = false; if DECISION { 42 } else { 123 } }").unwrap();
    let text = format!("{ast:?}");

    assert!(text.contains("Expr(123"), "{text}");
    assert!(!text.contains("Expr(42"), "{text}");

    // A branch that can never run is removed.
    let ast = engine.compile(r#"if 1 == 2 { println!("never"); }"#).unwrap();
    let text = format!("{ast:?}");

    assert!(!text.contains("If("), "{text}");
    assert!(!text.contains("never"), "{text}");

    engine.set_optimization_level(OptimizationLevel::Full);

    let ast = engine.compile("(-42_i64).abs()").unwrap();

    assert_eq!(format!("{ast:?}"), r#"AST { source: None, doc: "", resolver: None, body: [Expr(42 @ 1:1)] }"#);
}

#[test]
fn test_optimizer_scope() {
    const SCRIPT: &str = "FOO";

    let engine = Engine::new();
    let mut scope = Scope::new();

    scope.push_constant("FOO", 42 as INT);

    // The constant's value is compiled into the script.
    let ast = engine.compile_with_scope(&scope, SCRIPT).unwrap();

    scope.push("FOO", 123 as INT);

    assert_eq!(engine.eval_ast::<INT>(&ast).unwrap(), 42);
    assert_eq!(engine.eval_ast_with_scope::<INT>(&mut scope, &ast).unwrap(), 42);

    // A variable is read when the script runs.
    let ast = engine.compile_with_scope(&scope, SCRIPT).unwrap();

    assert_eq!(engine.eval_ast_with_scope::<INT>(&mut scope, &ast).unwrap(), 123);
}

#[test]
fn test_optimizer_re_optimize() {
    let engine = Engine::new();
    let ast = engine
        .compile(
            "
                const FOO: i64 = 42;
                fn foo() -> i64 {
                    let f = || FOO * 2;
                    f()
                }
                foo()
            ",
        )
        .unwrap();
    let scope: Scope = ast.iter_literal_variables(true, false).collect();
    let ast = engine.optimize_ast(&scope, ast, OptimizationLevel::Simple);

    assert_eq!(engine.eval_ast::<INT>(&ast).unwrap(), 84);
}

#[test]
fn test_optimizer_full() {
    #[derive(Debug, Clone)]
    struct TestStruct(INT);

    let mut engine = Engine::new();
    let mut scope = Scope::new();

    engine.set_optimization_level(OptimizationLevel::Full);

    assert_eq!(
        engine
            .eval::<INT>(
                r#"
                    fn foo(x: i64) { println!("{x}"); return; }
                    fn foo2(x: i64) { if x > 0 {} return; }
                    42
                "#
            )
            .unwrap(),
        42
    );

    engine
        .register_type_with_name::<TestStruct>("TestStruct")
        .register_fn("ts", |n: INT| TestStruct(n))
        .register_fn("value", |ts: &mut TestStruct| ts.0);

    let ast = engine
        .compile(
            "
                let foo = ts(40 + 2);
                value(foo)
            ",
        )
        .unwrap();

    #[cfg(feature = "internals")]
    assert_eq!(ast.statements().len(), 2);

    assert_eq!(engine.eval_ast_with_scope::<INT>(&mut scope, &ast).unwrap(), 42);

    assert_eq!(scope.len(), 1);

    assert_eq!(scope.get_value::<TestStruct>("foo").unwrap().0, 42);
}

#[test]
fn test_optimizer_volatile() {
    let mut engine = Engine::new();

    engine.set_optimization_level(OptimizationLevel::Full);

    FuncRegistration::new("foo").with_volatility(true).register_into_engine(&mut engine, |x: INT| x + 1);

    let ast = engine.compile("foo(42)").unwrap();

    let text_ast = format!("{ast:?}");

    // Make sure the call is not optimized away
    assert!(text_ast.contains(r#"name: "foo""#));

    FuncRegistration::new("foo").with_volatility(false).register_into_engine(&mut engine, |x: INT| x + 1);

    let ast = engine.compile("foo(42)").unwrap();

    let text_ast = format!("{ast:?}");
    println!("{text_ast:#?}");

    // Make sure the call is optimized away
    assert!(!text_ast.contains(r#"name: "foo""#));
}
