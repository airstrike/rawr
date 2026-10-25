use rawr::{Engine, EvalAltResult, INT, ParseErrorType, Position, Scope};

#[test]
fn test_var_scope() {
    let engine = Engine::new();
    let mut scope = Scope::new();

    engine.run_with_scope(&mut scope, "let mut x = 4 + 5;").unwrap();
    assert_eq!(engine.eval_with_scope::<INT>(&mut scope, "x").unwrap(), 9);
    engine.run_with_scope(&mut scope, "x += 1; x += 2;").unwrap();
    assert_eq!(engine.eval_with_scope::<INT>(&mut scope, "x").unwrap(), 12);

    scope.set_value("x", 42 as INT);
    assert_eq!(engine.eval_with_scope::<INT>(&mut scope, "x").unwrap(), 42);

    engine.run_with_scope(&mut scope, "{ let x = 3; }").unwrap();
    assert_eq!(engine.eval_with_scope::<INT>(&mut scope, "x").unwrap(), 42);

    scope.clear();
    engine.run_with_scope(&mut scope, "let x = 3; let x = 42; let x = 123;").unwrap();
    assert_eq!(scope.len(), 1);
    assert_eq!(scope.get_value::<INT>("x").unwrap(), 123);

    scope.clear();
    engine.run_with_scope(&mut scope, "let x = 3; let y = 0; let x = 42; let y = 999; let x = 123;").unwrap();
    assert_eq!(scope.len(), 2);
    assert_eq!(scope.get_value::<INT>("x").unwrap(), 123);
    assert_eq!(scope.get_value::<INT>("y").unwrap(), 999);

    // A `let` binding stays immutable in the scope, and `let mut` stays mutable.
    // A `const` item is not a variable, so it does not land in the scope.
    scope.clear();
    engine.run_with_scope(&mut scope, "const X: i64 = 3; let y = 0; let mut z = 42; z += 1;").unwrap();
    assert_eq!(scope.len(), 2);
    assert!(!scope.contains("X"));
    assert_eq!(scope.get_value::<INT>("y").unwrap(), 0);
    assert_eq!(scope.get_value::<INT>("z").unwrap(), 43);
    assert!(scope.is_constant("y").unwrap());
    assert!(!scope.is_constant("z").unwrap());

    scope.clear();
    engine.run_with_scope(&mut scope, "let x = 3; let y = 0; { let x = 42; let y = 999; } let x = 123;").unwrap();

    assert_eq!(scope.len(), 2);
    assert_eq!(scope.get_value::<INT>("x").unwrap(), 123);
    assert_eq!(scope.get_value::<INT>("y").unwrap(), 0);

    assert_eq!(
        engine
            .eval::<INT>(
                "
                        let mut sum = 0;
                        for x in 0..10 {
                            let x = 42;
                            sum += x;
                        }
                        sum
                    ",
            )
            .unwrap(),
        420
    );

    scope.clear();

    scope.push("x", 42 as INT);
    scope.push_constant("x", 42 as INT);

    let scope2 = scope.clone();
    let scope3 = scope.clone_visible();

    assert_eq!(scope2.is_constant("x"), Some(true));
    assert_eq!(scope3.is_constant("x"), Some(true));
}

#[test]
fn test_var_scope_max() {
    let mut engine = Engine::new();
    let mut scope = Scope::new();

    engine.set_max_variables(5);

    engine
        .run_with_scope(
            &mut scope,
            "
                let a = 0;
                let b = 0;
                let c = 0;
                let d = 0;
                let e = 0;
            ",
        )
        .unwrap();

    scope.clear();

    engine
        .run_with_scope(
            &mut scope,
            "
                let a = 0;
                let b = 0;
                let c = 0;
                let d = 0;
                let e = 0;
                let a = 42;     // reuse variable
            ",
        )
        .unwrap();

    scope.clear();

    engine
        .run_with_scope(
            &mut scope,
            "
                fn foo(n: i64) {
                    if n > 3 { return; }

                    let w = 0;
                    let x = 0;
                    let y = 0;
                    let z = 0;

                    foo(n + 1);
                }

                let a = 0;
                let b = 0;
                let c = 0;
                let d = 0;
                let e = 0;

                foo(0);
            ",
        )
        .unwrap();

    scope.clear();

    engine
        .run_with_scope(
            &mut scope,
            "
                fn foo(a: i64, b: i64, c: i64, d: i64, e: i64) -> i64 {
                    42
                }

                foo(0, 0, 0, 0, 0);
            ",
        )
        .unwrap();

    scope.clear();

    assert!(matches!(
        *engine
            .run_with_scope(
                &mut scope,
                "
                    let a = 0;
                    let b = 0;
                    let c = 0;
                    let d = 0;
                    let e = 0;
                    let f = 0;
                "
            )
            .unwrap_err(),
        EvalAltResult::ErrorTooManyVariables(..)
    ));

    scope.clear();

    assert!(matches!(
        *engine
            .run_with_scope(
                &mut scope,
                "
                    fn foo(n: i64) {
                        if n > 3 { return; }

                        let v = 0;
                        let w = 0;
                        let x = 0;
                        let y = 0;
                        let z = 0;

                        foo(n + 1);
                    }
        
                    let a = 0;
                    let b = 0;
                    let c = 0;
                    let d = 0;
                    let e = 0;
                    let f = 0;

                    foo(0);
                "
            )
            .unwrap_err(),
        EvalAltResult::ErrorTooManyVariables(..)
    ));

    scope.clear();

    assert!(matches!(
        *engine
            .run_with_scope(
                &mut scope,
                "
                    fn foo(a: i64, b: i64, c: i64, d: i64, e: i64, f: i64) -> i64 {
                        42
                    }
        
                    foo(0, 0, 0, 0, 0, 0);
                "
            )
            .unwrap_err(),
        EvalAltResult::ErrorTooManyVariables(..)
    ));
}

