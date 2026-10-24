//! Natives that call a compiled function back.
//!
//! `a.retain(|x| *x > 1)` is the shape: `retain` is rawr's, and the pointer it
//! is handed is resolved by rawr's dispatch rather than by ours. This is where
//! the wrappers that make that resolve are held to the walker's behaviour, and
//! where the places it still diverges are pinned rather than left to be
//! discovered.
//!
//! Not every adapter is a native: the typed lowering turns `iter().map(..)`
//! and its relatives into a loop inside the program, which calls the closure
//! itself. Those are here too, as closures reached from compiled code.

// Only the engine is wanted here; the corpus scripts belong to the harnesses
// that run all of them.
use super::corpus;

use rawr::grain::{Compiler, Vm};
use rawr::{Dynamic, Engine, Scope};

/// Run a source through the VM with the callback wrappers installed.
fn run(engine: &Engine, source: &str) -> Result<String, String> {
    let ast = engine.compile(source).map_err(|err| format!("{err:?}"))?;
    let program = Compiler::new().compile(&ast).into_shared();
    Vm::new(engine)
        .eval_with_callbacks(&mut Scope::new(), &program)
        .map(|value| format!("{value:?}"))
        .map_err(|err| format!("{err:?}"))
}

/// The same source through the walker, which is what the answer has to be.
fn walk(engine: &Engine, source: &str) -> Result<String, String> {
    let ast = engine.compile(source).map_err(|err| format!("{err:?}"))?;
    engine.eval_ast::<Dynamic>(&ast).map(|value| format!("{value:?}")).map_err(|err| format!("{err:?}"))
}

fn agree(source: &str) {
    let engine = corpus::engine();
    assert_eq!(walk(&engine, source), run(&engine, source), "{source}");
}

/// The vector is bound to a variable in every case here, and that is not
/// incidental: a chain rooted at a literal can still be a fragment, and a
/// fragment hands the whole expression back to the walker — which would make
/// every one of these pass without the wrappers existing at all.
fn lowered(source: &str) {
    let engine = corpus::engine();
    let ast = engine.compile(source).unwrap();
    let program = Compiler::new().compile(&ast);
    assert!(program.residual_count() == 0, "{source} still fragments, so it does not test the callback path",);
    assert!(program.makes_fn_pointers(), "{source}");
}

#[test]
fn a_native_can_call_a_closure_back() {
    lowered("let mut a = vec![3, 1, 2]; a.retain(|x| *x > 1); a");
    agree("let mut a = vec![3, 1, 2]; a.retain(|x| *x > 1); a");
    agree("let mut a = vec![3, 1, 2]; a.sort_by(|x, y| y.cmp(x)); a");
}

#[test]
fn a_native_can_call_a_named_function_back() {
    lowered("fn keep(x: &i64) -> bool { *x > 1 } let mut a = vec![3, 1, 2]; a.retain(keep); a");
    agree("fn keep(x: &i64) -> bool { *x > 1 } let mut a = vec![3, 1, 2]; a.retain(keep); a");
}

/// The adapters the lowering keeps inside the program.
#[test]
fn compiled_code_can_call_a_closure() {
    lowered("let a = vec![1, 2, 3]; a.iter().map(|x| x * 2).collect::<Vec<i64>>()");
    agree("let a = vec![1, 2, 3]; a.iter().map(|x| x * 2).collect::<Vec<i64>>()");
    agree("let a = vec![1, 2, 3, 4]; a.into_iter().filter(|x| x % 2 == 0).collect::<Vec<i64>>()");
    agree("fn double(x: i64) -> i64 { x * 2 } let a = vec![1, 2, 3]; a.into_iter().map(double).collect::<Vec<i64>>()");
}

/// A bare function name is a function pointer, not a variable read.
#[test]
fn a_bare_function_name_is_a_pointer() {
    agree("fn double(x: i64) -> i64 { x * 2 } let mut r: i64 = 0; { let f = double; r = f(4); } r");
    agree("fn double(x: i64) -> i64 { x * 2 } fn apply(f: fn(i64) -> i64, v: i64) -> i64 { f(v) } apply(double, 4)");
    // A variable of the same name still wins over the function.
    agree("fn double(x: i64) -> i64 { x * 2 } let double: i64 = 7; double");
}

/// A capture arrives, has the right value, and arrives in the right place.
///
/// Subtraction rather than multiplication, because `n - x` and `x - n` are the
/// same two values in the other order and only arithmetic that cares can tell
/// them apart. A closure that captures a script local is still a fragment, so
/// the walker evaluates the capture and the order is its own.
#[test]
#[cfg(not(feature = "no_closure"))]
fn a_capturing_closure_resolves_and_sees_its_capture() {
    let engine = corpus::engine();
    agree("let n: i64 = 10; let a = vec![1, 2, 3]; a.iter().map(|x| x * n).collect::<Vec<i64>>()");

    let source = "let n: i64 = 10; let a = vec![1, 2, 3]; a.iter().map(|x| n - x).collect::<Vec<i64>>()";
    assert_eq!(walk(&engine, source), Ok("[9, 8, 7]".to_string()));
    assert_eq!(run(&engine, source), walk(&engine, source));
}

