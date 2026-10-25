use rawr::{Engine, FLOAT};

const EPSILON: FLOAT = 0.000_000_000_1;

#[test]
fn test_float() {
    let engine = Engine::new();

    assert!(engine.eval::<bool>("let x = 0.0; let y = 1.0; x < y").unwrap());
    assert!(!engine.eval::<bool>("let x = 0.0; let y = 1.0; x > y").unwrap());
    assert!(!engine.eval::<bool>("let x = 0.; let y = 1.; x > y").unwrap());
    assert!((engine.eval::<FLOAT>("let x = 9.9999; x").unwrap() - 9.9999 as FLOAT).abs() < EPSILON);
}

#[test]
fn test_float_scientific() {
    let engine = Engine::new();

    assert!(engine.eval::<bool>("123.456 == 1.23456e2").unwrap());
    assert!(engine.eval::<bool>("123.456 == 1.23456e+2").unwrap());
    assert!(engine.eval::<bool>("123.456 == 123456e-3").unwrap());

    // Test uppercase 'E' as well
    assert!(engine.eval::<bool>("123.456 == 1.23456E2").unwrap());
    assert!(engine.eval::<bool>("123.456 == 1.23456E+2").unwrap());
    assert!(engine.eval::<bool>("123.456 == 123456E-3").unwrap());

    let _ = engine.compile("123.456e1.23").unwrap_err();
}

#[test]
fn test_float_epsilon() {
    let engine = Engine::new();

    assert!(engine.eval::<bool>("1e-19 < 1e-18").unwrap());
    assert!(engine.eval::<bool>("0.0 < 1e-15").unwrap());
    assert!(engine.eval::<bool>("0.00000000000000000001 < 0.00000000000000000001000000000000001").unwrap());
}

#[test]
fn test_float_parse() {
    let engine = Engine::new();

    assert!((engine.eval::<FLOAT>(r#""9.9999".parse::<f64>().unwrap()"#).unwrap() - 9.9999 as FLOAT).abs() < EPSILON);
}

#[test]
fn test_struct_with_float() {
    let engine = Engine::new();

    let types = "
        struct TestStruct { x: f64 }

        impl TestStruct {
            fn new() -> Self { Self { x: 1.0 } }
            fn update(&mut self) { self.x += 5.789; }
        }
    ";

    assert!((engine.eval::<FLOAT>(&format!("{types} let mut ts = TestStruct::new(); ts.update(); ts.x")).unwrap() - 6.789).abs() < EPSILON);
    assert!((engine.eval::<FLOAT>(&format!("{types} let mut ts = TestStruct::new(); ts.x = 10.1001; ts.x")).unwrap() - 10.1001).abs() < EPSILON);
}

#[test]
fn test_float_func() {
    let mut engine = Engine::new();

    engine.register_fn("sum", |x: FLOAT, y: FLOAT, z: FLOAT, w: FLOAT| x + y + z + w);

    assert_eq!(engine.eval::<FLOAT>("sum(1.0, 2.0, 3.0, 4.0)").unwrap(), 10.0);
}
