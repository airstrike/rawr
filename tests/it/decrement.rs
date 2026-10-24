use rawr::{Engine, INT};

#[test]
fn test_decrement() {
    let engine = Engine::new();

    assert_eq!(engine.eval::<INT>("let mut x = 10; x -= 7; x").unwrap(), 3);
    assert_eq!(engine.eval::<String>(r#"let mut s = String::from("test"); s = s.replace('s', ""); s"#).unwrap(), "tet");
}
