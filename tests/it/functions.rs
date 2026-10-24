#![cfg(not(feature = "no_function"))]
use rawr::resolve::Category;
use rawr::{Dynamic, Engine, FuncRegistration, INT, Module, NativeCallContext, ParseErrorType, Shared};

#[test]
fn test_functions() {
    let engine = Engine::new();

    assert_eq!(engine.eval::<INT>("fn add_me(a: i64, b: i64) -> i64 { a + b } add_me(3, 4)").unwrap(), 7);
    assert_eq!(engine.eval::<INT>("fn add_me(a: i64, b: i64,) -> i64 { a + b } add_me(3, 4,)").unwrap(), 7);
    assert_eq!(engine.eval::<INT>("fn bob() -> i64 { return 4; 5 } bob()").unwrap(), 4);
    assert_eq!(engine.eval::<INT>("fn add(x: i64, n: i64) -> i64 { x + n } add(40, 2)").unwrap(), 42);
    assert_eq!(engine.eval::<INT>("fn add(x: i64, n: i64,) -> i64 { x + n } add(40, 2,)").unwrap(), 42);
    assert_eq!(engine.eval::<INT>("fn add(x: i64, n: i64) -> i64 { x + n } let a = 40; add(a, 2); a").unwrap(), 40);
    assert_eq!(engine.eval::<INT>("fn mul2(x: i64) -> i64 { x * 2 } mul2(21)").unwrap(), 42);

    // An argument is passed by value, so the caller's variable keeps its value.
    assert_eq!(engine.eval::<INT>("fn mul2(mut x: i64) -> i64 { x *= 2; x } let a = 21; mul2(a); a").unwrap(), 21);

    // Methods on a value come from a trait implemented for its type.
    #[cfg(not(feature = "no_object"))]
    {
        assert_eq!(
            engine
                .eval::<INT>("trait Add { fn add(&self, n: i64) -> i64; } impl Add for i64 { fn add(&self, n: i64) -> i64 { *self + n } } let x = 40; x.add(2)")
                .unwrap(),
            42
        );
        assert_eq!(
            engine
                .eval::<INT>(
                    "trait AddIn { fn add_in(&mut self, n: i64); } impl AddIn for i64 { fn add_in(&mut self, n: i64) { *self += n; } } let mut x = 40; x.add_in(2); x"
                )
                .unwrap(),
            42
        );
        assert_eq!(
            engine
                .eval::<INT>("trait Mul2 { fn mul2(&self) -> i64; } impl Mul2 for i64 { fn mul2(&self) -> i64 { *self * 2 } } let x = 21; x.mul2()")
                .unwrap(),
            42
        );
        assert_eq!(
            engine
                .eval::<INT>("trait Twice { fn twice(&mut self); } impl Twice for i64 { fn twice(&mut self) { *self *= 2; } } let mut x = 21; x.twice(); x")
                .unwrap(),
            42
        );
    }

    let _ = engine.eval::<INT>("fn/*\0„").unwrap_err();
}

#[test]
fn test_functions_dynamic() {
    let mut engine = Engine::new();

    engine.register_fn(
        "foo",
        |a: INT, b: Dynamic, c: INT, d: INT, e: INT, f: INT, g: INT, h: INT, i: INT, j: INT, k: INT, l: INT, m: INT, n: INT, o: INT, p: INT, q: INT, r: INT, s: INT, t: INT| match b.try_cast::<bool>() {
            Some(true) => a + c + d + e + f + g + h + i + j + k + l + m + n + o + p + q + r + s + t,
            Some(false) => 0,
            None => 42,
        },
    );

    assert_eq!(engine.eval::<INT>("foo(1, true, 3,4,5,6,7,8,9,10,11,12,13,14,15,16,17,18,19,20)").unwrap(), 208);
    assert_eq!(engine.eval::<INT>("foo(1, false, 3,4,5,6,7,8,9,10,11,12,13,14,15,16,17,18,19,20)").unwrap(), 0);
    assert_eq!(engine.eval::<INT>("foo(1, 2, 3,4,5,6,7,8,9,10,11,12,13,14,15,16,17,18,19,20)").unwrap(), 42);
}

#[cfg(not(feature = "no_object"))]
#[test]
fn test_functions_trait_object() {
    trait TestTrait {
        fn greet(&self) -> INT;
    }

    #[allow(clippy::upper_case_acronyms)]
    #[derive(Debug, Clone)]
    struct ABC(INT);

    impl TestTrait for ABC {
        fn greet(&self) -> INT {
            self.0
        }
    }

    #[cfg(not(feature = "sync"))]
    type MySharedTestTrait = Shared<dyn TestTrait>;

    #[cfg(feature = "sync")]
    type MySharedTestTrait = Shared<dyn TestTrait + Send + Sync>;

    let mut engine = Engine::new();

    engine
        .register_type_with_name::<MySharedTestTrait>("MySharedTestTrait")
        .register_fn("new_ts", || Shared::new(ABC(42)) as MySharedTestTrait)
        .register_fn("greet", |x: MySharedTestTrait| x.greet());

    let value = engine.eval::<Dynamic>("new_ts()").unwrap();
    assert_eq!(engine.map_type_name(value.type_name()), "MySharedTestTrait");
    assert_eq!(engine.eval::<INT>("let x = new_ts(); greet(x)").unwrap(), 42);
}