#[test]
fn test_scope_eval() {
    let engine = Engine::new();

    // First create the state
    let mut scope = Scope::new();

    // Then push some initialized variables into the state
    // NOTE: Remember the default numbers used by Rawr are INT and f64.
    //       Better stick to them or it gets hard to work with other variables in the script.
    scope.push("y", 42 as INT);
    scope.push("z", 999 as INT);

    // First invocation
    engine.run_with_scope(&mut scope, " let x = 4 + 5 - y + z; y = 1;").expect("variables y and z should exist");

    // Second invocation using the same state
    let result = engine.eval_with_scope::<INT>(&mut scope, "x").unwrap();

    println!("result: {result}"); // should print 966

    // Variable y is changed in the script
    assert_eq!(scope.get_value::<INT>("y").expect("variable y should exist"), 1);
}

#[test]
fn test_var_resolver1() {
    let mut engine = Engine::new();

    let mut scope = Scope::new();
    scope.push("innocent", 1 as INT);
    scope.push("chameleon", 123 as INT);
    scope.push("DO_NOT_USE", 999 as INT);

    #[allow(deprecated)] // not deprecated but unstable
    engine.on_var(move |name, _, context| {
        match name {
            // Override a variable - make it not found even if it exists!
            "DO_NOT_USE" => Err(EvalAltResult::ErrorVariableNotFound(name.to_string(), Position::NONE).into()),
            // Silently maps 'chameleon' into 'innocent'.
            "chameleon" => context
                .scope()
                .get_value("innocent")
                .map(Some)
                .ok_or_else(|| EvalAltResult::ErrorVariableNotFound(name.to_string(), Position::NONE).into()),
            // Return Ok(None) to continue with the normal variable resolution process.
            _ => Ok(None),
        }
    });

    assert_eq!(engine.eval_with_scope::<INT>(&mut scope, "chameleon").unwrap(), 1);
    assert!(matches!(
        *engine.eval_with_scope::<INT>(&mut scope, "DO_NOT_USE").unwrap_err(),
        EvalAltResult::ErrorVariableNotFound(n, ..) if n == "DO_NOT_USE"));
}

#[test]
fn test_var_def_filter() {
    let mut engine = Engine::new();

    let ast = engine.compile("let x = 42;").unwrap();
    engine.run_ast(&ast).unwrap();

    #[allow(deprecated)] // not deprecated but unstable
    engine.on_def_var(|_, info, _| match (info.name(), info.nesting_level()) {
        ("x", 0 | 1) => Ok(false),
        _ => Ok(true),
    });

    assert_eq!(engine.eval::<INT>("let y = 42; let y = 123; let z = y + 1; z").unwrap(), 124);
    assert!(matches!(engine.compile("let x = 42;").unwrap_err().err_type(), ParseErrorType::ForbiddenVariable(s) if s == "x"));
    assert!(matches!(*engine.run_ast(&ast).expect_err("should err"), EvalAltResult::ErrorForbiddenVariable(s, _) if s == "x"));
    let _ = engine.run("let y = 42; { let x = y + 1; }").unwrap_err();
    let _ = engine.run("let y = 42; { let x = y + 1; }").unwrap_err();
    engine.run("let y = 42; { let z = y + 1; { let x = z + 1; } }").unwrap();
}

#[test]
fn test_var_scope_cloning() {
    struct Foo {
        field: INT,
    }

    impl Clone for Foo {
        fn clone(&self) -> Self {
            panic!("forbidden to clone!");
        }
    }

    let mut engine = Engine::new();
    engine.register_fn("field", |foo: &mut Foo| foo.field);

    let mut scope = Scope::new();
    scope.push("foo", Foo { field: 1 });

    // Reading a scope value through a borrow must not clone it.
    engine.run_with_scope(&mut scope, r#"let x = 42; println!("{}", x + field(&foo));"#).unwrap();
    assert_eq!(engine.eval_with_scope::<INT>(&mut scope, "let x = 42; x + field(&foo)").unwrap(), 43);
}
