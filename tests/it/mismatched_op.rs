use rawr::{Engine, EvalAltResult, INT, ParseErrorType, resolve::Error};

#[test]
fn test_mismatched_op() {
    let engine = Engine::new();

    assert!(matches!(
        *engine.eval::<INT>(r#""hello, ".to_string() + "world!""#).expect_err("expects error"),
        EvalAltResult::ErrorMismatchOutputType(need, actual, ..) if need == std::any::type_name::<INT>() && actual == "string"
    ));
}

#[test]
fn test_mismatched_op_name() {
    let engine = Engine::new();

    assert!(matches!(
        *engine.eval::<String>("true").expect_err("expects error"),
        EvalAltResult::ErrorMismatchOutputType(need, actual, ..) if need == "string" && actual == "bool"
    ));

    assert!(matches!(
        *engine.eval::<&str>("true").expect_err("expects error"),
        EvalAltResult::ErrorMismatchOutputType(need, actual, ..) if need == "&str" && actual == "bool"
    ));
}

#[test]
#[cfg(not(feature = "no_object"))]
fn test_mismatched_op_custom_type() {
    #[allow(dead_code)] // used inside `register_type_with_name`
    #[derive(Debug, Clone)]
    struct TestStruct {
        x: INT,
    }

    impl TestStruct {
        fn new() -> Self {
            Self { x: 1 }
        }
    }

    let mut engine = Engine::new();

    engine.register_type_with_name::<TestStruct>("TestStruct").register_fn("new_ts", TestStruct::new);

    // A registered function returns an untyped value, which the checker won't mix with a typed one.
    let type_mismatch = |script: &str| {
        let err = engine.eval::<bool>(script).expect_err(script);
        matches!(*err, EvalAltResult::ErrorParsing(ParseErrorType::Resolution(Error::TypeMismatch { .. }), ..))
    };
    assert!(type_mismatch("new_ts() == 42"));
    assert!(type_mismatch("60 + new_ts()"));

    // Two untyped values can't be compared either.
    assert!(matches!(
        *engine.eval::<bool>("let x = new_ts(); let y = new_ts(); x == y").unwrap_err(),
        EvalAltResult::ErrorParsing(ParseErrorType::Resolution(Error::UnsupportedNominalSyntax { construct, .. }), ..) if construct == "equality of dynamic values"
    ));

    assert!(matches!(
        *engine.eval::<TestStruct>("42").unwrap_err(),
        EvalAltResult::ErrorMismatchOutputType(need, actual, ..)
            if need == "TestStruct" && actual == std::any::type_name::<INT>()
    ));
}