#[test]
#[cfg(not(feature = "unchecked"))]
fn a_callback_reaching_a_second_one_still_resolves() {
    // The limit is raised because reaching a chunk through rawr's dispatch
    // costs more call levels than reaching a script function does, and the
    // default in a debug build is 8. `a_callback_costs_more_call_levels`
    // measures the difference; this is about resolution, not depth.
    let mut engine = corpus::engine();
    engine.set_max_call_levels(64);
    let source = "let a = vec![vec![1, 2], vec![3, 4]]; a.iter().map(|row| row.iter().map(|x| x + 1).collect::<Vec<i64>>()).collect::<Vec<Vec<i64>>>()";
    assert_eq!(walk(&engine, source), run(&engine, source));
}

/// `move` copies what the closure captures, so a write after the closure is
/// made does not reach it, however late the native calls it.
#[test]
#[cfg(not(feature = "no_closure"))]
fn a_moved_closure_called_back_keeps_what_it_copied() {
    let engine = corpus::engine();
    let source = "let mut n: i64 = 1; let f = move |x: i64| x * n; n = 10; vec![1, 2, 3].into_iter().map(f).collect::<Vec<i64>>()";
    assert_eq!(walk(&engine, source), Ok("[1, 2, 3]".to_string()));
    assert_eq!(run(&engine, source), walk(&engine, source));
}

#[test]
fn an_error_inside_a_callback_still_arrives() {
    let engine = corpus::engine();
    let err = run(&engine, r#"let a = vec![1, 2, 3]; a.iter().map(|x| -> i64 { panic!("{}", x) }).collect::<Vec<i64>>()"#).unwrap_err();
    assert!(err.contains("ErrorInFunctionCall"), "{err}");
}

/// Reaching a chunk through rawr's dispatch spends more of the call budget
/// than reaching a script function does, so a callback nests less deeply before
/// `max_call_levels` stops it.
///
/// Measured rather than asserted at a number, because the number is a property
/// of rawr's dispatch and would drift. What has to hold is the direction: we
/// are stricter, never laxer. A program that would have run out of budget on
/// the walker must not somehow keep going here.
#[test]
#[cfg(not(feature = "unchecked"))]
fn a_callback_costs_more_call_levels() {
    // `Vec<Vec<..i64..>>`, `levels` deep.
    fn vec_type(levels: usize) -> String {
        format!("{}i64{}", "Vec<".repeat(levels), ">".repeat(levels))
    }
    // `levels` closures nested through `map`, each one a boundary, over a
    // vector nested to match.
    fn mapped(receiver: &str, levels: usize) -> String {
        let param = format!("v{levels}");
        let body = if levels == 1 { format!("{param} + 1") } else { mapped(&param, levels - 1) };
        format!("{receiver}.into_iter().map(|{param}| {body}).collect::<{}>()", vec_type(levels))
    }

    let deepest = |depth: usize, run: &dyn Fn(&Engine, &str) -> Result<String, String>| {
        let source = format!("let a = {}1{}; {}", "vec![".repeat(depth), "]".repeat(depth), mapped("a", depth));
        (1..=64)
            .find(|levels| {
                let mut engine = corpus::engine();
                engine.set_max_call_levels(*levels);
                run(&engine, &source).is_ok()
            })
            .unwrap_or(usize::MAX)
    };

    for depth in 1..=3 {
        let walker = deepest(depth, &walk);
        let vm = deepest(depth, &run);
        println!("{depth} nested callback(s): walker needs {walker}, we need {vm}");
        assert!(walker < usize::MAX, "depth {depth}: the walker never ran it, so there is nothing to compare");
        assert!(
            vm >= walker,
            "depth {depth}: we ran at {vm} levels where the walker needed {walker}, \
             so a budget the walker respects is not being enforced",
        );
    }
}

#[test]
fn without_the_wrappers_the_pointer_does_not_resolve() {
    // The whole reason `eval_with_callbacks` exists. A plain eval leaves rawr
    // nowhere to look, and the failure is a lookup failure rather than
    // anything worse.
    //
    //
    // A program holding compiler-owned support can answer differently, and
    // worse: `sort_by` without the wrappers leaves the vector unsorted and
    // reports nothing. This one is chosen to need no support at all.
    let engine = corpus::engine();
    let source = "let mut a = vec![3, 1, 2]; a.retain(|x| *x > 1); a";
    let ast = engine.compile(source).unwrap();
    let program = Compiler::new().compile(&ast);
    assert!(program.write().is_ok(), "the premise is a program with no support: {:?}", program.write());

    let err = Vm::new(&engine).eval_with_scope(&mut Scope::new(), &program).unwrap_err();
    assert!(format!("{err:?}").contains("ErrorFunctionNotFound"), "{err}");
}
