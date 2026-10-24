#![feature(test)]

///! Test evaluating expressions
extern crate test;

use rawr::{Engine, OptimizationLevel, Scope, INT};
use test::Bencher;

#[derive(Debug, Clone)]
struct Test {
    x: INT,
}

impl Test {
    pub fn action(&mut self) {
        self.x = 0;
    }
    pub fn update(&mut self, val: INT) {
        self.x = val;
    }
}

#[bench]
fn bench_type_method(bench: &mut Bencher) {
    // A script reaches a host type through the functions registered for it,
    // and a `&mut` first parameter takes the variable by reference.
    let script = "action(foo)";

    let mut engine = Engine::new();
    engine.set_optimization_level(OptimizationLevel::None);

    engine.register_type_with_name::<Test>("Test");
    engine.register_fn("action", Test::action);

    let mut scope = Scope::new();
    scope.push("foo", Test { x: 42 });

    let ast = engine.compile_expression_with_scope(&scope, script).unwrap();

    bench.iter(|| engine.run_ast_with_scope(&mut scope, &ast).unwrap());
}

#[bench]
fn bench_type_method_with_params(bench: &mut Bencher) {
    let script = "update(foo, 1)";

    let mut engine = Engine::new();
    engine.set_optimization_level(OptimizationLevel::None);

    engine.register_type_with_name::<Test>("Test");
    engine.register_fn("update", Test::update);

    let mut scope = Scope::new();
    scope.push("foo", Test { x: 42 });

    let ast = engine.compile_expression_with_scope(&scope, script).unwrap();

    bench.iter(|| engine.run_ast_with_scope(&mut scope, &ast).unwrap());
}
