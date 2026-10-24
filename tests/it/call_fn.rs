#![cfg(not(feature = "no_function"))]
use rawr::{AST, CallFnOptions, Dynamic, Engine, EvalAltResult, Func, FuncArgs, INT, Scope};

#[test]
fn test_call_fn() {
    let mut engine = Engine::new();
    let mut scope = Scope::new();

    engine.register_fn("test", |x: INT, y: bool| if y { x + 1 } else { x - 1 });
    scope.push("foo", 42 as INT);

    let ast = engine
        .compile(
            "
                fn hello(x: i64, y: i64) -> i64 {
                    x + y
                }
                fn twice(x: i64) -> i64 {
                    x * 2
                }
                fn answer() -> i64 {
                    42
                }
                fn define_var(scale: i64) -> i64 {
                    let bar = 21;
                    bar * scale
                }
            ",
        )
        .unwrap();

    let r = engine.call_fn::<INT>(&mut scope, &ast, "hello", (42 as INT, 123 as INT)).unwrap();
    assert_eq!(r, 165);

    let r = engine.call_fn::<INT>(&mut scope, &ast, "twice", (123 as INT,)).unwrap();
    assert_eq!(r, 246);

    let r = engine.call_fn::<INT>(&mut scope, &ast, "answer", ()).unwrap();
    assert_eq!(r, 42);

    // The scope keeps its variables across calls.
    assert_eq!(scope.get_value::<INT>("foo").expect("variable foo should exist"), 42);

    let r = engine.call_fn::<INT>(&mut scope, &ast, "define_var", (2 as INT,)).unwrap();
    assert_eq!(r, 42);

    assert!(!scope.contains("bar"));

    let options = CallFnOptions::new().eval_ast(false).rewind_scope(false);

    let r = engine.call_fn_with_options::<INT>(options, &mut scope, &ast, "define_var", (2 as INT,)).unwrap();
    assert_eq!(r, 42);

    assert_eq!(scope.get_value::<INT>("bar").expect("variable bar should exist"), 21);

    assert!(!scope.contains("scale"));

    let options = CallFnOptions::new().in_all_namespaces(true);

    let _ = engine.call_fn::<INT>(&mut scope, &ast, "test", (41 as INT, true)).unwrap_err();

    let r = engine.call_fn_with_options::<INT>(options, &mut scope, &ast, "test", (41 as INT, true)).unwrap();
    assert_eq!(r, 42);
}

#[test]
fn test_call_fn_scope() {
    let engine = Engine::new();
    let mut scope = Scope::new();

    let ast = engine
        .compile(
            "
                fn foo(x: i64) -> i64 {
                    let hello = 42;
                    123 + hello + x
                }

                let bar = 123;
            ",
        )
        .unwrap();

    for _ in 0..50 {
        assert_eq!(
            engine
                .call_fn_with_options::<INT>(CallFnOptions::new().rewind_scope(false), &mut scope, &ast, "foo", [Dynamic::THREE],)
                .unwrap(),
            168
        );
    }

    assert_eq!(scope.len(), 100);
}

struct Options {
    pub foo: bool,
    pub bar: String,
    pub baz: INT,
}

impl FuncArgs for Options {
    fn parse<C: Extend<Dynamic>>(self, container: &mut C) {
        container.extend(Some(self.foo.into()));
        container.extend(Some(self.bar.into()));
        container.extend(Some(self.baz.into()));
    }
}

#[test]
fn test_call_fn_args() {
    let options = Options { foo: false, bar: "world".to_string(), baz: 42 };

    let engine = Engine::new();
    let mut scope = Scope::new();

    let ast = engine
        .compile(
            r#"
                fn hello(x: bool, y: String, z: i64) -> String {
                    if x { format!("hello {y}") } else { format!("{y}{z}") }
                }
            "#,
        )
        .unwrap();

    let result = engine.call_fn::<String>(&mut scope, &ast, "hello", options).unwrap();

    assert_eq!(result, "world42");
}

#[test]
fn test_call_fn_private() {
    let engine = Engine::new();
    let mut scope = Scope::new();

    let ast = engine.compile("pub fn add(x: i64, n: i64) -> i64 { x + n }").unwrap();

    let r = engine.call_fn::<INT>(&mut scope, &ast, "add", (40 as INT, 2 as INT)).unwrap();
    assert_eq!(r, 42);

    // A function without `pub` is private to the script, and the host still calls it.
    let ast = engine.compile("fn add(x: i64, n: i64, ) -> i64 { x + n }").unwrap();

    let r = engine.call_fn::<INT>(&mut scope, &ast, "add", (40 as INT, 2 as INT)).unwrap();
    assert_eq!(r, 42);
}

