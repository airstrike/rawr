#![cfg(not(feature = "unchecked"))]
use rawr::{Engine, EvalAltResult, INT, ParseErrorType};

#[cfg(not(feature = "no_index"))]
use rawr::Array;

#[test]
fn test_max_string_size() {
    let mut engine = Engine::new();
    engine.set_max_string_size(10);

    assert_eq!(*engine.compile(r#"let x = "hello, world!";"#).unwrap_err().err_type(), ParseErrorType::LiteralTooLarge("Length of string".to_string(), 10));

    assert_eq!(*engine.compile(r#"let x = "朝に紅顔、暮に白骨";"#).unwrap_err().err_type(), ParseErrorType::LiteralTooLarge("Length of string".to_string(), 10));

    assert_eq!(*engine.compile(r##"let x = r#"朝に紅顔、暮に白骨"#;"##).unwrap_err().err_type(), ParseErrorType::LiteralTooLarge("Length of string".to_string(), 10));

    assert_eq!(*engine.compile(r#"let x = format!("Say {} and so be it.", "Hi!");"#).unwrap_err().err_type(), ParseErrorType::LiteralTooLarge("Length of string".to_string(), 10));

    assert!(matches!(
        *engine
            .run(
                r#"
                    let x = String::from("hello, ");
                    let y = "world!";
                    x + y
                "#
            )
            .unwrap_err(),
        EvalAltResult::ErrorDataTooLarge(..)
    ));

    assert!(matches!(
        *engine
            .run(
                r#"
                    let mut x = String::from("hello");
                    for _ in 0..100 {
                        x.push('!');
                    }
                    x
                "#
            )
            .unwrap_err(),
        EvalAltResult::ErrorDataTooLarge(..)
    ));

    engine.set_max_string_size(0);

    assert_eq!(
        engine
            .eval::<String>(
                r#"
                    let x = String::from("hello, ");
                    let y = "world!";
                    x + y
                "#
            )
            .unwrap(),
        "hello, world!"
    );
}

#[test]
#[cfg(not(feature = "no_index"))]
fn test_max_array_size() {
    let mut engine = Engine::new();
    engine.set_max_array_size(10);

    assert_eq!(*engine.compile("let x = [1,2,3,4,5,6,7,8,9,10,11,12,13,14,15];").unwrap_err().err_type(), ParseErrorType::LiteralTooLarge("Size of array literal".to_string(), 10));

    assert!(matches!(
        *engine
            .run(
                "
                    let mut x = vec![1, 2, 3, 4, 5, 6];
                    let y = vec![7, 8, 9, 10, 11, 12];
                    x.extend(y);
                "
            )
            .unwrap_err(),
        EvalAltResult::ErrorDataTooLarge(..)
    ));

    #[cfg(not(feature = "no_closure"))]
    assert_eq!(
        engine
            .eval::<INT>(
                "
                    let x = 42;
                    let mut y = Vec::new();
                    let _f = || x;
                    for _ in 0..10 {
                        y.push(x);
                    }
                    y.len() as i64
                "
            )
            .unwrap(),
        10
    );

    #[cfg(not(feature = "no_closure"))]
    assert!(matches!(
        *engine
            .run(
                "
                    let x = 42;
                    let mut y = Vec::new();
                    let _f = || x;
                    for _ in 0..11 {
                        y.push(x);
                    }
                "
            )
            .unwrap_err(),
        EvalAltResult::ErrorDataTooLarge(..)
    ));

    assert_eq!(
        engine
            .eval::<INT>(
                "
                    let mut x = vec![1, 2, 3, 4, 5, 6];
                    x.resize(10, 42);
                    x.len() as i64
                "
            )
            .unwrap(),
        10
    );

    assert!(matches!(
        *engine
            .run(
                "
                    let mut x = vec![1, 2, 3, 4, 5, 6];
                    x.resize(11, 42);
                "
            )
            .unwrap_err(),
        EvalAltResult::ErrorDataTooLarge(..)
    ));

    assert_eq!(
        engine
            .eval::<INT>(
                "
                    let x = vec![1, 2];
                    vec![x.clone(), x.clone(), x].len() as i64
                "
            )
            .unwrap(),
        3
    );

    assert!(matches!(
        *engine
            .run(
                "
                    let x = vec![1, 2, 3];
                    vec![x.clone(), x.clone(), x.clone(), x]
                "
            )
            .unwrap_err(),
        EvalAltResult::ErrorDataTooLarge(..)
    ));

    assert!(matches!(
        *engine
            .run(
                "
                    let x = vec![1];
                    let y = vec![x.clone(), x];
                    let z = vec![y.clone(), y];
                    vec![z.clone(), z.clone(), z]
                "
            )
            .unwrap_err(),
        EvalAltResult::ErrorDataTooLarge(..)
    ));

    engine.set_max_array_size(0);

    assert_eq!(
        engine
            .eval::<Array>(
                "
                    let mut x = vec![1, 2, 3, 4, 5, 6];
                    let y = vec![7, 8, 9, 10, 11, 12];
                    x.extend(y);
                    x
                "
            )
            .unwrap()
            .len(),
        12
    );

    assert_eq!(
        engine
            .eval::<Array>(
                "
                    let x = vec![1, 2, 3];
                    vec![x.clone(), x.clone(), x.clone(), x]
                "
            )
            .unwrap()
            .len(),
        4
    );
}
