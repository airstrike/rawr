use rawr::{Engine, EvalAltResult, INT, ParseErrorType};

#[test]
fn test_stack_overflow_fn_calls() {
    let engine = Engine::new();

    assert_eq!(
        engine
            .eval::<INT>(
                "
                    fn foo(n: i64) -> i64 { if n <= 1 { 0 } else { n + foo(n-1) } }
                    foo(6)
                ",
            )
            .unwrap(),
        20
    );

    let max = engine.max_call_levels();

    assert!(matches!(
        *engine
            .run(&format!(
                "
                    fn foo(n: i64) -> i64 {{ if n == 0 {{ 0 }} else {{ n + foo(n-1) }} }}
                    foo({})
                ",
                max + 1
            ))
            .unwrap_err(),
        EvalAltResult::ErrorStackOverflow(..)
    ));
}

#[test]
fn test_stack_overflow_parsing() {
    // Deep parses need a main-thread-sized stack, not the 2 MiB test-thread default.
    std::thread::Builder::new().stack_size(8 << 20).spawn(stack_overflow_parsing).unwrap().join().unwrap();
}

fn stack_overflow_parsing() {
    let mut engine = Engine::new();

    // Deep nesting is refused with an error rather than overflowing the stack.
    let nested = format!("let a = {}1{};", "(1+".repeat(100), ")".repeat(100));
    assert_eq!(*engine.compile(&nested).unwrap_err().0, ParseErrorType::ExprTooDeep);

    engine.compile("1 + 2").unwrap();

    // A typed function body nests deeper in the code Rawr generates than in
    // its source, so the function limit leaves room for that.
    engine.set_max_expr_depths(100, 16);

    // Rawr's checked integer `+` nests in the code it generates, so a chain
    // of additions is deeper than its source. Ten terms fit in depth 100.
    engine.compile("1 + 2 + 3 + 4 + 5 + 6 + 7 + 8 + 9 + 0").unwrap();

    assert_eq!(
        *engine
            .compile(
                "
                    1 + 2 + 3 + 4 + 5 + 6 + 7 + 8 + 9 + 0 +
                    1 + 2 + 3 + 4 + 5 + 6 + 7 + 8 + 9 + 0 +
                    1 + 2 + 3 + 4 + 5 + 6 + 7 + 8 + 9 + 0 +
                    1 + 2 + 3 + 4 + 5 + 6 + 7 + 8 + 9 + 0 +
                    1 + 2 + 3 + 4 + 5 + 6 + 7 + 8 + 9 + 0 +
                    1 + 2 + 3 + 4 + 5 + 6 + 7 + 8 + 9 + 0 +
                    1 + 2 + 3 + 4 + 5 + 6 + 7 + 8 + 9 + 0 +
                    1 + 2 + 3 + 4 + 5 + 6 + 7 + 8 + 9 + 0 +
                    1 + 2 + 3 + 4 + 5 + 6 + 7 + 8 + 9 + 0 +
                    1 + 2 + 3 + 4 + 5 + 6 + 7 + 8 + 9 + 0
                "
            )
            .unwrap_err()
            .err_type(),
        ParseErrorType::ExprTooDeep
    );

    engine.compile("fn abc(x: i64) -> i64 { x + 1 }").unwrap();
}
