use rawr::{Engine, INT};

#[test]
fn test_while() {
    let engine = Engine::new();

    assert_eq!(
        engine
            .eval::<INT>(
                "
                    let mut x = 0;

                    while x < 10 {
                        x += 1;
                        if x > 5 { break; }
                        if x > 3 { continue; }
                        x += 3;
                    }

                    x
                ",
            )
            .unwrap(),
        6
    );

    // Only `loop` breaks with a value, as in Rust.
    assert_eq!(
        engine
            .eval::<INT>(
                "
                    let mut x = 0;

                    loop {
                        x += 1;
                        if x > 5 { break x * 2; }
                        if x > 3 { continue; }
                        x += 3;
                    }
                ",
            )
            .unwrap(),
        12
    );

    engine.run("while false {}").unwrap();

    assert_eq!(engine.eval::<INT>("loop { break 42; }").unwrap(), 42);
}

#[cfg(not(feature = "unchecked"))]
#[test]
fn test_infinite_loops() {
    let mut engine = Engine::new();

    engine.set_max_operations(1024);

    let _ = engine.run("loop {}").unwrap_err();
    let _ = engine.run("while true {}").unwrap_err();
}