#[test]
fn test_anonymous_fn() {
    let calc_func = Func::<(INT, INT, INT), INT>::create_from_script(Engine::new(), "fn calc(x: i64, y: i64, z: i64,) -> i64 { (x + y) * z }", "calc").unwrap();
    assert_eq!(calc_func(42, 123, 9).unwrap(), 1485);

    let calc_func =
        Func::<(INT, String, INT), INT>::create_from_script(Engine::new(), "fn calc(x: i64, y: String, z: i64) -> i64 { (x + y.len() as i64) * z }", "calc").unwrap();
    assert_eq!(calc_func(42, "hello".to_string(), 9).unwrap(), 423);

    let calc_func =
        Func::<(INT, String, INT), INT>::create_from_script(Engine::new(), "pub fn calc(x: i64, y: String, z: i64) -> i64 { (x + y.len() as i64) * z }", "calc")
            .unwrap();
    assert_eq!(calc_func(42, "hello".to_string(), 9).unwrap(), 423);

    let calc_func = Func::<(INT, &str, INT), INT>::create_from_script(Engine::new(), "fn calc(x: i64, y: &str, z: i64) -> i64 { (x + y.len() as i64) * z }", "calc").unwrap();
    assert_eq!(calc_func(42, "hello", 9).unwrap(), 423);
}

#[test]
fn test_call_fn_events() {
    // Event handler
    struct Handler {
        // Scripting engine
        pub engine: Engine,
        // Use a custom 'Scope' to keep stored state
        pub scope: Scope<'static>,
        // Program script
        pub ast: AST,
    }

    const SCRIPT: &str = r#"
        fn start(data: i64) -> i64 { 42 + data }
        fn end(_data: i64) -> i64 { 0 }
    "#;

    impl Handler {
        pub fn new() -> Self {
            let engine = Engine::new();

            // Create a custom 'Scope' to hold state
            let mut scope = Scope::new();

            // Add initialized state into the custom 'Scope'
            scope.push("state", false);

            // Compile the handler script.
            let ast = engine.compile(SCRIPT).unwrap();

            // Evaluate the script to initialize it and other state variables.
            // In a real application you'd again be handling errors...
            engine.run_ast_with_scope(&mut scope, &ast).unwrap();

            // The event handler is essentially these three items:
            Handler { engine, scope, ast }
        }

        // Say there are three events: 'start', 'end', 'update'.
        // In a real application you'd be handling errors...
        pub fn on_event(&mut self, event_name: &str, event_data: INT) -> Dynamic {
            let engine = &self.engine;
            let scope = &mut self.scope;
            let ast = &self.ast;

            match event_name {
                // The 'start' event maps to function 'start'.
                // In a real application you'd be handling errors...
                "start" => engine.call_fn(scope, ast, "start", (event_data,)).unwrap(),

                // The 'end' event maps to function 'end'.
                // In a real application you'd be handling errors...
                "end" => engine.call_fn(scope, ast, "end", (event_data,)).unwrap(),

                // The 'update' event maps to function 'update'.
                // This event provides a default implementation when the script-defined function is not found.
                "update" => engine
                    .call_fn(scope, ast, "update", (event_data,))
                    .or_else(|err| match *err {
                        EvalAltResult::ErrorFunctionNotFound(fn_name, ..) if fn_name.starts_with("update") => {
                            // Default implementation of 'update' event handler
                            self.scope.set_value("state", true);
                            // Turn function-not-found into a success
                            Ok(Dynamic::UNIT)
                        }
                        _ => Err(err),
                    })
                    .unwrap(),
                // In a real application you'd be handling unknown events...
                _ => panic!("unknown event: {}", event_name),
            }
        }
    }

    let mut handler = Handler::new();
    assert!(!handler.scope.get_value::<bool>("state").unwrap());
    let _ = handler.on_event("update", 999);
    assert!(handler.scope.get_value::<bool>("state").unwrap());
    assert_eq!(handler.on_event("start", 999).as_int().unwrap(), 1041);
}
