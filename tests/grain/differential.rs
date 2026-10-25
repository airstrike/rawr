//! The VM must mean exactly what rawr means.
//!
//! Every corpus script is evaluated twice against the same `Engine` — once
//! through `eval_ast_with_scope`, once through the VM — and the two runs must
//! agree on the result, on the error (variant *and* position), and on the scope
//! they leave behind.
//!
//! The scope check is not incidental. Rawr evaluates a program's top-level
//! statements without rewinding, so `let` at the top level outlives the run and
//! is observable by the caller. A VM that manages its own frames could return
//! the right value and still get that wrong.

use super::corpus;

use rawr::grain::{Compiler, Vm};
use rawr::{Dynamic, Engine, Scope};

/// What a run produced, in a form two runs can be compared on.
///
/// `Dynamic` and `EvalAltResult` have no `PartialEq`, so this compares their
/// `Debug` rendering. That is stricter than value equality, not looser: it
/// distinguishes `1` from `1.0`, and it includes error positions.
#[derive(Debug, PartialEq, Eq)]
struct Outcome {
    result: Result<String, String>,
    scope: Vec<(String, String)>,
}

fn snapshot_scope(scope: &Scope) -> Vec<(String, String)> {
    scope.iter_raw().map(|(name, _, value)| (name.to_string(), format!("{value:?}"))).collect()
}

fn run_stock(engine: &Engine, source: &str) -> Outcome {
    let mut scope = Scope::new();
    let result = engine.compile(source).map_err(|err| format!("{err:?}")).and_then(|ast| {
        engine
            .eval_ast_with_scope::<Dynamic>(&mut scope, &ast)
            .map(|value| format!("{value:?}"))
            .map_err(|err| format!("{err:?}"))
    });

    Outcome { result, scope: snapshot_scope(&scope) }
}

fn run_vm(engine: &Engine, source: &str) -> Outcome {
    let mut scope = Scope::new();
    let result = engine.compile(source).map_err(|err| format!("{err:?}")).and_then(|ast| {
        let program = Compiler::new().compile(&ast);
        // A program that can hand a pointer to a native has to be run the
        // way such a program is meant to be run, or the comparison is
        // against a configuration nobody would ship.
        if program.makes_fn_pointers() {
            let program = program.into_shared();
            Vm::new(engine).eval_with_callbacks(&mut scope, &program)
        } else {
            Vm::new(engine).eval_with_scope(&mut scope, &program)
        }
        .map(|value| format!("{value:?}"))
        .map_err(|err| format!("{err:?}"))
    });

    Outcome { result, scope: snapshot_scope(&scope) }
}

#[test]
fn vm_agrees_with_the_engine() {
    let engine = corpus::engine();

    let mut failures = Vec::new();

    for case in corpus::CASES.iter().filter(|c| applies_to_this_build(c.name)) {
        let stock = run_stock(&engine, case.source);
        let vm = run_vm(&engine, case.source);

        if stock != vm {
            failures.push(format!(
                "\n=== {} ===\n  source: {}\n  rawr:   {:?}\n  vm:     {:?}\n  \
                 rawr scope: {:?}\n  vm scope:   {:?}",
                case.name, case.source, stock.result, vm.result, stock.scope, vm.scope,
            ));
        }
    }

    let applicable = corpus::CASES.iter().filter(|c| applies_to_this_build(c.name)).count();
    assert!(failures.is_empty(), "{} of {applicable} corpus scripts diverged:{}", failures.len(), failures.join(""),);
}

/// The corpus is only worth anything if the comparison can actually fail.
///
/// Guards against the harness silently degrading into a tautology — comparing
/// two identical code paths, or stringifying everything into the same value.
#[test]
fn harness_detects_a_real_difference() {
    let engine = corpus::engine();

    assert_ne!(run_stock(&engine, "1 + 1"), run_stock(&engine, "1 + 2"), "differing results must compare unequal",);
    assert_ne!(run_stock(&engine, "1"), run_stock(&engine, "1.0"), "int and float must not compare equal",);
    assert_ne!(run_stock(&engine, "let a: i64 = 1; a"), run_stock(&engine, "1"), "differing leftover scope must compare unequal",);
    // `no_position` compiles positions out, so there is no such thing as the
    // same error at a different one and nothing here to detect.
    #[cfg(not(feature = "no_position"))]
    assert_ne!(run_stock(&engine, "let a = vec![1]; a[9]"), run_stock(&engine, "let a = vec![1]; 0 + a[9]"), "the same error at a different position must compare unequal",);
}

