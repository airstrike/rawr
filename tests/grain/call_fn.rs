//! Calling one compiled function, with the environment rawr gives it.
//!
//! A program's library, source and module resolver used to be installed around
//! its *main chunk* only, so a function reached through `call_fn` ran without
//! them. That is not a corner: the typed lowering leaves compiler-owned
//! support in the library — a tuple, a string slice, a field of a struct — and
//! rawr finds it only in `global.lib`.

use rawr::grain::format::WriteError;
use rawr::grain::{Compiler, Vm};
use rawr::{CallFnOptions, Dynamic, Engine, EvalAltResult, INT, Scope};

/// `max` and `min` on an `i64` are compiler-owned support, which is the part
/// of the program only the library can answer.
const PROGRAM: &str = "fn outer(n: i64) -> i64 { n.max(3) * 10 + n.min(3) }";

fn walked(engine: &Engine, source: &str) -> INT {
    let ast = engine.compile(source).unwrap();
    engine.call_fn(&mut Scope::new(), &ast, "outer", (7 as INT,)).unwrap()
}

fn run(engine: &Engine, source: &str) -> INT {
    let ast = engine.compile(source).unwrap();
    let program = Compiler::new().compile(&ast);
    // The premise: the program really does lean on support the compiler left
    // to the engine, so the library has to be installed for the call to
    // resolve at all. Refusing to write for *that* reason is the public way to
    // see it.
    assert!(matches!(program.write(), Err(WriteError::HasCompilerSupport)), "{source:?} no longer needs compiler support, so this would prove nothing (write said {:?})", program.write(),);
    Vm::new(engine).call_fn(&mut Scope::new(), &program, "outer", (7 as INT,)).unwrap()
}

#[test]
fn a_call_reaches_support_the_compiler_left_to_the_engine() {
    let engine = Engine::new();
    assert_eq!(walked(&engine, PROGRAM), 73);
    assert_eq!(run(&engine, PROGRAM), walked(&engine, PROGRAM));
}

/// The environment is now installed once around both halves of the call, so the
/// half that used to have it must not have lost it.
#[test]
fn the_main_chunk_still_runs_before_the_call() {
    let engine = Engine::new();
    let ast = engine.compile("fn outer() -> i64 { 1 } let started: i64 = 9;").unwrap();
    let program = Compiler::new().compile(&ast);

    let mut scope = Scope::new();
    let value: INT = Vm::new(&engine)
        .call_fn_with_options(CallFnOptions::new().rewind_scope(false), &mut scope, &program, "outer", ())
        .unwrap();

    assert_eq!(value, 1);
    // What the main chunk declared is left in the caller's scope, as
    // `Engine::eval_ast_with_scope` would leave it.
    assert_eq!(scope.get_value::<INT>("started"), Some(9));
}

#[test]
fn eval_ast_off_skips_the_main_chunk() {
    let engine = Engine::new();
    let ast = engine.compile("fn outer() -> i64 { 1 } let started: i64 = 9;").unwrap();
    let program = Compiler::new().compile(&ast);

    let mut scope = Scope::new();
    let _: INT = Vm::new(&engine)
        .call_fn_with_options(CallFnOptions::new().eval_ast(false).rewind_scope(false), &mut scope, &program, "outer", ())
        .unwrap();

    assert!(scope.get_value::<INT>("started").is_none());
}

#[test]
fn an_error_inside_a_call_carries_the_program_source() {
    let engine = Engine::new();
    let mut ast = engine.compile(r#"fn outer() -> i64 { panic!("boom") }"#).unwrap();
    ast.set_source("handlers.rs");
    let program = Compiler::new().compile(&ast);

    let err = *Vm::new(&engine).call_fn::<Dynamic>(&mut Scope::new(), &program, "outer", ()).unwrap_err();

    match err {
        EvalAltResult::ErrorInFunctionCall(name, source, ..) => {
            assert_eq!(name, "outer");
            assert_eq!(source, "handlers.rs");
        }
        other => panic!("expected a wrapped call error, got {other:?}"),
    }
}

/// A program of plain functions is a set of chunks, so it is an artifact.
#[test]
fn a_program_of_handlers_is_writable() {
    let engine = Engine::new();
    let ast = engine.compile("fn on_open(count: i64, n: i64) -> i64 { count + n } fn on_close(count: i64) -> i64 { count * 0 }").unwrap();
    let program = Compiler::new().compile(&ast);

    assert_eq!(program.residual_count(), 0);
    assert_eq!(program.functions().len(), 2);
    assert!(program.write().is_ok(), "got {:?}", program.write());
}
