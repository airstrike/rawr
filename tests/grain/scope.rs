//! Variables the caller already had, which no slot can name.
//!
//! Slots are indices into the caller's `Scope` counted from a base taken when
//! the program starts, so everything the host put there beforehand sits below
//! every slot. Those reads and writes go by name instead, and this is where
//! that is held to rawr's behaviour.
//!
//! A typed script is checked against the scope it will run in, so every
//! program here is compiled with that scope: an `i64` or a `String` the host
//! pushed is a typed local to the script, and anything else is a `<dynamic>`
//! value only a registered function can use.
//!
//! It matters more than the node count suggests: a script that reads anything
//! its host supplied used to be a fragment, and a program with a fragment in
//! it cannot be written to an artifact at all.

// Only the engine is wanted here; the corpus scripts belong to the harnesses
// that run all of them.
use super::corpus;

use rawr::grain::{Compiler, Vm};
use rawr::{Dynamic, Engine, INT, Scope};

/// What a run produced, in a form two runs can be compared on.
#[derive(Debug, PartialEq, Eq)]
struct Outcome {
    result: Result<String, String>,
    scope: Vec<(String, String)>,
}

fn capture(scope: &Scope, result: Result<Dynamic, Box<rawr::EvalAltResult>>) -> Outcome {
    Outcome {
        result: result.map(|value| format!("{value:?}")).map_err(|err| format!("{err:?}")),
        scope: scope.iter_raw().map(|(name, _, value)| (name.to_string(), format!("{value:?}"))).collect(),
    }
}

/// Run `source` under rawr and under the VM, from the same starting scope, and
/// require they agree on the value, the error and what the scope holds after.
///
/// `lowered` is the point of the exercise rather than a detail: a program that
/// still fragments cannot cross a wire, so a case that passes while
/// fragmenting has proved nothing about the feature.
#[track_caller]
fn agree(source: &str, build: impl Fn(&mut Scope), lowered: bool) {
    agree_with(&corpus::engine(), source, build, lowered);
}

/// The same, against an engine the caller has set up.
#[track_caller]
fn agree_with(engine: &Engine, source: &str, build: impl Fn(&mut Scope), lowered: bool) {
    let mut checked = Scope::new();
    build(&mut checked);
    let ast = engine.compile_with_scope(&checked, source).expect("must compile");
    let program = Compiler::new().compile(&ast);

    assert_eq!(program.residual_count() == 0, lowered, "{source:?} fragments: {:?}", program.first_unsupported(),);

    let mut walked = Scope::new();
    build(&mut walked);
    let expected = capture(&walked.clone(), engine.eval_ast_with_scope::<Dynamic>(&mut walked, &ast));
    let expected = Outcome {
        scope: capture(&walked, Ok(Dynamic::UNIT)).scope,
        ..expected
    };

    let mut run = Scope::new();
    build(&mut run);
    let actual = {
        let result = Vm::new(engine).eval_with_scope(&mut run, &program);
        capture(&run, result)
    };

    assert_eq!(actual, expected, "{source:?}");
}

#[test]
fn a_caller_variable_can_be_read() {
    agree(
        "brightness * 2",
        |s| {
            s.push("brightness", 21 as INT);
        },
        true,
    );
    agree(
        "mode.clone() + \"!\"",
        |s| {
            s.push("mode", "chase".to_string());
        },
        true,
    );
}

#[test]
fn a_caller_variable_can_be_written() {
    agree(
        "brightness = 7; brightness",
        |s| {
            s.push("brightness", 1 as INT);
        },
        true,
    );
    agree(
        "brightness += 5; brightness",
        |s| {
            s.push("brightness", 1 as INT);
        },
        true,
    );
    agree(
        "mode += \"!\"; mode",
        |s| {
            s.push("mode", "go".to_string());
        },
        true,
    );
}

