use rawr::{Engine, INT};

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
