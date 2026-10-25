use rawr::{Engine, INT, ParseErrorType, resolve::Error};

#[test]
fn test_ops() {
    let engine = Engine::new();

    assert_eq!(engine.eval::<INT>("60 + 5").unwrap(), 65);
    assert_eq!(engine.eval::<INT>("(1 + 2) * (6 - 4) / 2").unwrap(), 3);
    assert_eq!(engine.eval::<INT>("let mut x = 41; x = x + 1; x").unwrap(), 42);
    assert_eq!(engine.eval::<String>(r#"let mut s = String::from("hello"); s = s + &42.to_string(); s"#).unwrap(), "hello42");
}

#[test]
fn test_ops_other_number_types() {
    let engine = Engine::new();

    assert!(engine.eval::<bool>("let x: u16 = 42; x == 42").unwrap());

    // Values of different types don't compare, as in Rust.
    assert!(matches!(
        engine.compile("let x: u16 = 42; x == 42_i64").unwrap_err().err_type(),
        ParseErrorType::Resolution(Error::TypeMismatch { expected, .. }) if expected == "u16"
    ));
    assert!(matches!(
        engine.compile("let x: u16 = 42; x == 42.0").unwrap_err().err_type(),
        ParseErrorType::Resolution(Error::TypeMismatch { expected, .. }) if expected == "u16"
    ));
    assert!(matches!(
        engine.compile(r#"let x: u16 = 42; x == "hello""#).unwrap_err().err_type(),
        ParseErrorType::Resolution(Error::TypeMismatch { expected, .. }) if expected == "u16"
    ));
}

#[test]
fn test_ops_strings() {
    let engine = Engine::new();

    assert!(engine.eval::<bool>(r#""hello" > "c""#).unwrap());
    assert!(engine.eval::<bool>(r#""" < "c""#).unwrap());
    assert!(engine.eval::<bool>("'x' > 'h'").unwrap());
    assert!(engine.eval::<bool>(r#""hello" > "foo""#).unwrap());

    // A string and a char don't compare, as in Rust.
    assert!(matches!(engine.compile(r#""hello" > 'c'"#).unwrap_err().err_type(), ParseErrorType::Resolution(Error::TypeMismatch { .. })));
}

#[test]
fn test_ops_precedence() {
    let engine = Engine::new();

    assert_eq!(engine.eval::<INT>("let mut x = 0; if x == 10 || true { x = 1 } x").unwrap(), 1);
}

#[test]
fn test_ops_custom_types() {
    let engine = Engine::new();

    let types = "
        struct Test1;

        impl PartialEq for Test1 {
            fn eq(&self, _: &Self) -> bool { true }
        }

        #[derive(PartialEq, PartialOrd)]
        struct Test2(i64);
    ";

    assert!(engine.eval::<bool>(&format!("{types} let x = Test1; let y = Test1; x == y")).unwrap());
    assert!(!engine.eval::<bool>(&format!("{types} let x = Test1; let y = Test1; x != y")).unwrap());
    assert!(engine.eval::<bool>(&format!("{types} let x = Test2(1); let y = Test2(2); x < y")).unwrap());
    assert!(engine.eval::<bool>(&format!("{types} let x = Test2(1); let y = Test2(2); y >= x")).unwrap());
    assert!(!engine.eval::<bool>(&format!("{types} let x = Test2(1); x > Test2(1)")).unwrap());
}
