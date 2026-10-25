use rawr::{Engine, EvalAltResult, INT};

#[test]
fn test_max_operations() {
    let mut engine = Engine::new();
    engine.set_optimization_level(rawr::OptimizationLevel::None);
    engine.set_max_operations(500);

    engine.on_progress(|count| {
        if count % 100 == 0 {
            println!("{count}");
        }
        None
    });

    engine.run("let mut x = 0; while x < 20 { x += 1; }").unwrap();

    assert!(matches!(*engine.run("for x in 0..500 {}").unwrap_err(), EvalAltResult::ErrorTooManyOperations(..)));

    engine.set_max_operations(0);

    engine.run("for x in 0..10000 {}").unwrap();
}

#[test]
fn test_max_operations_literal() {
    let mut engine = Engine::new();
    engine.set_optimization_level(rawr::OptimizationLevel::None);
    engine.set_max_operations(10);

    engine.run("[1, 2, 3, 4, 5, 6, 7]").unwrap();

    assert!(matches!(*engine.run("[1, 2, 3, 4, 5, 6, 7, 8, 9]").unwrap_err(), EvalAltResult::ErrorTooManyOperations(..)));
}

#[test]
fn test_max_operations_functions() {
    let mut engine = Engine::new();
    engine.set_max_operations(500);

    engine.on_progress(|count| {
        if count % 100 == 0 {
            println!("{count}");
        }
        None
    });

    engine
        .run(
            r#"
                println!("Test1");
                let mut x = 0;

                while x < 15 {
                    println!("{x}");
                    x += 1;
                }
            "#,
        )
        .unwrap();

    engine
        .run(
            r#"
                println!("Test2");
                fn inc(x: i64) -> i64 { x + 1 }
                let mut x = 0;
                while x < 20 { x = inc(x); }
            "#,
        )
        .unwrap();

    assert!(matches!(
        *engine
            .run(
                r#"
                    println!("Test3");
                    fn inc(x: i64) -> i64 { x + 1 }
                    let mut x = 0;

                    while x < 36 {
                        println!("{x}");
                        x = inc(x);
                    }
                "#,
            )
            .unwrap_err(),
        EvalAltResult::ErrorTooManyOperations(..)
    ));
}

#[test]
fn test_max_operations_progress() {
    let mut engine = Engine::new();
    engine.set_optimization_level(rawr::OptimizationLevel::None);
    engine.set_max_operations(500);

    engine.on_progress(|count| if count < 100 { None } else { Some((42 as INT).into()) });

    assert!(matches!(
        *engine.run("for x in 0..500 {}").unwrap_err(),
        EvalAltResult::ErrorTerminated(x, ..) if x.as_int().unwrap() == 42));
}
