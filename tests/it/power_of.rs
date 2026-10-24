use rawr::{Engine, INT};

#[cfg(not(feature = "no_float"))]
use rawr::FLOAT;

#[cfg(not(feature = "no_float"))]
const EPSILON: FLOAT = FLOAT::EPSILON;

#[test]
fn test_power_of() {
    let engine = Engine::new();

    assert_eq!(engine.eval::<INT>("2_i64.pow(3)").unwrap(), 8);
    assert_eq!(engine.eval::<INT>("(-2_i64).pow(3)").unwrap(), -8);
    assert_eq!(engine.eval::<INT>("2_i64.pow(3_u32.pow(2))").unwrap(), 512);
    assert_eq!(engine.eval::<INT>("let x = 4; x.pow(3)").unwrap(), 64);

    #[cfg(not(feature = "no_float"))]
    {
        assert!((engine.eval::<FLOAT>("2.2_f64.powf(3.3)").unwrap() - (2.2 as FLOAT).powf(3.3)).abs() <= EPSILON);
        assert!((engine.eval::<FLOAT>("2.0_f64.powf(-2.0)").unwrap() - 0.25 as FLOAT).abs() < EPSILON);
        assert!((engine.eval::<FLOAT>("(-2.0_f64).powf(-2.0)").unwrap() - 0.25 as FLOAT).abs() < EPSILON);
        assert!((engine.eval::<FLOAT>("(-2.0_f64).powi(-2)").unwrap() - 0.25 as FLOAT).abs() < EPSILON);
    }

    // Rust has no `**` operator.
    let _ = engine.compile("2 ** 3").unwrap_err();
}

#[test]
fn test_power_of_assign() {
    let engine = Engine::new();

    assert_eq!(engine.eval::<INT>("let mut x: i64 = 2; x = x.pow(3); x").unwrap(), 8);
    assert_eq!(engine.eval::<INT>("let mut x: i64 = -2; x = x.pow(3); x").unwrap(), -8);

    #[cfg(not(feature = "no_float"))]
    {
        assert!((engine.eval::<FLOAT>("let mut x = 2.2_f64; x = x.powf(3.3); x").unwrap() - (2.2 as FLOAT).powf(3.3)).abs() <= EPSILON);
        assert!((engine.eval::<FLOAT>("let mut x = -2.0_f64; x = x.powi(-2); x").unwrap() - 0.25 as FLOAT).abs() < EPSILON);
    }

    // Rust has no `**=` operator.
    let _ = engine.compile("let mut x = 2; x **= 3; x").unwrap_err();
}
