#![cfg(not(feature = "no_object"))]
#[cfg(feature = "internals")]
use rawr::{ASTNode, Expr};
use rawr::{Engine, Exports, INT};

#[derive(Debug, Clone, Eq, PartialEq)]
#[rawr::export]
pub struct TestStruct {
    pub x: i64,
}

#[rawr::export]
impl TestStruct {
    pub fn update(&mut self, n: i64) {
        self.x += n;
    }

    pub fn new() -> Self {
        Self { x: 1 }
    }

    pub fn range(&self, _from: i64, _to: i64) -> Self {
        Self::new()
    }
}

fn engine() -> Engine {
    let mut exports = Exports::new("test");
    exports.add::<TestStruct>();
    let mut engine = Engine::new();
    engine.export(exports).unwrap();
    engine
}

#[test]
fn test_method_call() {
    let engine = engine();

    assert_eq!(engine.eval::<TestStruct>("use test::TestStruct; let mut x = TestStruct::new(); x.update(1000); x").unwrap(), TestStruct { x: 1001 });
}

#[test]
fn test_method_call_style() {
    let engine = Engine::new();

    assert_eq!(engine.eval::<INT>("let x: i64 = -123; x.abs(); x").unwrap(), -123);
}

#[cfg(not(feature = "no_optimize"))]
#[test]
fn test_method_call_with_full_optimization() {
    let mut engine = engine();

    engine.set_optimization_level(rawr::OptimizationLevel::Full);

    engine.register_fn("ymd", |_: INT, _: INT, _: INT| 42 as INT);

    assert_eq!(
        engine
            .eval::<TestStruct>(
                "
                    use test::TestStruct;
                    let xs = TestStruct::new();
                    let ys = xs.range(ymd(2022, 2, 1), ymd(2022, 2, 2));
                    ys
                "
            )
            .unwrap(),
        TestStruct::new()
    );
}

/// AST walk tests — verify that `walk` visits arguments inside method calls.

#[cfg(feature = "internals")]
fn walk_vars(engine: &Engine, src: &str) -> Vec<String> {
    let ast = engine.compile(src).unwrap();
    let mut vars = Vec::new();
    ast.walk(&mut |nodes: &[ASTNode]| {
        if let Some(ASTNode::Expr(Expr::Variable(info, _, _))) = nodes.last() {
            vars.push(info.1.to_string());
        }
        true
    });
    vars
}

#[test]
#[cfg(feature = "internals")]
fn test_method_call_walk_visits_args() {
    let engine = Engine::new();
    // `my_array.contains(&value)`: `value` is an argument of the method call.
    let vars = walk_vars(&engine, "fn check(my_array: Vec<i64>, value: i64) -> bool { my_array.contains(&value) }");

    assert!(vars.contains(&"my_array".to_string()), "walk should visit the receiver `my_array`");
    assert!(vars.contains(&"value".to_string()), "walk should visit the argument `value`");
}

#[test]
#[cfg(feature = "internals")]
fn test_method_call_walk_visits_multiple_args() {
    let engine = Engine::new();
    // Three variable arguments — all must be visited.
    let vars = walk_vars(
        &engine,
        "
            struct Obj;
            impl Obj {
                fn foo(&self, p: i64, q: i64, r: i64) -> i64 { p + q + r }
            }
            fn check(obj: Obj, a: i64, b: i64, c: i64) -> i64 { obj.foo(a, b, c) }
        ",
    );

    for name in &["obj", "a", "b", "c"] {
        assert!(vars.contains(&name.to_string()), "walk should visit `{name}`", name = name);
    }
}

#[test]
#[cfg(feature = "internals")]
fn test_method_call_walk_visits_nested_expr_in_arg() {
    let engine = Engine::new();
    // The argument itself contains a variable (`n`) inside an expression.
    let vars = walk_vars(
        &engine,
        "
            struct Obj;
            impl Obj {
                fn foo(&self, k: i64) -> i64 { k }
            }
            fn check(obj: Obj, n: i64) -> i64 { obj.foo(n + 1) }
        ",
    );

    assert!(vars.contains(&"obj".to_string()), "walk should visit the receiver `obj`");
    assert!(vars.contains(&"n".to_string()), "walk should visit `n` nested inside the arg expression");
}

#[test]
#[cfg(feature = "internals")]
fn test_method_call_walk_count_visits_matches_fn_call() {
    // `obj.foo(x)` (method-call syntax) and `foo(obj, x)` (free-function syntax)
    // should both surface the same two variable names via `walk`.
    let engine = Engine::new();

    let method_vars = walk_vars(
        &engine,
        "
            struct Obj;
            impl Obj {
                fn foo(&self, k: i64) -> i64 { k }
            }
            fn check(obj: Obj, x: i64) -> i64 { obj.foo(x) }
        ",
    );
    let fn_vars = walk_vars(
        &engine,
        "
            struct Obj;
            fn foo(_o: Obj, k: i64) -> i64 { k }
            fn check(obj: Obj, x: i64) -> i64 { foo(obj, x) }
        ",
    );

    // Both forms must surface `obj` and `x`.
    for name in &["obj", "x"] {
        assert!(method_vars.contains(&name.to_string()), "method syntax: walk should visit `{name}`", name = name);
        assert!(fn_vars.contains(&name.to_string()), "free-fn syntax: walk should visit `{name}`", name = name);
    }
}
