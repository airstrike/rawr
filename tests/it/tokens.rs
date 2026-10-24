use rawr::{Engine, INT};

#[test]
fn test_tokens_disabled() {
    let mut engine = Engine::new();

    engine.compile("let x = if true { 42 } else { 0 };").unwrap();

    engine.disable_symbol("if"); // disable the 'if' keyword

    let error = engine.compile("let x = if true { 42 } else { 0 };").unwrap_err();
    assert!(error.to_string().contains("`if`"), "{error}");

    engine.compile("let mut x = 40 + 2; x += 1;").unwrap();

    engine.disable_symbol("+="); // disable the '+=' operator

    let _ = engine.compile("let mut x = 40 + 2; x += 1;").unwrap_err();
}

#[test]
fn test_tokens_unicode_xid_ident() {
    let engine = Engine::new();
    let result = engine.eval::<INT>(
        "
            fn すべての答え() -> i64 { 42 }
            すべての答え()
        ",
    );
    #[cfg(feature = "unicode-xid-ident")]
    assert_eq!(result.unwrap(), 42);

    #[cfg(not(feature = "unicode-xid-ident"))]
    let _ = result.unwrap_err();
}
