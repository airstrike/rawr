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

/// The default depth is the same in a debug build as in a release one, and
/// deep enough for a recursion a debug build of Rust runs: script calls
/// grow the stack, so a test thread's 2 MiB stack holds 5000 of them
/// (#1624).
#[test]
fn the_default_call_depth_is_rusts_in_every_build_and_grows_the_stack() {
    let mut engine = Engine::new();
    assert_eq!(engine.max_call_levels(), 100_000);

    let script = "
        struct Node { depth: i64 }

        impl Node {
            fn down(&self) -> i64 {
                match self.depth {
                    0 => 0,
                    depth => 1 + Node { depth: depth - 1 }.down(),
                }
            }
        }

        fn count(n: i64, seen: Vec<i64>) -> i64 {
            if n == 0 { seen.len() as i64 } else { count(n - 1, seen.iter().map(|x| x + 1).collect()) + 0 }
        }
    ";
    let ast = engine.compile(script).unwrap();
    let mut scope = rawr::Scope::new();
    let counted: INT = engine.call_fn(&mut scope, &ast, "count", (5000 as INT, vec![rawr::Dynamic::from(1 as INT)])).unwrap();
    assert_eq!(counted, 1);
    let depth: INT = engine.eval(&format!("{script} Node {{ depth: 5000 }}.down()")).unwrap();
    assert_eq!(depth, 5000);

    engine.set_max_call_levels(64);
    let error = engine.eval::<INT>(&format!("{script} Node {{ depth: 65 }}.down()")).unwrap_err();
    assert!(matches!(*error, EvalAltResult::ErrorStackOverflow(..)), "{error}");
}

#[test]
fn test_stack_overflow_parsing() {
    let mut engine = Engine::new();

    // Rust limits no expression's nesting, so neither does the parser,
    // which grows a 2 MiB test thread's stack (#962).
    let nested = format!("{}1{}", "(1+".repeat(300), ")".repeat(300));
    assert_eq!(engine.eval::<INT>(&nested).unwrap(), 301);

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