#[test]
fn test_functions_namespaces() {
    let mut engine = Engine::new();

    #[cfg(not(feature = "no_module"))]
    {
        let mut m = Module::new();

        let f = || 999 as INT;
        FuncRegistration::new("test").in_global_namespace().set_into_module(&mut m, f);

        engine.register_static_module("hello", m.into());

        // A function the script defines wins over a registered one of the same name.
        assert_eq!(engine.eval::<INT>("fn test() -> i64 { 123 } test()").unwrap(), 123);
    }

    engine.register_fn("test", || 42 as INT);

    assert_eq!(engine.eval::<INT>("fn test() -> i64 { 123 } test()").unwrap(), 123);
    assert_eq!(engine.eval::<INT>("test()").unwrap(), 42);
}

#[test]
fn test_functions_constants() {
    let mut engine = Engine::new();

    // A function reads a constant declared at the top of the script.
    assert_eq!(engine.eval::<INT>("const ANSWER: i64 = 42; fn foo() -> i64 { ANSWER } foo()").unwrap(), 42);

    // A constant declared inside a block is not visible to a function.
    let error = engine.compile("fn foo() -> i64 { ANSWER } { const ANSWER: i64 = 42; foo() }").unwrap_err();
    assert!(matches!(error.err_type(), ParseErrorType::Resolution(e) if e.category() == Category::UnknownVariable), "{error:?}");

    engine.register_fn("do_stuff", |context: NativeCallContext, callback: rawr::FnPtr| -> Result<INT, _> { callback.call_within_context(&context, ()) });

    // A closure reads the script's constants and its own.
    #[cfg(not(feature = "no_closure"))]
    {
        assert_eq!(engine.eval::<INT>("do_stuff(|| { const LOCAL_VALUE: i64 = 42; LOCAL_VALUE })").unwrap(), 42);
        assert_eq!(engine.eval::<INT>("const GLOBAL_VALUE: i64 = 42; do_stuff(|| GLOBAL_VALUE)").unwrap(), 42);
    }
}

#[test]
fn test_functions_big() {
    let engine = Engine::new();

    assert_eq!(
        engine
            .eval::<INT>(
                "
                    fn math_me(a: i64, b: i64, c: i64, d: i64, e: i64, f: i64) -> i64 {
                        a - b * c + d * e - f
                    }
                    math_me(100, 5, 2, 9, 6, 32)
                ",
            )
            .unwrap(),
        112
    );
}

#[test]
fn test_functions_duplicated_definition() {
    let engine = Engine::new();

    let error = engine
        .compile(
            "
                fn abc(x: i64) -> i64 { x + 42 }
                fn abc(x: i64) -> i64 { x - 42 }
            ",
        )
        .unwrap_err();
    assert!(matches!(error.err_type(), ParseErrorType::Resolution(e) if e.category() == Category::DuplicateItem), "{error:?}");

    // A different arity is still the same name.
    let error = engine.compile("fn abc() -> i64 { 42 } fn abc(x: i64) -> i64 { x }").unwrap_err();
    assert!(matches!(error.err_type(), ParseErrorType::Resolution(e) if e.category() == Category::DuplicateItem), "{error:?}");
}

#[test]
fn test_functions_params() {
    let engine = Engine::new();

    // Expect duplicated parameters error
    let error = engine.compile("fn hello(x: i64, x: i64) -> i64 { x }").unwrap_err();
    assert!(matches!(error.err_type(), ParseErrorType::Resolution(e) if e.category() == Category::DuplicateMember), "{error:?}");
}

#[test]
fn test_function_pointers() {
    let engine = Engine::new();

    assert_eq!(engine.eval::<INT>("fn foo(x: i64) -> i64 { 40 + x } let f = foo; f(2)").unwrap(), 42);
    assert_eq!(engine.eval::<INT>("fn foo(x: i64) -> i64 { 40 + x } let f: fn(i64) -> i64 = foo; f(2)").unwrap(), 42);

    // A name that is not a function is an unknown variable.
    let error = engine.compile("let f = abc; f(0)").unwrap_err();
    assert!(matches!(error.err_type(), ParseErrorType::Resolution(e) if e.category() == Category::UnknownVariable), "{error:?}");

    // A struct field holds a function.
    #[cfg(not(feature = "no_object"))]
    assert_eq!(
        engine
            .eval::<INT>(
                "
                    fn foo(x: i64) -> i64 { 40 + x }

                    struct Actions { action: fn(i64) -> i64 }

                    let x = Actions { action: foo };
                    (x.action)(2)
                "
            )
            .unwrap(),
        42
    );
}

#[test]
#[cfg(not(feature = "unchecked"))]
fn test_functions_max() {
    let mut engine = Engine::new();
    engine.set_max_functions(5);

    engine
        .compile(
            "
            fn foo1() {}
            fn foo2() {}
            fn foo3() {}
            fn foo4() {}
            fn foo5() {}
        ",
        )
        .unwrap();

    assert!(matches!(
        engine
            .compile(
                "
                fn foo1() {}
                fn foo2() {}
                fn foo3() {}
                fn foo4() {}
                fn foo5() {}
                fn foo6() {}
            "
            )
            .expect_err("should err")
            .err_type(),
        ParseErrorType::TooManyFunctions
    ))
}
