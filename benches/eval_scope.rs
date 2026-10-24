#![feature(test)]

///! Test evaluating with scope
extern crate test;

use rawr::{Engine, OptimizationLevel, Scope, INT};
use test::Bencher;

#[bench]
fn bench_eval_scope_single(bench: &mut Bencher) {
    let script = "requests_made == requests_made";

    let mut engine = Engine::new();
    engine.set_optimization_level(OptimizationLevel::None);

    let mut scope = Scope::new();
    scope.push("requests_made", 99 as INT);

    let ast = engine.compile_expression_with_scope(&scope, script).unwrap();

    bench.iter(|| engine.run_ast_with_scope(&mut scope, &ast).unwrap());
}

#[bench]
fn bench_eval_scope_multiple(bench: &mut Bencher) {
    let script = "requests_made > requests_succeeded";

    let mut engine = Engine::new();
    engine.set_optimization_level(OptimizationLevel::None);

    let mut scope = Scope::new();
    scope.push("requests_made", 99 as INT);
    scope.push("requests_succeeded", 90 as INT);

    let ast = engine.compile_expression_with_scope(&scope, script).unwrap();

    bench.iter(|| engine.run_ast_with_scope(&mut scope, &ast).unwrap());
}

#[bench]
fn bench_eval_scope_longer(bench: &mut Bencher) {
    let script = "(requests_made * requests_succeeded / 100) >= 90";

    let mut engine = Engine::new();
    engine.set_optimization_level(OptimizationLevel::None);

    let mut scope = Scope::new();
    scope.push("requests_made", 99 as INT);
    scope.push("requests_succeeded", 90 as INT);

    let ast = engine.compile_expression_with_scope(&scope, script).unwrap();

    bench.iter(|| engine.run_ast_with_scope(&mut scope, &ast).unwrap());
}

#[bench]
fn bench_eval_scope_complex(bench: &mut Bencher) {
    let script = r#"
            2 > 1 &&
            "something" != "nothing" ||
            "2014-01-20" < "Wed Jul  8 23:07:35 MDT 2015" &&
            other_name <= variable_name &&
            modifier_test + 1000 / 2 > (80 * 100 % 2)
        "#;

    let mut engine = Engine::new();
    engine.set_optimization_level(OptimizationLevel::None);

    let mut scope = Scope::new();
    scope.push("other_name", 99 as INT);
    scope.push("variable_name", 90 as INT);
    scope.push("modifier_test", 5 as INT);

    let ast = engine.compile_expression_with_scope(&scope, script).unwrap();

    bench.iter(|| engine.run_ast_with_scope(&mut scope, &ast).unwrap());
}
