#![cfg(not(feature = "no_function"))]
use rawr::{Engine, FnPtr, INT, Scope};
#[cfg(not(feature = "sync"))]
use rawr::{AST, EvalAltResult};
use std::any::TypeId;
#[cfg(not(feature = "sync"))]
use std::cell::{OnceCell, RefCell};
use std::mem::take;
#[cfg(not(feature = "sync"))]
use std::rc::Rc;

/// A closure that calls another closure is passed to a native, which calls it.
#[test]
#[cfg(not(feature = "no_closure"))]
fn test_closure_call_from_native() {
    let mut engine = Engine::new();

    engine.register_raw_fn("call_with_arg", [TypeId::of::<FnPtr>(), TypeId::of::<INT>()], |context, args| {
        let fn_ptr = args[0].take().cast::<FnPtr>();
        fn_ptr.call_raw(&context, None, [args[1].take()])
    });

    assert_eq!(
        engine
            .eval::<INT>(
                "
                    let addition = |x: i64, y: i64| x + y;
                    let add_two = move |y: i64| addition(2, y);

                    call_with_arg(add_two, 40)
                "
            )
            .unwrap(),
        42
    );
}

#[test]
#[cfg(not(feature = "no_closure"))]
fn test_closures() {
    let mut engine = Engine::new();
    let mut scope = Scope::new();

    scope.push("x", 42 as INT);

    // A closure captures a variable the host put in the scope.
    assert_eq!(
        engine
            .eval_with_scope::<INT>(
                &mut scope,
                "
                    let f = || x;
                    f()
                ",
            )
            .unwrap(),
        42
    );

    // A closure that mutates a captured variable.
    assert_eq!(
        engine
            .eval::<INT>(
                "
                    let mut a = 41;
                    let mut foo = |x: i64| a += x;
                    foo(1);
                    a
                "
            )
            .unwrap(),
        42
    );

    engine.register_fn("plus_one", |x: INT| x + 1);

    assert_eq!(
        engine
            .eval::<INT>(
                "
                    let a = 41;
                    let f = || plus_one(a);
                    f()
                "
            )
            .unwrap(),
        42
    );

    assert_eq!(
        engine
            .eval::<INT>(
                "
                    let a = 40;
                    let f = |x: i64| {
                        let f = |x: i64| {
                            let f = |x: i64| plus_one(a) + x;
                            f(x)
                        };
                        f(x)
                    };
                    f(1)
                "
            )
            .unwrap(),
        42
    );

    engine.register_raw_fn("custom_call", [TypeId::of::<INT>(), TypeId::of::<FnPtr>()], |context, args| {
        let func = take(args[1]).cast::<FnPtr>();

        func.call_raw(&context, None, [])
    });

    assert_eq!(
        engine
            .eval::<INT>(
                "
                    let a = 41;
                    let b = 0;
                    let f = || custom_call(b, || a + 1);

                    f()
                "
            )
            .unwrap(),
        42
    );
}

#[test]
#[cfg(not(feature = "no_closure"))]
fn test_closures_sharing() {
    let mut engine = Engine::new();

    engine.register_fn("foo", |x: INT, s: &str| s.len() as INT + x);
    engine.register_fn("bar", |x: INT, s: String| s.len() as INT + x);

    assert_eq!(
        engine
            .eval::<INT>(
                r#"
                    let s = "hello";
                    let f = || s;
                    foo(1, s)
                "#
            )
            .unwrap(),
        6
    );

    assert_eq!(
        engine
            .eval::<String>(
                r#"
                    let s = "hello";
                    let f = || s;
                    let _n = foo(1, s);
                    s
                "#
            )
            .unwrap(),
        "hello"
    );

    assert_eq!(
        engine
            .eval::<INT>(
                r#"
                    let s = "hello";
                    let f = || s;
                    bar(1, s)
                "#
            )
            .unwrap(),
        6
    );
}

#[test]
#[cfg(not(feature = "no_closure"))]
fn test_closures_external() {
    let engine = Engine::new();

    let ast = engine
        .compile(
            r#"
                let test = String::from("hello");
                move |x: i64| format!("{test}{x}")
            "#,
        )
        .unwrap();

    let fn_ptr = engine.eval_ast::<FnPtr>(&ast).unwrap();

    let f = move |x: INT| fn_ptr.call::<String>(&engine, &ast, (x,));

    assert_eq!(f(42).unwrap(), "hello42");
}

#[test]
#[cfg(not(feature = "no_closure"))]
#[cfg(not(feature = "sync"))]
fn test_closures_callback() {
    type SingleNode = Rc<dyn Node>;

    trait Node {
        fn run(&self, x: INT) -> Result<INT, Box<EvalAltResult>>;
    }

    struct PhaserNode {
        func: Box<dyn Fn(INT) -> Result<INT, Box<EvalAltResult>>>,
    }

    impl Node for PhaserNode {
        fn run(&self, x: INT) -> Result<INT, Box<EvalAltResult>> {
            (self.func)(x)
        }
    }

    fn phaser(callback: impl Fn(INT) -> Result<INT, Box<EvalAltResult>> + 'static) -> impl Node {
        PhaserNode { func: Box::new(callback) }
    }

    let mut engine = Engine::new();

    // The script is compiled after `phaser` is registered, so the check knows
    // the name. The callback reads the compiled script from this cell.
    let ast: Rc<OnceCell<AST>> = Rc::new(OnceCell::new());

    let shared_engine = Rc::new(RefCell::new(Engine::new_raw()));
    let engine2 = shared_engine.clone();
    let ast2 = ast.clone();

    engine.register_fn("phaser", move |fp: FnPtr| {
        let engine = engine2.clone();
        let ast = ast2.clone();

        let callback = Box::new(move |x: INT| fp.call(&engine.borrow(), ast.get().unwrap(), (x,)));

        Rc::new(phaser(callback)) as SingleNode
    });

    let compiled = engine
        .compile(
            "
                const FACTOR: i64 = 2;
                phaser(|x: i64| x * FACTOR)
            ",
        )
        .unwrap();
    let _ = ast.set(compiled);

    *shared_engine.borrow_mut() = engine;

    let cb = shared_engine.borrow().eval_ast::<SingleNode>(ast.get().unwrap()).unwrap();

    assert_eq!(cb.run(21).unwrap(), 42);
}
