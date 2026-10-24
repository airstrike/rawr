use rawr::{Engine, FnPtr, INT};

/// A function item is a value, as in Rust: it can be bound to a local and
/// called through it.
#[test]
#[cfg(not(feature = "no_function"))]
fn test_fn_ptr() {
    let engine = Engine::new();

    assert_eq!(
        engine
            .eval::<INT>(
                "
                    fn foo(x: i64) -> i64 { x + 1 }
                    let f = foo;
                    let mut g = foo;
                    g = f;
                    f(39) + g(1)
                "
            )
            .unwrap(),
        42
    );

    // A local of the same name as a function hides it.
    assert_eq!(engine.eval::<INT>("fn dbl(x: i64) -> i64 { x * 2 } let dbl = 7; dbl").unwrap(), 7);
}

#[test]
#[cfg(not(feature = "no_closure"))]
fn test_fn_ptr_make_closure() {
    let f = {
        let engine = Engine::new();

        let ast = engine
            .compile(
                r#"
                    let test = String::from("hello");
                    move |x: i64| format!("{test}{x}")      // this creates a closure
                "#,
            )
            .unwrap();

        let fn_ptr = engine.eval_ast::<FnPtr>(&ast).unwrap();

        move |x: INT| -> Result<String, _> { fn_ptr.call(&engine, &ast, (x,)) }
    };

    // 'f' captures: the Engine, the AST, and the closure
    assert_eq!(f(42).unwrap(), "hello42");
}