/// A registered function's `&mut` first parameter reaches a caller's variable
/// too.
///
/// A local is addressed by slot and one the caller supplied is not, but the
/// rule is the same for both — which is the point, since whether a script's
/// variable is a local is not something the script says.
#[test]
fn a_caller_variable_in_first_argument_position_is_taken_by_reference() {
    agree(
        "bump(w); level_of(w)",
        |s| {
            s.push("w", corpus::Widget::default());
        },
        true,
    );
    // And a failure after the mutation has already landed.
    agree(
        "bump_then_fail(w)",
        |s| {
            s.push("w", corpus::Widget::default());
        },
        true,
    );
}

/// A chain rooted at a caller's variable, which is the last root shape that
/// did not lower.
#[test]
fn a_chain_can_be_rooted_at_a_caller_variable() {
    let text = |s: &mut Scope| {
        s.push("mode", "chase".to_string());
    };

    // Read, mutate through a method, and fail inside one.
    agree("mode.len()", text, true);
    agree("mode.push('!'); mode", text, true);
    agree("mode.push_str(\"!!\"); mode.len()", text, true);
    agree("mode.remove(99)", text, true);

    // A constant can be read through a chain too.
    agree(
        "mode.len()",
        |s| {
            s.push_constant("mode", "chase".to_string());
        },
        true,
    );
}

/// A closure can capture a variable the caller supplied.
///
/// Sharing used to find the entry by walking `Scope::iter_raw`, which runs from
/// the top down, and used the position as if it counted from the bottom — so
/// with more than one caller entry it shared the wrong variable outright.
#[test]
fn a_closure_can_capture_a_caller_variable() {
    let seed = |scope: &mut Scope| {
        // Two of them, and the interesting one is not last: the index bug is
        // invisible with a single entry.
        scope.push("first", 7 as INT);
        scope.push("second", 1 as INT);
    };

    agree("let mut n: i64 = 0; { let f = |x: i64| x + first; n = f(100); } n", seed, true);
    agree("let mut n: i64 = 0; { let f = |x: i64| x + second; n = f(100); } n", seed, true);
}

/// A local of the same name hides the caller's and must not write through to
/// it — the caller's entry has to come back untouched.
#[test]
fn a_local_shadows_the_caller_without_disturbing_it() {
    agree(
        "let mut brightness: i64 = 1; brightness += 1; brightness",
        |s| {
            s.push("brightness", 100 as INT);
        },
        true,
    );
    // And the other order: read before the local exists, so the same name is
    // two different variables in one script.
    agree(
        "let first = brightness; let brightness: i64 = 1; vec![first, brightness]",
        |s| {
            s.push("brightness", 100 as INT);
        },
        true,
    );
}

/// A constant the caller pushed is read like any other variable, and the
/// checker refuses an assignment to it before anything runs.
#[test]
fn a_caller_constant_can_be_read_but_not_assigned() {
    let constant = |s: &mut Scope| {
        s.push_constant("mode", 1 as INT);
    };

    agree("mode + 1", constant, true);

    let engine = corpus::engine();
    let mut scope = Scope::new();
    constant(&mut scope);
    for source in ["mode = 2; mode", "mode += 1; mode"] {
        let err = engine.compile_with_scope(&scope, source).expect_err("a constant is not assignable");
        assert!(err.to_string().contains("immutable"), "{source:?}: {err}");
    }
}

/// A bare script-function name is a function pointer, not a variable read, so
/// it must not become a name lookup that fails.
#[test]
fn a_script_function_name_is_not_a_variable() {
    let engine = corpus::engine();
    let ast = engine.compile("fn helper() -> i64 { 1 } let f = helper; f()").expect("must compile");
    let program = Compiler::new().compile(&ast);

    assert!(
        !rawr::grain::bytecode::disassemble(program.code()).any(|(.., op)| matches!(op, rawr::grain::bytecode::Op::LoadNamed(..))),
        "nothing in {:?} may load `helper` by name",
        program,
    );
}

