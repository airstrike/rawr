use rawr::{Engine, EvalAltResult, INT};

use rawr::FLOAT;

#[test]
fn test_math() {
    let engine = Engine::new();

    assert_eq!(engine.eval::<INT>("1 + 2").unwrap(), 3);
    assert_eq!(engine.eval::<INT>("1 - 2").unwrap(), -1);
    assert_eq!(engine.eval::<INT>("2 * 3").unwrap(), 6);
    assert_eq!(engine.eval::<INT>("1 / 2").unwrap(), 0);
    assert_eq!(engine.eval::<INT>("3 % 2").unwrap(), 1);

    assert!((engine.eval::<FLOAT>("(std::f64::consts::PI / 6.0).sin()").unwrap() - 0.5).abs() < 0.001);

    assert!(engine.eval::<FLOAT>("(std::f64::consts::PI / 2.0).cos()").unwrap().abs() < 0.001);

    assert_eq!(engine.eval::<INT>("(-9223372036854775807_i64).abs()").unwrap(), 9_223_372_036_854_775_807);

    // Overflow, underflow and division by zero are runtime errors. The operands come in as
    // function arguments, so the checker can't see the overflow ahead of time.
    {
        let ops = "
            fn add(a: i64, b: i64) -> i64 { a + b }
            fn sub(a: i64, b: i64) -> i64 { a - b }
            fn mul(a: i64, b: i64) -> i64 { a * b }
            fn div(a: i64, b: i64) -> i64 { a / b }
            fn rem(a: i64, b: i64) -> i64 { a % b }
            fn magnitude(a: i64) -> i64 { a.abs() }
        ";
        let arithmetic_error = |call: &str| {
            let err = engine.eval::<INT>(&format!("{ops} {call}")).expect_err(call);
            match *err {
                EvalAltResult::ErrorInFunctionCall(_, _, inner, ..) => matches!(*inner, EvalAltResult::ErrorArithmetic(..)),
                other => matches!(other, EvalAltResult::ErrorArithmetic(..)),
            }
        };

        assert!(arithmetic_error("magnitude(i64::MIN)"));
        assert!(arithmetic_error("add(i64::MAX, 1)"));
        assert!(arithmetic_error("sub(i64::MIN, 1)"));
        assert!(arithmetic_error("mul(i64::MAX, i64::MAX)"));
        assert!(arithmetic_error("div(i64::MAX, 0)"));
        assert!(arithmetic_error("rem(i64::MAX, 0)"));
    }
}

#[test]
fn test_math_parse() {
    let engine = Engine::new();

    assert_eq!(engine.eval::<INT>(r#""42".parse::<i64>().unwrap()"#).unwrap(), 42);
}
