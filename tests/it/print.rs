use rawr::{Dynamic, Engine, Output};
use std::sync::{Arc, RwLock};

#[test]
fn test_to_string() {
    let engine = Engine::new();

    assert_eq!(engine.eval::<String>("let x: u8 = 42; x.to_string()").unwrap(), "42");
    assert_eq!(engine.eval::<String>("let y: i32 = 42; y.to_string()").unwrap(), "42");
    assert_eq!(engine.eval::<String>("let z: i16 = 42; z.to_string()").unwrap(), "42");
}

#[test]
fn test_print_debug() {
    let logbook = Arc::new(RwLock::new(Vec::<String>::new()));

    // Redirect the output of `println!` and `eprintln!` to the logbook.
    let log = logbook.clone();

    let mut engine = Engine::new();

    engine.on_output(move |output, text| {
        let stream = if output == Output::Stderr { "err" } else { "out" };
        log.write().unwrap().push(format!("{stream}: {text}"))
    });

    // Evaluate script
    engine.run(r#"println!("{}", 40 + 2)"#).unwrap();
    let ast = engine.compile(r#"let x = "hello!"; eprintln!("{x:?}")"#).unwrap();
    let r = engine.eval_ast::<Dynamic>(&ast).unwrap();

    // The logbook captures each macro's text, its newline included.
    assert!(r.is_unit());
    assert_eq!(logbook.read().unwrap().len(), 2);
    assert_eq!(logbook.read().unwrap()[0], "out: 42\n");
    assert_eq!(logbook.read().unwrap()[1], "err: \"hello!\"\n");

    for entry in logbook.read().unwrap().iter() {
        print!("{entry}");
    }
}

#[test]
fn test_print_custom_type() {
    let engine = Engine::new();

    let types = "
        use std::fmt;

        struct MyStruct { field: i64 }

        impl fmt::Display for MyStruct {
            fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result { write!(f, \"hello: {}\", self.field) }
        }

        impl fmt::Debug for MyStruct {
            fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result { write!(f, \"hello: {}\", self.field) }
        }
    ";

    assert_eq!(engine.eval::<String>(&format!("{types} let x = MyStruct {{ field: 42 }}; x.to_string()")).unwrap(), "hello: 42");

    assert_eq!(
        engine
            .eval::<String>(&format!(
                r#"{types}
                    let x = (123, true, (), "world", MyStruct {{ field: 42 }});
                    format!("{{x:?}}")
                "#
            ))
            .unwrap(),
        r#"(123, true, (), "world", hello: 42)"#
    );

    assert_eq!(
        engine
            .eval::<String>(&format!(
                r#"{types}
                    let x = vec![MyStruct {{ field: 42 }}, MyStruct {{ field: 7 }}];
                    format!("{{x:?}}")
                "#
            ))
            .unwrap(),
        "[hello: 42, hello: 7]"
    );
}