/// The one place a compiled closure is not the walker's closure.
///
/// Rawr's parser builds a pointer that embeds the closure's `ScriptFuncDef` —
/// the AST body — and tags it with the environment it was written in. That is
/// exactly what an artifact must not carry, so the compiler emits a
/// name-only pointer to the chunk it compiled from that same body.
///
/// Everything the closure *does* is identical; what differs is that ours is
/// late-bound, which rawr renders. Pinned here rather than left to be
/// discovered, because it is visible to a script that prints one.
#[test]
fn a_closure_pointer_is_late_bound() {
    let engine = corpus::engine();
    let source = "let f = |x: i64| x + 1; f";

    let ast = engine.compile(source).expect("must compile");
    let program = Compiler::new().compile(&ast);
    assert_eq!(program.residual_count(), 0, "the closure must lower");

    let ours = Vm::new(&engine).eval_with_scope(&mut Scope::new(), &program).expect("must run");
    let walker = engine.eval_ast_with_scope::<Dynamic>(&mut Scope::new(), &ast).expect("must run under rawr too");

    let (ours, walker) = (format!("{ours:?}"), format!("{walker:?}"));
    assert!(ours.starts_with("Fn(\"anon$"), "ours is a plain named pointer: {ours}",);
    assert!(walker.starts_with("Fn*+(\"anon$"), "rawr's carries a script body and an environment: {walker}",);

    // And the difference is only in the binding: calling either gives the
    // same answer, which is what the corpus covers.
    assert_ne!(ours, walker, "if these ever match, delete this test");
}

