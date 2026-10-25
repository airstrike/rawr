use rawr::{Engine, INT, ParseErrorType, resolve::Error};

#[test]
fn test_loop() {
    let engine = Engine::new();

    assert_eq!(
        engine
            .eval::<INT>(
                "
                    let mut x = 0;
                    let mut i = 0;

                    loop {
                        if i < 10 {
                            i += 1;
                            if x > 20 { continue; }
                            x += i;
                        } else {
                            break;
                        }
                    }

                    x
                "
            )
            .unwrap(),
        21
    );

    assert_eq!(
        engine
            .eval::<INT>(
                "
                    let mut found = 0;
                    for n in 0..10 {
                        let Some(_x) = (if n <= 5 { Some(n) } else { None }) else {
                            found = 42;
                            break;
                        };
                    }
                    found
                "
            )
            .unwrap(),
        42
    );

    let outside_loop = |script: &str| {
        let err = engine.compile(script).expect_err(script);
        matches!(err.err_type(), ParseErrorType::Resolution(Error::UnsupportedNominalSyntax { construct, .. }) if construct == "`break` or `continue` outside a loop")
    };

    assert!(outside_loop("let x = 0; break;"));

    // A closure body is not inside the loop around it.
    assert!(outside_loop("loop { let f = || { break; }; }"));

    assert!(outside_loop("let x = 0; if x > 0 { continue; }"));
}

#[test]
fn test_loop_expression() {
    let mut engine = Engine::new();

    // `break` with a value ends a `loop`, as in Rust, not a `while` (E0571).
    assert_eq!(
        engine
            .eval::<INT>(
                "
                    let mut x = 0;

                    let value = loop {
                        if x % 5 == 0 { break 42; }
                        x += 1;
                    };

                    value
                "
            )
            .unwrap(),
        42
    );

    engine.set_allow_loop_expressions(false);

    let _ = engine
        .eval::<INT>(
            "
				let mut x = 0;

				let value = loop {
                    if x % 5 == 0 { break 42; }
                    x += 1;
				};

				value
		    ",
        )
        .unwrap_err();
}
