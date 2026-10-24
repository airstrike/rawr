use rawr::{Engine, INT};

#[test]
fn test_unary_after_binary() {
    let engine = Engine::new();

    assert_eq!(engine.eval::<INT>("10 % -4").unwrap(), 2);
    assert_eq!(engine.eval::<INT>("10 & -4").unwrap(), 8);
    assert_eq!(engine.eval::<INT>("10 | -4").unwrap(), -2);
    assert_eq!(engine.eval::<INT>("10 ^ -4").unwrap(), -10);
    assert_eq!(engine.eval::<INT>("10 & !4").unwrap(), 10);
    assert_eq!(engine.eval::<INT>("10 + -4 * -2").unwrap(), 18);

    // Rust has no unary `+`.
    let _ = engine.compile("10 % +4").unwrap_err();
}