/// A script that does not parse compares equal on both sides for the wrong
/// reason: two identical parse errors. Such a case tests nothing, and would sit
/// in the corpus looking like coverage.
#[test]
fn every_corpus_script_parses() {
    let engine = corpus::engine();

    let broken: Vec<_> = corpus::CASES
        .iter()
        .filter(|case| applies_to_this_build(case.name))
        .filter_map(|case| engine.compile(case.source).err().map(|err| format!("\n  {}: {err}", case.name)))
        .collect();

    assert!(broken.is_empty(), "{} corpus scripts do not parse, so they assert nothing:{}", broken.len(), broken.join(""),);
}

/// Whether a corpus case exercises anything on this build.
///
/// Distinct from [`MAY_FRAGMENT`], which is a tolerance: this says the syntax
/// is not in the language on this build at all. Defined beside the cases, in
/// `corpus`, because every harness that walks them needs the same answer.
use corpus::applies_to_this_build;

/// A case that errors unintentionally is nearly as weak as one that does not
/// parse: both sides agree on the failure, and the machinery the case was
/// written to exercise never runs. Cases that mean to fail say so in the name.
#[test]
fn only_error_cases_error() {
    let engine = corpus::engine();

    let surprises: Vec<_> = corpus::CASES
        .iter()
        .filter(|case| !case.name.starts_with("error_"))
        .filter(|case| applies_to_this_build(case.name))
        .filter_map(|case| match run_stock(&engine, case.source).result {
            Err(err) => Some(format!("\n  {}: {err}", case.name)),
            Ok(_) => None,
        })
        .collect();

    assert!(surprises.is_empty(), "{} cases fail without meaning to, so they exercise nothing:{}", surprises.len(), surprises.join(""),);
}

/// Corpus scripts allowed to leave a fragment behind.
///
/// A case that fragments and is not listed fails on arrival rather than
/// quietly joining a majority, which is the point of stating it this way round.
///
/// What is here today is work the compiler has not done yet rather than the
/// escape hatch: a closure that captures a script local, and a pattern that
/// takes an `Option` apart. Both still give the walker's answer, which
/// `vm_agrees_with_the_engine` checks, and neither can be written to an
/// artifact until it lowers.
///
/// `method_beats_free_fn` and `method_call_beats_free_fn` call a std function
/// that a script function shares the name of: the call reaches only the
/// runtime's functions, which `Op::Call` cannot say, so the walker runs it.
const MAY_FRAGMENT: &[&str] = &["match_option", "if_let", "closure_capture_read", "closure_capture_in_map", "method_beats_free_fn", "method_call_beats_free_fn"];

/// Every chunk the compiler emits must pass its own verifier.
///
/// The check that matters is depth agreement at merge points: one branch of a
/// conditional leaving a value where the other does not is invisible until a
/// program happens to take the unlucky path, and the differential corpus only
/// covers the paths it happens to exercise.
#[test]
fn every_compiled_chunk_verifies() {
    let engine = corpus::engine();

    let broken: Vec<_> = corpus::CASES
        .iter()
        .filter_map(|case| {
            let ast = engine.compile(case.source).ok()?;
            Compiler::new().compile(&ast).verify().err().map(|err| format!("\n  {}: {err:?}", case.name))
        })
        .collect();

    assert!(broken.is_empty(), "{} chunks failed verification:{}", broken.len(), broken.join(""),);
}

/// A chunk must declare the stack it uses, not the stack it might use.
///
/// The lowering's own estimate is one slot per instruction, which is safe and
/// wildly loose — and it is what the VM reserves from, and what the artifact
/// records. On a device with ~12KB to spend, reserving 25 `Dynamic` slots for a
/// chunk that stacks three is the difference worth closing.
#[test]
fn every_compiled_chunk_declares_the_stack_it_uses() {
    let engine = corpus::engine();

    let mut loose = Vec::new();
    let mut total_declared = 0usize;
    let mut total_ops = 0usize;

    for case in corpus::CASES {
        let Ok(ast) = engine.compile(case.source) else {
            continue;
        };
        let program = Compiler::new().compile(&ast);
        let Ok(high_water) = program.verify() else {
            continue;
        };

        let declared: Vec<u16> = std::iter::once(program.main().max_stack()).chain(program.functions().iter().map(|f| f.chunk.max_stack())).collect();

        total_declared += high_water.iter().map(|n| *n as usize).sum::<usize>();
        total_ops += program.code().len();

        if declared != high_water {
            loose.push(format!("\n  {}: declares {declared:?}, uses {high_water:?}", case.name,));
        }
    }

    println!("\n{total_declared} stack slots declared across {total_ops} bytes of code");

    assert!(loose.is_empty(), "{} chunks declare a stack they do not use:{}", loose.len(), loose.join(""),);
}

