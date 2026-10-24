use rawr::{Engine, EvalAltResult, INT, ImmutableString, LexError, ParseErrorType, Position, Scope};

#[test]
fn test_string() {
    let engine = Engine::new();

    assert_eq!(engine.eval::<String>(r#""Test string: \u{2764}""#).unwrap(), "Test string: ❤");
    assert_eq!(engine.eval::<String>("\"Test\\rstring: \\u{2764}\"").unwrap(), "Test\rstring: ❤");
    assert_eq!(
        engine.eval::<String>("   \"Test string: \\u{2764}\\\n     hello, world!\"").unwrap(),
        // A `\` at the end of a line skips the line break and the next
        // line's leading whitespace, as in Rust.
        "Test string: ❤hello, world!"
    );
    assert_eq!(engine.eval::<String>(r#""Test string: \x58""#).unwrap(), "Test string: X");
    assert_eq!(engine.eval::<String>(r#""\"hello\"""#).unwrap(), r#""hello""#);
    assert_eq!(engine.eval::<String>(r##"r#"Test"#"##).unwrap(), "Test");
    assert_eq!(engine.eval::<String>(r##"r#"Test string: \\x58\nhello,\nworld!"#"##).unwrap(), r#"Test string: \\x58\nhello,\nworld!"#);
    assert_eq!(engine.eval::<String>(r###"r##"Test string: #"\\x58\nhello,\\nworld!"#"##"###).unwrap(), r##"Test string: #"\\x58\nhello,\\nworld!"#"##);
    assert_eq!(
        engine
            .eval::<String>(
                r###"r##"Test
string: "##.to_string() + "\u{2764}""###
            )
            .unwrap(),
        "Test\nstring: ❤"
    );
    assert_eq!(
        engine
            .eval::<String>(
                r###"r##"Test"
string: "##.to_string() + "\u{2764}""###
            )
            .unwrap(),
        "Test\"\nstring: ❤"
    );
    // A raw string followed by a stray `#` is a syntax error.
    let bad_result = *engine.eval::<String>(r###"r#"Test string: \"##"###).unwrap_err();
    assert!(matches!(bad_result, EvalAltResult::ErrorParsing(..)), "Wrong error type: {bad_result}");
    let bad_result = *engine
        .eval::<String>(
            r###"r##"Test string:
    \"#"###,
        )
        .unwrap_err();
    if let EvalAltResult::ErrorParsing(parse_error, pos, _) = bad_result {
        assert_eq!(parse_error, ParseErrorType::BadInput(LexError::UnterminatedString));
        assert_eq!(pos, Position::new(1, 1));
    } else {
        panic!("Wrong error type: {}", bad_result);
    }

    assert_eq!(engine.eval::<String>(r#""foo".to_string() + "bar""#).unwrap(), "foobar");

    assert!(engine.eval::<bool>(r#"let y = "hello, world!"; y.contains("world")"#).unwrap());
    assert!(engine.eval::<bool>(r#"let y = "hello, world!"; y.contains('w')"#).unwrap());
    assert!(!engine.eval::<bool>(r#"let y = "hello, world!"; y.contains("hey")"#).unwrap());

    assert_eq!(engine.eval::<String>(r#"format!("foo{}", 123)"#).unwrap(), "foo123");
    assert_eq!(engine.eval::<String>(r#""foo".to_string() + &123.to_string()"#).unwrap(), "foo123");

    #[cfg(not(feature = "no_object"))]
    assert_eq!(engine.eval::<String>("42.to_string()").unwrap(), "42");

    #[cfg(not(feature = "no_index"))]
    assert_eq!(engine.eval::<char>(r#"let y = "hello"; y.chars().nth(1).unwrap()"#).unwrap(), 'e');

    #[cfg(not(feature = "no_index"))]
    assert_eq!(engine.eval::<char>(r#"let y = "hello"; y.chars().last().unwrap()"#).unwrap(), 'o');

    #[cfg(not(feature = "no_object"))]
    assert_eq!(engine.eval::<INT>(r#"let y = "hello"; y.len()"#).unwrap(), 5);

    #[cfg(not(feature = "no_object"))]
    assert_eq!(engine.eval::<INT>(r#"let mut y = String::from("hello"); y.clear(); y.len()"#).unwrap(), 0);

    #[cfg(not(feature = "no_object"))]
    #[cfg(not(feature = "no_index"))]
    assert_eq!(engine.eval::<char>(r#"let y = "hello"; y.as_bytes()[y.len() - 1] as char"#).unwrap(), 'o');

    #[cfg(not(feature = "no_float"))]
    assert_eq!(engine.eval::<String>(r#"format!("foo{}", 123.4556)"#).unwrap(), "foo123.4556");
}
#[cfg(not(feature = "no_index"))]
#[test]
fn test_string_index() {
    let engine = Engine::new();

    // A range of a string is a string, not a char.
    assert_eq!(engine.eval::<char>(r#"let y = "hello"; y[1..2].to_string()"#).unwrap_err().to_string(), "Output type incorrect: string (expecting char)");

    // A range of a string is its slice, as in Rust.
    assert_eq!(engine.eval::<String>(r#"let y = "hello"; y[1..3].to_string()"#).unwrap(), "el");
    assert_eq!(engine.eval::<String>(r#"let y = "hello"; y[..].to_string()"#).unwrap(), "hello");
    assert_eq!(engine.eval::<String>(r#"let y = "hello"; y[..2].to_string()"#).unwrap(), "he");
    assert_eq!(engine.eval::<String>(r#"let y = "hello"; y[1..].to_string()"#).unwrap(), "ello");
    assert_eq!(engine.eval::<String>(r#"let y = "hello"; y[1..=3].to_string()"#).unwrap(), "ell");
    assert_eq!(engine.eval::<String>(r#"let y = "hello"; y[..=2].to_string()"#).unwrap(), "hel");

    // A range past the end fails, as Rust's panics.
    let _ = engine.eval::<String>(r#"let y = "hello"; y[0..18].to_string()"#).unwrap_err();
}
#[test]
fn test_string_dynamic() {
    let engine = Engine::new();
    let mut scope = Scope::new();
    scope.push("x", "foo");
    scope.push("y", "foo");
    scope.push("z", "foo");

    assert!(engine.eval_with_scope::<bool>(&mut scope, r#"x == "foo""#).unwrap());
    assert!(engine.eval_with_scope::<bool>(&mut scope, r#"y == "foo""#).unwrap());
    assert!(engine.eval_with_scope::<bool>(&mut scope, r#"z == "foo""#).unwrap());
}

#[test]
fn test_string_mut() {
    let mut engine = Engine::new();

    engine.register_fn("foo", |s: &str| s.len() as INT);
    engine.register_fn("bar", |s: String| s.len() as INT);
    engine.register_fn("baz", |s: &mut String| s.len());

    assert_eq!(engine.eval::<char>(r#"let mut s = String::from("hello"); s.pop().unwrap()"#).unwrap(), 'o');
    assert_eq!(engine.eval::<String>(r#"let mut s = String::from("hello"); s.split_off(2)"#).unwrap(), "llo");
    assert_eq!(engine.eval::<String>(r#"let mut s = String::from("hello"); s.truncate(2); s"#).unwrap(), "he");

    assert_eq!(engine.eval::<INT>(r#"foo("hello")"#).unwrap(), 5);
    assert_eq!(engine.eval::<INT>(r#"bar("hello")"#).unwrap(), 5);
    assert!(matches!(
        *engine.eval::<INT>(r#"baz("hello")"#).unwrap_err(),
        EvalAltResult::ErrorFunctionNotFound(f, ..) if f == "baz (&str | ImmutableString | String)"
    ));
}

#[cfg(not(feature = "no_object"))]
#[test]
fn test_string_substring() {
    let engine = Engine::new();

    // Substrings by character position go through `chars`, as in Rust.
    assert_eq!(engine.eval::<String>(r#"let x = "\u{2764}\u{2764}\u{2764} hello! \u{2764}\u{2764}\u{2764}"; x.chars().skip(1).take(5).collect::<String>()"#).unwrap(), "❤❤ he");
    assert_eq!(engine.eval::<String>(r#"let x = "\u{2764}\u{2764}\u{2764} hello! \u{2764}\u{2764}\u{2764}"; x.chars().skip(1).collect::<String>()"#).unwrap(), "❤❤ hello! ❤❤❤");
    assert_eq!(engine.eval::<String>(r#"let x = "\u{2764}\u{2764}\u{2764} hello! \u{2764}\u{2764}\u{2764}"; x.chars().skip(99).collect::<String>()"#).unwrap(), "");
    assert_eq!(engine.eval::<String>(r#"let x = "\u{2764}\u{2764}\u{2764} hello! \u{2764}\u{2764}\u{2764}"; x.chars().skip(1).take(999).collect::<String>()"#).unwrap(), "❤❤ hello! ❤❤❤");
    assert_eq!(engine.eval::<String>(r#"let x = "\u{2764}\u{2764}\u{2764} hello! \u{2764}\u{2764}\u{2764}"; x.chars().skip(4).take(6).collect::<String>()"#).unwrap(), "hello!");
    assert_eq!(engine.eval::<String>(r#"let x = "hello! \u{2764}\u{2764}\u{2764}"; x.chars().rev().take(2).collect::<String>()"#).unwrap(), "❤❤");
    assert_eq!(engine.eval::<String>(r#"let x = "\u{2764}\u{2764}\u{2764} hello! \u{2764}\u{2764}\u{2764}"; x.replace('l', "")"#).unwrap(), "❤❤❤ heo! ❤❤❤");
    assert_eq!(engine.eval::<String>(r#"let x = "\u{2764}\u{2764}\u{2764} hello! \u{2764}\u{2764}\u{2764}"; x.replace("\u{2764}\u{2764}", "")"#).unwrap(), "❤ hello! ❤");
    assert_eq!(engine.eval::<INT>(r#"let x = "\u{2764}\u{2764}\u{2764} hello! \u{2764}\u{2764}\u{2764}"; x.chars().position(|c| c == '\u{2764}').unwrap()"#).unwrap(), 0);
    assert_eq!(engine.eval::<INT>(r#"let x = "\u{2764}\u{2764}\u{2764} hello! \u{2764}\u{2764}\u{2764}"; x.chars().skip(5).position(|c| c == '\u{2764}').unwrap() + 5"#).unwrap(), 11);
    assert!(engine.eval::<bool>(r#"let x = "\u{2764}\u{2764}\u{2764} hello! \u{2764}\u{2764}\u{2764}"; x.chars().skip(999).position(|c| c == '\u{2764}').is_none()"#).unwrap());
    assert!(engine.eval::<bool>(r#"let x = "\u{2764}\u{2764}\u{2764} hello! \u{2764}\u{2764}\u{2764}"; x.chars().position(|c| c == 'x').is_none()"#).unwrap());
    // `find` gives a byte offset.
    assert_eq!(engine.eval::<INT>(r#"let x = "\u{2764}\u{2764}\u{2764} hello! \u{2764}\u{2764}\u{2764}"; x.find('h').unwrap()"#).unwrap(), 10);
}

#[test]
fn test_string_fn() {
    let mut engine = Engine::new();

    engine
        .register_fn("foo1", |s: &str| s.len() as INT)
        .register_fn("foo2", |s: ImmutableString| s.len() as INT)
        .register_fn("foo3", |s: String| s.len() as INT)
        .register_fn("foo4", |s: &mut ImmutableString| s.len() as INT);

    assert_eq!(engine.eval::<INT>(r#"foo1("hello")"#).unwrap(), 5);
    assert_eq!(engine.eval::<INT>(r#"foo2("hello")"#).unwrap(), 5);
    assert_eq!(engine.eval::<INT>(r#"foo3("hello")"#).unwrap(), 5);
    assert_eq!(engine.eval::<INT>(r#"foo4("hello")"#).unwrap(), 5);
}

#[cfg(not(feature = "no_object"))]
#[cfg(not(feature = "no_index"))]
#[test]
fn test_string_split() {
    let engine = Engine::new();

    // `split` is Rust's iterator of the parts.
    assert_eq!(engine.eval::<INT>(r#"let x = "\u{2764}\u{2764}\u{2764} hello! \u{2764}\u{2764}\u{2764}"; x.split(' ').count()"#).unwrap(), 3);
    assert_eq!(engine.eval::<INT>(r#"let x = "\u{2764}\u{2764}\u{2764} hello! \u{2764}\u{2764}\u{2764}"; x.split("hello").count()"#).unwrap(), 2);

    // Verify that split works on const strings (regression test for issue #1081).
    assert_eq!(engine.eval::<INT>(r#"const X: &str = "a,b,c"; X.split(",").count()"#).unwrap(), 3);
    assert_eq!(engine.eval::<INT>(r#"const X: &str = "a,b,c"; X.splitn(2, ",").count()"#).unwrap(), 2);
    assert_eq!(engine.eval::<INT>(r#"const X: &str = "a,b,c"; X.split(',').count()"#).unwrap(), 3);
    assert_eq!(engine.eval::<INT>(r#"const X: &str = "a,b,c"; X.splitn(2, ',').count()"#).unwrap(), 2);
    assert_eq!(engine.eval::<INT>(r#"const X: &str = "a b c"; X.split_whitespace().count()"#).unwrap(), 3);
}

#[test]
fn test_string_interpolated() {
    let engine = Engine::new();

    assert_eq!(engine.eval::<String>(r#"format!("")"#).unwrap(), "");

    assert_eq!(
        engine
            .eval::<String>(
                r#"
                    let x = 40;
                    format!("hello {} worlds!", x + 2)
                "#
            )
            .unwrap(),
        "hello 42 worlds!"
    );

    // A plain string literal does not interpolate.
    assert_eq!(
        engine
            .eval::<String>(
                r#"
                    let x = 40;
                    "hello ${x+2} worlds!"
                "#
            )
            .unwrap(),
        "hello ${x+2} worlds!"
    );

    assert_eq!(
        engine
            .eval::<String>(
                r#"
                    const X: i64 = 42;
                    format!("hello {} worlds!", X)
                "#
            )
            .unwrap(),
        "hello 42 worlds!"
    );

    assert_eq!(engine.eval::<String>(r#"format!("hello {{world}}")"#).unwrap(), "hello {world}");

    assert_eq!(
        engine
            .eval::<String>(
                r#"
                  let x = 7;
                  format!("literal {{a}} value {x}")
                "#
            )
            .unwrap(),
        "literal {a} value 7"
    );

    assert_eq!(
        engine
            .eval::<String>(
                r#"
                    const X: i64 = 20;
                    format!("hello {} worlds!", { let y = X + 1; format!("{}", y * 2) })
                "#
            )
            .unwrap(),
        "hello 42 worlds!"
    );

    assert_eq!(
        engine
            .eval::<String>(
                r#"
                    let x = 42;
                    let y = 123;
                    let w = {
                        let mut w = format!("{x} world");
                        if x > 1 { w += "s"; }
                        w
                    };

                    format!("\nUndeniable logic:\n1) Hello, {w}!\n2) If {y} > {x} then it is {}!\n", y > x)
                "#
            )
            .unwrap(),
        "\nUndeniable logic:\n1) Hello, 42 worlds!\n2) If 123 > 42 then it is true!\n",
    );
}

#[test]
#[allow(clippy::op_ref)] // Exercise owned/borrowed comparison implementations independently.
fn test_immutable_string() {
    let x: ImmutableString = "hello".into();
    assert_eq!(x, "hello");
    assert!(x == "hello");
    assert!(&x == "hello");
    assert!("hello" == x);
    assert!("hello" == &x);

    let s2 = String::from("hello");
    let s3: ImmutableString = s2.clone().into();
    assert_eq!(s2, s3);
    let s3: ImmutableString = (&s2).into();
    assert_eq!(s2, s3);

    assert!(x == s2);
    assert!(&x == s2);
    assert!(x == &s2);
    assert!(&x == &s2);

    assert!(s2 == x);
    assert!(&s2 == x);
    assert!(s2 == &x);
    assert!(&s2 == &x);

    assert!(x >= s2);
    assert!(&x >= s2);
    assert!(x >= &s2);
    assert!(&x >= &s2);

    assert!(s2 >= x);
    assert!(&s2 >= x);
    assert!(s2 >= &x);
    assert!(&s2 >= &x);

    let _sx: String = x.clone().into();
    let _sx: String = (&x).into();
    let _ssx: Box<str> = x.clone().into();
    let _ssx: Box<str> = (&x).into();
}
