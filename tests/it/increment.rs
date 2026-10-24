use rawr::{Engine, INT};

#[test]
fn test_increment() {
    let engine = Engine::new();

    assert_eq!(engine.eval::<INT>("let mut x = 1; x += 2; x").unwrap(), 3);
    assert_eq!(engine.eval::<String>(r#"let mut s = String::from("test"); s += "ing"; s"#).unwrap(), "testing");
}
