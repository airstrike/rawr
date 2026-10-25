use rawr::{Engine, INT};

#[test]
fn test_binary_ops() {
    let engine = Engine::new();

    assert_eq!(engine.eval::<INT>("10 + 4").unwrap(), 14);
    assert_eq!(engine.eval::<INT>("10 - 4").unwrap(), 6);
    assert_eq!(engine.eval::<INT>("10 * 4").unwrap(), 40);
    assert_eq!(engine.eval::<INT>("10 / 4").unwrap(), 2);
    assert_eq!(engine.eval::<INT>("10 % 4").unwrap(), 2);
    assert_eq!(engine.eval::<INT>("10_i64.pow(4)").unwrap(), 10000);
    assert_eq!(engine.eval::<INT>("10 << 4").unwrap(), 160);
    assert_eq!(engine.eval::<INT>("10 >> 4").unwrap(), 0);
    assert_eq!(engine.eval::<INT>("10 & 4").unwrap(), 0);
    assert_eq!(engine.eval::<INT>("10 | 4").unwrap(), 14);
    assert_eq!(engine.eval::<INT>("10 ^ 4").unwrap(), 14);

    assert!(engine.eval::<bool>("42 == 42").unwrap());
    assert!(!engine.eval::<bool>("42 != 42").unwrap());
    assert!(!engine.eval::<bool>("42 > 42").unwrap());
    assert!(engine.eval::<bool>("42 >= 42").unwrap());
    assert!(!engine.eval::<bool>("42 < 42").unwrap());
    assert!(engine.eval::<bool>("42 <= 42").unwrap());

    assert_eq!(engine.eval::<INT>("let mut x = 10; x += 4; x").unwrap(), 14);
    assert_eq!(engine.eval::<INT>("let mut x = 10; x -= 4; x").unwrap(), 6);
    assert_eq!(engine.eval::<INT>("let mut x = 10; x *= 4; x").unwrap(), 40);
    assert_eq!(engine.eval::<INT>("let mut x = 10; x /= 4; x").unwrap(), 2);
    assert_eq!(engine.eval::<INT>("let mut x = 10; x %= 4; x").unwrap(), 2);
    assert_eq!(engine.eval::<INT>("let mut x = 10; x <<= 4; x").unwrap(), 160);
    assert_eq!(engine.eval::<INT>("let mut x = 10; x >>= 4; x").unwrap(), 0);
    assert_eq!(engine.eval::<INT>("let mut x = 10; x &= 4; x").unwrap(), 0);
    assert_eq!(engine.eval::<INT>("let mut x = 10; x |= 4; x").unwrap(), 14);
    assert_eq!(engine.eval::<INT>("let mut x = 10; x ^= 4; x").unwrap(), 14);

    {
        use rawr::FLOAT;

        assert_eq!(engine.eval::<FLOAT>("10.0 + 4.0").unwrap(), 14.0);
        assert_eq!(engine.eval::<FLOAT>("10.0 - 4.0").unwrap(), 6.0);
        assert_eq!(engine.eval::<FLOAT>("10.0 * 4.0").unwrap(), 40.0);
        assert_eq!(engine.eval::<FLOAT>("10.0 / 4.0").unwrap(), 2.5);
        assert_eq!(engine.eval::<FLOAT>("10.0 % 4.0").unwrap(), 2.0);
        assert_eq!(engine.eval::<FLOAT>("10.0_f64.powf(4.0)").unwrap(), 10000.0);

        // Rust doesn't mix integers and floats in one operation.
        let _ = engine.compile("10.0 + 4").unwrap_err();
        let _ = engine.compile("10 * 4.0").unwrap_err();
        let _ = engine.compile("42 == 42.0").unwrap_err();
        let _ = engine.compile("42.0 < 42").unwrap_err();

        assert_eq!(engine.eval::<FLOAT>("let mut x = 10.0; x += 4.0; x").unwrap(), 14.0);
        assert_eq!(engine.eval::<FLOAT>("let mut x = 10.0; x -= 4.0; x").unwrap(), 6.0);
        assert_eq!(engine.eval::<FLOAT>("let mut x = 10.0; x *= 4.0; x").unwrap(), 40.0);
        assert_eq!(engine.eval::<FLOAT>("let mut x = 10.0; x /= 4.0; x").unwrap(), 2.5);
        assert_eq!(engine.eval::<FLOAT>("let mut x = 10.0; x %= 4.0; x").unwrap(), 2.0);
    }

    assert_eq!(engine.eval::<String>(r#""hello".to_string() + ", world""#).unwrap(), "hello, world");
    assert_eq!(engine.eval::<String>(r#"let mut s = "hello".to_string(); s.push('!'); s"#).unwrap(), "hello!");

    assert!(!engine.eval::<bool>(r#""a" == "x""#).unwrap());
    assert!(engine.eval::<bool>(r#""a" != "x""#).unwrap());
    assert!(!engine.eval::<bool>(r#""a" > "x""#).unwrap());
    assert!(!engine.eval::<bool>(r#""a" >= "x""#).unwrap());
    assert!(engine.eval::<bool>(r#""a" < "x""#).unwrap());
    assert!(engine.eval::<bool>(r#""a" <= "x""#).unwrap());

    // Values of different types can't be compared.
    let _ = engine.compile(r#""x" == 'x'"#).unwrap_err();
    let _ = engine.compile("true == 42").unwrap_err();
    let _ = engine.compile("true < 42").unwrap_err();
    let _ = engine.compile(r#""42" == 42"#).unwrap_err();
    let _ = engine.compile(r#""42" >= 42"#).unwrap_err();
}
