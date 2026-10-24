use rawr::{Engine, EvalAltResult, INT};

#[test]
fn test_panic() {
    let engine = Engine::new();

    assert!(matches!(
        *engine.run(r#"if true { panic!("{}", 42) }"#).expect_err("expects error"),
        EvalAltResult::ErrorRuntime(s, ..) if s.to_string() == "42"
    ));

    assert!(matches!(
        *engine.run(r#"panic!("boom")"#).expect_err("expects error"),
        EvalAltResult::ErrorRuntime(s, ..) if s.to_string() == "boom"
    ));
}

#[test]
fn test_result() {
    let engine = Engine::new();

    let fail = "fn fail() -> Result<i64, i64> { Err(42) }";

    assert_eq!(engine.eval::<INT>(&format!("{fail} match fail() {{ Ok(v) => v, Err(x) => x }}")).unwrap(), 42);

    assert_eq!(engine.eval::<INT>(&format!("{fail} match fail() {{ Ok(v) => v, Err(_) => 123 }}")).unwrap(), 123);

    // `?` returns the error from the calling function.
    assert_eq!(
        engine
            .eval::<INT>(&format!(
                "
                    {fail}
                    fn foo() -> Result<i64, i64> {{ let x = fail()?; Ok(x + 1) }}
                    foo().unwrap_or_else(|x| x)
                "
            ))
            .unwrap(),
        42
    );

    // The error's binding shadows an outer one only inside its arm.
    assert_eq!(
        engine
            .eval::<INT>(&format!(
                "
                    {fail}
                    let err = 123;
                    if let Err(err) = fail() {{ err }} else {{ 0 }}
                "
            ))
            .unwrap(),
        42
    );

    assert_eq!(
        engine
            .eval::<INT>(&format!(
                r#"
                    {fail}
                    let err = 123;
                    if let Err(err) = fail() {{ println!("{{err}}"); }}
                    err
                "#
            ))
            .unwrap(),
        123
    );

    #[cfg(not(feature = "unchecked"))]
    assert!(matches!(*engine.run("let x = 42; let y = 0; let z = x / y;").expect_err("expects error"), EvalAltResult::ErrorArithmetic(..)));
}