/// Calling a compiled function from outside, which is what a native wrapper
/// does when it calls a compiled chunk back.
#[test]
fn a_compiled_function_can_be_called_by_name() {
    let engine = corpus::engine();
    let ast = engine.compile(r#"fn add(a: i64, b: i64) -> i64 { a + b } fn boom() -> i64 { panic!("7") } 0"#).expect("must compile");
    let program = Compiler::new().compile(&ast);

    let mut vm = Vm::new(&engine);
    let value = vm.call_function(&program, "add", vec![Dynamic::from(2 as INT), Dynamic::from(3 as INT)], 0, rawr::Position::NONE).expect("must call");
    assert_eq!(value.as_int().unwrap(), 5);

    // Wrong arity is a miss, not a crash — the table is keyed on both.
    let err = vm
        .call_function(&program, "add", vec![Dynamic::from(1 as INT)], 0, rawr::Position::NONE)
        .expect_err("one argument is a different function");
    assert!(matches!(*err, rawr::EvalAltResult::ErrorFunctionNotFound(..)));

    // And what the function raises comes back, wrapped as rawr wraps it.
    let err = vm.call_function(&program, "boom", Vec::new(), 0, rawr::Position::NONE).expect_err("must propagate");
    assert!(matches!(*err, rawr::EvalAltResult::ErrorInFunctionCall(..)), "got {err:?}",);

    // The operand stack is where it started, so a caller can keep using it.
    let value = vm.call_function(&program, "add", vec![Dynamic::from(10 as INT), Dynamic::from(1 as INT)], 0, rawr::Position::NONE).expect("must call again");
    assert_eq!(value.as_int().unwrap(), 11);
}

/// The same call through the API a host actually reaches for, which mirrors
/// [`Engine::call_fn`].
///
/// The differences from `call_function` are the ones rawr's own `call_fn` has:
/// arguments as a tuple rather than a `Vec<Dynamic>`, a typed result, and the
/// program's body run first — which is what sets up anything the call needs.
#[test]
fn call_fn_mirrors_the_engines() {
    let engine = corpus::engine();
    let ast = engine.compile("fn add(a: i64, b: i64) -> i64 { a + b } let marker: i64 = 10; 0").expect("must compile");
    let program = Compiler::new().compile(&ast);

    let mut vm = Vm::new(&engine);
    let mut scope = Scope::new();

    let sum: INT = vm.call_fn(&mut scope, &program, "add", (2 as INT, 3 as INT)).expect("must call");
    assert_eq!(sum, 5);
    // `eval_ast` ran the body, and `rewind_scope` took back what it declared.
    // Both default to on, as they do in rawr.
    assert_eq!(scope.len(), 0, "the default rewinds what the body declared");

    let keep = rawr::CallFnOptions::new().rewind_scope(false);
    let sum: INT = vm.call_fn_with_options(keep, &mut scope, &program, "add", (4 as INT, 5 as INT)).expect("must call");
    assert_eq!(sum, 9);
    assert_eq!(scope.len(), 1, "`marker` outlives the call now");
    assert_eq!(scope.get_value::<INT>("marker").unwrap(), 10);

    // Skipping the body is what a caller does when the function needs nothing
    // from it — the common case, and the one that costs nothing.
    let mut fresh = Scope::new();
    let skip = rawr::CallFnOptions::new().eval_ast(false);
    let sum: INT = vm.call_fn_with_options(skip, &mut fresh, &program, "add", (1 as INT, 1 as INT)).expect("must call");
    assert_eq!(sum, 2);
    assert_eq!(fresh.len(), 0, "the body never ran, so it declared nothing");

    // A result that is not the type asked for is an error, not a panic.
    let err = vm.call_fn::<String>(&mut scope, &program, "add", (1 as INT, 2 as INT)).expect_err("an INT is not a String");
    assert!(matches!(*err, rawr::EvalAltResult::ErrorMismatchOutputType(..)), "got {err:?}",);

    // And a missing name still misses.
    let err = vm.call_fn::<INT>(&mut scope, &program, "nope", (1 as INT,)).expect_err("no such function");
    assert!(matches!(*err, rawr::EvalAltResult::ErrorFunctionNotFound(..)), "got {err:?}",);
}

/// The flag a host uses to decide whether a program has to be owned.
///
/// Registering compiled functions so a native can call one back requires a
/// `'static` wrapper, which means owning the program and giving up the
/// borrowed-from-the-artifact loading. Nobody should have to read a script to
/// find out whether that is needed — and the answer must be the same for a
/// compiled program and for the same program read back, or one of the two
/// paths quietly loses its callbacks.
#[test]
fn the_compiler_says_whether_a_program_makes_function_pointers() {
    let engine = corpus::engine();

    for (source, expected) in [
        ("let a: i64 = 1; a + 2", false),
        ("fn f(x: i64) -> i64 { x } f(1)", false),
        ("let mut s: i64 = 0; for i in 0..3 { s += i; } s", false),
        // Every shape that produces one.
        ("let f = |x: i64| x + 1; f(2)", true),
        ("fn t(x: i64) -> i64 { x } let f = t; f(1)", true),
        ("fn t(x: i64) -> i64 { x } fn apply(f: fn(i64) -> i64) -> i64 { f(1) } apply(t)", true),
        ("let a = vec![1, 2]; a.iter().map(|x| x + 1).collect::<Vec<i64>>()", true),
    ] {
        let ast = engine.compile(source).expect("must compile");
        let program = Compiler::new().compile(&ast);
        assert_eq!(program.makes_fn_pointers(), expected, "for {source:?}",);

        // Read off the code, so an artifact answers the same.
        if let Ok(bytes) = program.write() {
            let reloaded = rawr::grain::Program::read(&bytes).expect("must load");
            assert_eq!(reloaded.makes_fn_pointers(), expected, "after a round trip, for {source:?}",);
        }
    }
}

/// The whole reason for the opcode: these now cross a wire.
#[test]
fn a_program_reading_caller_state_can_be_written() {
    let engine = corpus::engine();
    let source = "let mut out = brightness; if mode == \"chase\" { out += 10 } out";

    let mut scope = Scope::new();
    scope.push("brightness", 5 as INT);
    scope.push("mode", "chase".to_string());

    let ast = engine.compile_with_scope(&scope, source).expect("must compile");
    let program = Compiler::new().compile(&ast);
    let bytes = program.write().expect("must be writable");

    let reloaded = rawr::grain::Program::read(&bytes).expect("must load");
    let value = Vm::new(&engine).eval_with_scope(&mut scope, &reloaded).expect("must run");

    assert_eq!(value.as_int().unwrap(), 15);
}