/// Residuals are the work left to do, so the count is the progress metric.
///
/// Prints the whole census so a change in coverage is visible, and pins the
/// cases that should already be at zero.
#[test]
fn residual_census() {
    let engine = corpus::engine();

    let mut total_nodes = 0usize;
    let mut at_zero = Vec::new();
    let mut remaining = Vec::new();
    let mut regressions = Vec::new();

    // Cases the build removed are counted on neither side, or the completeness
    // check below would read their absence as a corpus that stopped compiling.
    let applicable = corpus::CASES.iter().filter(|case| applies_to_this_build(case.name)).count();

    for case in corpus::CASES.iter().filter(|c| applies_to_this_build(c.name)) {
        let Ok(ast) = engine.compile(case.source) else {
            continue;
        };
        let program = Compiler::new().compile(&ast);
        let count = program.residual_count();
        let nodes = program.residual_nodes();
        total_nodes += nodes;

        if count == 0 {
            at_zero.push(case.name);
        } else {
            remaining.push((case.name, nodes));
            if !MAY_FRAGMENT.contains(&case.name) && applies_to_this_build(case.name) {
                regressions.push(format!("\n  {} leaves {count}", case.name));
            }
        }
    }

    println!("\n{} of {applicable} scripts fully lowered, {total_nodes} AST nodes still in fragments", at_zero.len(),);
    println!("\nfully lowered: {}", at_zero.join(", "));
    println!("\nremaining:");
    remaining.sort_by_key(|(_, count)| std::cmp::Reverse(*count));
    for (name, count) in &remaining {
        println!("  {count:>3}  {name}");
    }

    assert!(
        regressions.is_empty(),
        "{} scripts fragment that are not on `MAY_FRAGMENT`. Either the \
         construct regressed, or the case needs something the compiler does \
         not lower yet — say what, in the list:{}",
        regressions.len(),
        regressions.join(""),
    );

    // The other direction, which the check above cannot see: a corpus that
    // stopped compiling at all would have nothing to fragment and would pass.
    assert_eq!(at_zero.len(), applicable - MAY_FRAGMENT.len(), "some scripts did not compile, so they were counted as neither",);
}

/// A divergence the corpus found and cannot keep: moving a `Vec` out of a
/// local.
///
/// Reading a `Vec` or a `String` local by value moves it, and the lowered
/// program spells the move as a call to a support function the walker finds
/// and the VM does not. Pinned so the day it is fixed is noticed: then this
/// fails, and the case belongs back in the corpus as
/// `vec_empty_nested_in_computed` with its last `.clone()` removed.
#[test]
fn moving_a_vec_out_of_a_local_is_not_found_by_the_vm() {
    let engine = corpus::engine();
    let source = "let v: i64 = 7; let e: Vec<i64> = Vec::new(); vec![e.clone(), vec![v], e]";

    assert_eq!(run_stock(&engine, source).result, Ok("[[], [7], []]".to_string()));
    let vm = run_vm(&engine, source).result.unwrap_err();
    assert!(vm.contains("ErrorFunctionNotFound") && vm.contains("__rawr_move"), "{vm}");
}

/// A divergence the generated scripts found: `to_string` straight on a
/// string's length.
///
/// The walker answers `"2"`. The VM reaches a property write on the string and
/// fails, which no script asked for. Naming the length first, or adding to it,
/// avoids it. Pinned so the day it is fixed is noticed, and so the generator
/// can go back to writing `.to_string()`.
#[test]
fn to_string_on_a_string_length_is_refused_by_the_vm() {
    let engine = corpus::engine();
    let source = r#"let s = "ab"; (s.len() as i64).to_string()"#;

    assert_eq!(run_stock(&engine, source).result, Ok(r#""2""#.to_string()));
    let vm = run_vm(&engine, source).result.unwrap_err();
    assert!(vm.contains("ErrorDotExpr") && vm.contains("bytes"), "{vm}");
}
